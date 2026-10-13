//! Adversarial checks for the configured sandbox. These run as repository code.
use crate::{
    process,
    sandbox::{self, SandboxRunner},
    types::Settings,
};
use anyhow::{ensure, Result};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

pub fn sandbox_probe(root: &Path, settings: &Settings, cancel: &process::Cancel) -> Result<String> {
    sandbox::check_runtime(cancel)?;
    let image = sandbox::image_id(&settings.image, cancel)?;
    let dir = root.join(format!("sandbox-probe-{}", crate::types::now()));
    fs::create_dir_all(dir.join("source/src"))?;
    fs::create_dir_all(dir.join("vendor"))?;
    let sentinel = dir.join("HOST_SECRET_DO_NOT_MOUNT");
    fs::write(&sentinel, "GPUFold-host-secret")?;
    let sentinel = sentinel.canonicalize()?;
    let manifest = "[package]\nname=\"sandbox_probe\"\nversion=\"0.1.0\"\nedition=\"2021\"\n";
    fs::write(dir.join("source/Cargo.toml"), manifest)?;
    fs::write(
        dir.join("source/Cargo.lock"),
        "version = 4\n[[package]]\nname = \"sandbox_probe\"\nversion = \"0.1.0\"\n",
    )?;
    let code = format!(
        r#"
#[test] fn filesystem_network_environment_and_devices() {{
    assert!(std::fs::read_to_string({sentinel}).is_err(), "host secret exposed");
    assert!(std::env::var("GPUFOLD_HOST_SECRET").is_err(), "host environment exposed");
    assert!(!std::path::Path::new("/var/run/docker.sock").exists());
    assert!(!std::path::Path::new("/run/podman/podman.sock").exists());
    assert!(!std::path::Path::new("/dev/nvidia0").exists());
    let interfaces=std::fs::read_to_string("/proc/net/dev").unwrap();
    for line in interfaces.lines().skip(2) {{assert!(line.trim_start().starts_with("lo:"), "network interface exposed: {{line}}");}}
    let address="203.0.113.1:443".parse().unwrap();
    assert!(std::net::TcpStream::connect_timeout(&address,std::time::Duration::from_millis(300)).is_err());
    std::fs::write("source-write-probe","sandbox-only").unwrap();
    println!("SANDBOX_ISOLATION_PASS");
}}
"#,
        sentinel = serde_json::to_string(&sentinel.to_string_lossy())?
    );
    fs::write(dir.join("source/src/lib.rs"), &code)?;
    fs::write(dir.join("source/build.rs"),format!("fn main(){{assert!(std::fs::read_to_string({}).is_err());assert!(std::env::var(\"GPUFOLD_HOST_SECRET\").is_err());}}",serde_json::to_string(&sentinel.to_string_lossy())?))?;
    let observed =
        sandbox::PodmanRunner.test(&dir.join("source"), &dir.join("vendor"), &image, cancel)?;
    ensure!(
        observed.exit_code == Some(0),
        "Sandbox isolation checks failed: {}",
        observed.output
    );
    ensure!(
        !dir.join("source/source-write-probe").exists(),
        "Sandbox changed the host snapshot"
    );
    fs::write(
        dir.join("source/src/lib.rs"),
        "#[test] fn hangs(){std::thread::sleep(std::time::Duration::from_secs(120));}\n",
    )?;
    let stop = process::cancel_token();
    let signal = stop.clone();
    let timer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        signal.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let start = Instant::now();
    let stopped =
        sandbox::PodmanRunner.test(&dir.join("source"), &dir.join("vendor"), &image, &stop);
    timer
        .join()
        .map_err(|_| anyhow::anyhow!("Cancel timer failed"))?;
    ensure!(
        stopped.is_err() && start.elapsed() < Duration::from_secs(15),
        "Sandbox cancellation exceeded cleanup budget"
    );
    ensure!(
        fs::read_to_string(&sentinel)? == "GPUFold-host-secret",
        "Host sentinel changed"
    );
    Ok(format!("PASS: build-script host read denied; host environment absent; no network interface beyond loopback; no GPU/runtime socket; writable sandbox copy did not modify snapshot; cancellation and cleanup {} ms. Image {image}",start.elapsed().as_millis()))
}
