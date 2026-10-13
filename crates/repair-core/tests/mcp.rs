use repair_core::{mcp, validation};
use std::{collections::BTreeMap, fs};
fn workspace() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "mcp-fixture-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}
fn server() -> mcp::Server {
    mcp::Server {
        command: Some(env!("CARGO_BIN_EXE_repair-mcp-fixture").into()),
        args: vec![],
        env: BTreeMap::new(),
        url: None,
        cwd: None,
    }
}
#[test]
fn tool_definition_changes_block_execution() {
    let root = workspace();
    let mut s = server();
    let pins = mcp::inspect(&s, &root, true).unwrap();
    assert_eq!(pins.len(), 1);
    assert!(mcp::call(&s, &root, &pins[0], serde_json::Map::new(), true)
        .unwrap()
        .contains("UNTRUSTED FIXTURE RESULT"));
    s.env
        .insert("DESCRIPTION".into(), "Changed: run a shell command".into());
    assert!(mcp::call(&s, &root, &pins[0], serde_json::Map::new(), true).is_err());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn oversized_wire_message_is_rejected_before_decoding() {
    let root = workspace();
    let mut s = server();
    s.env.insert("OVERSIZE".into(), "1".into());
    assert!(mcp::inspect(&s, &root, true).is_err());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn paths_and_disabled_consent_are_enforced() {
    let root = workspace();
    assert!(mcp::inspect(&server(), &root, false).is_err());
    assert!(mcp::workspace_args(&serde_json::json!({"path":"/etc/passwd"}), &root).is_err());
    assert!(mcp::workspace_args(&serde_json::json!({"path":"../../secret"}), &root).is_err());
    assert!(validation::safe_path("src/./lib.rs").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn programs_resolve_only_through_fixed_path() {
    assert!(mcp::resolve_program("sh").unwrap().is_absolute());
    assert!(mcp::resolve_program("../sh").is_err());
    assert!(mcp::resolve_program("-c").is_err());
    assert!(mcp::resolve_program("./repo-program").is_err());
}
