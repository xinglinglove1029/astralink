use crate::models::ConnectionProfile;
use argon2::Argon2;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u8,
    pub profiles: Vec<BackupProfile>,
}

#[derive(Serialize, Deserialize)]
pub struct BackupProfile {
    pub profile: ConnectionProfile,
    pub password: Option<String>,
    pub passphrase: Option<String>,
    pub totp_secret: Option<String>,
}

impl BackupProfile {
    pub fn from_resolved(profile: ConnectionProfile) -> Self {
        Self {
            password: profile.password.clone(),
            passphrase: profile.passphrase.clone(),
            totp_secret: profile.totp_secret.clone(),
            profile,
        }
    }

    pub fn into_profile(self) -> ConnectionProfile {
        let mut p = self.profile;
        p.password = Some(self.password.unwrap_or_default());
        p.passphrase = Some(self.passphrase.unwrap_or_default());
        p.totp_secret = Some(self.totp_secret.unwrap_or_default());
        p
    }
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    format: String,
    salt: String,
    nonce: String,
    ciphertext: String,
}

fn key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    if passphrase.chars().count() < 12 || passphrase.len() > 1024 {
        return Err("备份口令需至少 12 个字符，最多 1024 字节".into());
    }
    let mut output = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut output)
        .map_err(|_| "无法派生备份密钥".to_string())?;
    Ok(output)
}

pub fn encrypt(snapshot: &Snapshot, passphrase: &str) -> Result<Vec<u8>, String> {
    let plaintext = serde_json::to_vec(snapshot).map_err(|_| "无法序列化备份".to_string())?;
    if plaintext.len() > MAX_BYTES {
        return Err("备份内容超过大小限制".into());
    }
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    rand::rng().fill_bytes(&mut salt);
    rand::rng().fill_bytes(&mut nonce);
    let secret = key(passphrase, &salt)?;
    let cipher =
        XChaCha20Poly1305::new_from_slice(&secret).map_err(|_| "无法初始化加密器".to_string())?;
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|_| "无法加密备份".to_string())?;
    serde_json::to_vec(&Envelope {
        format: "astralink-backup-v1".into(),
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(ciphertext),
    })
    .map_err(|_| "无法编码备份".into())
}

pub fn decrypt(bytes: &[u8], passphrase: &str) -> Result<Snapshot, String> {
    if bytes.len() > MAX_BYTES * 2 {
        return Err("远端备份超过大小限制".into());
    }
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|_| "备份格式无效".to_string())?;
    if envelope.format != "astralink-backup-v1" {
        return Err("备份版本不受支持".into());
    }
    let salt = STANDARD
        .decode(envelope.salt)
        .map_err(|_| "备份格式无效".to_string())?;
    let nonce = STANDARD
        .decode(envelope.nonce)
        .map_err(|_| "备份格式无效".to_string())?;
    let ciphertext = STANDARD
        .decode(envelope.ciphertext)
        .map_err(|_| "备份格式无效".to_string())?;
    if salt.len() != 16 || nonce.len() != 24 || ciphertext.len() > MAX_BYTES + 16 {
        return Err("备份格式无效".into());
    }
    let secret = key(passphrase, &salt)?;
    let cipher =
        XChaCha20Poly1305::new_from_slice(&secret).map_err(|_| "无法初始化解密器".to_string())?;
    let plaintext = cipher
        .decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| "备份口令不正确或内容已损坏".to_string())?;
    let snapshot: Snapshot =
        serde_json::from_slice(&plaintext).map_err(|_| "备份内容无效".to_string())?;
    if snapshot.version != 1 || snapshot.profiles.len() > 1000 {
        return Err("备份内容不受支持".into());
    }
    Ok(snapshot)
}

pub fn fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn webdav_client(url: &str) -> Result<reqwest::Client, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "WebDAV 地址无效".to_string())?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("请使用无凭据和查询参数的 HTTPS WebDAV 文件地址".into());
    }
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "无法创建 WebDAV 客户端".into())
}

pub async fn fetch(
    client: &reqwest::Client,
    url: &str,
    username: &str,
    password: &str,
) -> Result<Option<(Vec<u8>, Option<String>)>, String> {
    let response = client
        .get(url)
        .basic_auth(username, Some(password))
        .send()
        .await
        .map_err(|_| "无法连接 WebDAV".to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err("无法读取 WebDAV 备份，请检查地址和权限".into());
    }
    if response
        .content_length()
        .is_some_and(|size| size > (MAX_BYTES * 2) as u64)
    {
        return Err("远端备份超过大小限制".into());
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|header| header.to_str().ok())
        .map(ToOwned::to_owned);
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "无法读取远端备份".to_string())?;
    if bytes.len() > MAX_BYTES * 2 {
        return Err("远端备份超过大小限制".into());
    }
    Ok(Some((bytes.to_vec(), etag)))
}

pub async fn upload(
    client: &reqwest::Client,
    url: &str,
    username: &str,
    password: &str,
    bytes: Vec<u8>,
    previous: Option<&str>,
) -> Result<(), String> {
    let current = fetch(client, url, username, password).await?;
    let sent_fingerprint = fingerprint(&bytes);
    let mut request = client
        .put(url)
        .basic_auth(username, Some(password))
        .body(bytes);
    match (current, previous) {
        (None, None) => {
            request = request.header(reqwest::header::IF_NONE_MATCH, "*");
        }
        (Some((current, Some(etag))), Some(expected))
            if current_fingerprint_matches(&current, expected) =>
        {
            request = request.header(reqwest::header::IF_MATCH, etag);
        }
        _ => return Err("远端版本已变化或不支持安全覆盖，请重新预览".into()),
    }
    let response = request
        .send()
        .await
        .map_err(|_| "无法上传 WebDAV 备份".to_string())?;
    if !response.status().is_success() {
        return Err("WebDAV 拒绝写入，请检查权限或版本冲突".into());
    }
    let (stored, _) = fetch(client, url, username, password)
        .await?
        .ok_or("上传后无法读取远端备份")?;
    if fingerprint(&stored) != sent_fingerprint {
        return Err("远端备份校验失败，请重新预览".into());
    }
    Ok(())
}

fn current_fingerprint_matches(bytes: &[u8], expected: &str) -> bool {
    fingerprint(bytes) == expected
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backup_roundtrip_does_not_leak_secrets_and_rejects_tampering() {
        let profile = ConnectionProfile {
            name: "fixture".into(),
            password: Some("sensitive-fixture".into()),
            ..Default::default()
        };
        let snapshot = Snapshot {
            version: 1,
            profiles: vec![BackupProfile::from_resolved(profile)],
        };
        let encrypted = encrypt(&snapshot, "fixture-passphrase-123").unwrap();
        assert!(!String::from_utf8_lossy(&encrypted).contains("sensitive-fixture"));
        assert_eq!(
            decrypt(&encrypted, "fixture-passphrase-123")
                .unwrap()
                .profiles[0]
                .password
                .as_deref(),
            Some("sensitive-fixture")
        );
        assert!(decrypt(&encrypted, "wrong-passphrase-123").is_err());
        let cleared = decrypt(&encrypted, "fixture-passphrase-123")
            .unwrap()
            .profiles
            .remove(0)
            .into_profile();
        assert_eq!(cleared.passphrase.as_deref(), Some(""));
        assert_eq!(cleared.totp_secret.as_deref(), Some(""));
        let mut altered: Envelope = serde_json::from_slice(&encrypted).unwrap();
        let replacement = if altered.ciphertext.starts_with('A') {
            "B"
        } else {
            "A"
        };
        altered.ciphertext.replace_range(0..1, replacement);
        assert!(decrypt(
            &serde_json::to_vec(&altered).unwrap(),
            "fixture-passphrase-123"
        )
        .is_err());
    }
    #[test]
    fn webdav_endpoint_requires_https_without_embedded_credentials() {
        assert!(webdav_client("https://backup.example/astralink.json").is_ok());
        assert!(webdav_client("http://backup.example/astralink.json").is_err());
        assert!(webdav_client("https://user:pass@backup.example/file").is_err());
    }
}
