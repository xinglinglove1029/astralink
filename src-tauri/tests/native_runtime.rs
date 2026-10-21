#[test]
fn native_api_can_import_without_launching_a_window() {
    let preview = astralink_lib::importers::parse(
        "mcp-json",
        r#"{"fixture":{"host":"example.invalid","port":22,"username":"ops"}}"#,
        std::path::Path::new("fixture.json"),
        std::path::Path::new("C:/Users/test"),
    )
    .unwrap();
    assert_eq!(preview.profiles.len(), 1);
    assert_eq!(preview.profiles[0].host, "example.invalid");
}
