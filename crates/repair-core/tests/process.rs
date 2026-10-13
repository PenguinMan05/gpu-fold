#[cfg(unix)]
#[test]
fn inherited_pipes_do_not_prevent_completion() {
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.args(["-c", "sleep 30 & printf done"]);
    let start = std::time::Instant::now();
    let o = repair_core::process::run(
        cmd,
        vec!["trusted pipe fixture".into()],
        std::time::Duration::from_secs(5),
        &repair_core::process::cancel_token(),
    )
    .unwrap();
    assert_eq!(o.exit_code, Some(0));
    assert!(start.elapsed() < std::time::Duration::from_secs(3));
}
#[cfg(unix)]
#[test]
fn timeout_terminates_the_process_tree() {
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.args(["-c", "sleep 30 & wait"]);
    let start = std::time::Instant::now();
    assert!(repair_core::process::run(
        cmd,
        vec!["trusted timeout fixture".into()],
        std::time::Duration::from_millis(100),
        &repair_core::process::cancel_token()
    )
    .is_err());
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
}
