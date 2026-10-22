use super::*;
use quick_xml::{
    events::{BytesStart, Event},
    Reader, XmlVersion,
};
use std::collections::HashMap;

#[derive(Default)]
struct Frame {
    name: String,
    fields: HashMap<String, String>,
}

fn attr(event: &BytesStart<'_>) -> Result<Option<String>, String> {
    for result in event.attributes() {
        let value = result.map_err(|_| "SecureCRT XML 属性无效")?;
        if value.key.as_ref() == "name" {
            return value
                .normalized_value(XmlVersion::Implicit1_0)
                .map(|text| Some(text.into_owned()))
                .map_err(|_| "SecureCRT XML 名称无效".into());
        }
    }
    Ok(None)
}

fn value_tag(name: &str) -> bool {
    matches!(name, "string" | "dword")
}

fn session(frame: &Frame, parents: &[Frame], preview: &mut ImportPreview) {
    let Some(root) = parents.iter().position(|parent| parent.name == "Sessions") else {
        return;
    };
    if !frame.fields.contains_key("Protocol Name") {
        return;
    }
    if !frame.fields["Protocol Name"].eq_ignore_ascii_case("SSH2") {
        unsupported(preview, "仅导入 SecureCRT SSH2 会话", Some(&frame.name));
        return;
    }
    let mut profile = new_profile(&frame.name, "securecrt");
    profile.host = frame
        .fields
        .get("Hostname")
        .map(String::as_str)
        .unwrap_or_default()
        .into();
    profile.username = frame
        .fields
        .get("Username")
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("root")
        .into();
    profile.port = frame
        .fields
        .get("[SSH2] Port")
        .or_else(|| frame.fields.get("Port"))
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(22);
    profile.group = parents[root + 1..]
        .iter()
        .map(|p| p.name.as_str())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    issue(
        preview,
        "warning",
        "SecureCRT 导出不包含可用密码，请补充凭据",
        Some(&profile.name),
    );
    normalize(profile, preview);
}

pub(super) fn parse(input: &str, preview: &mut ImportPreview) -> Result<(), String> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<Frame> = Vec::new();
    let mut current_field: Option<String> = None;
    let mut current_value = String::new();
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|_| "SecureCRT XML 格式无效")?
        {
            Event::Start(e) if e.name().as_ref() == "key" => {
                if stack.len() > 32 {
                    return Err("SecureCRT XML 层级过多".into());
                }
                stack.push(Frame {
                    name: attr(&e)?.unwrap_or_default(),
                    ..Frame::default()
                });
            }
            Event::Start(e) if value_tag(e.name().as_ref()) => {
                current_field = attr(&e)?;
                current_value.clear();
            }
            Event::Empty(e) if value_tag(e.name().as_ref()) => {
                if let (Some(frame), Some(name)) = (stack.last_mut(), attr(&e)?) {
                    frame.fields.insert(name, String::new());
                }
            }
            Event::Text(e) if current_field.is_some() => {
                let content = e.xml10_content();
                let text =
                    quick_xml::escape::unescape(&content).map_err(|_| "SecureCRT XML 文本无效")?;
                current_value.push_str(&text);
            }
            Event::CData(e) if current_field.is_some() => {
                current_value.push_str(e.as_ref());
            }
            Event::End(e) if value_tag(e.name().as_ref()) => {
                if let (Some(frame), Some(name)) = (stack.last_mut(), current_field.take()) {
                    frame.fields.insert(name, current_value.trim().to_owned());
                }
                current_value.clear();
            }
            Event::End(e) if e.name().as_ref() == "key" => {
                if let Some(frame) = stack.pop() {
                    session(&frame, &stack, preview);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if !stack.is_empty() {
        return Err("SecureCRT XML 结构不完整".into());
    }
    Ok(())
}
