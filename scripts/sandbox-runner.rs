//! Trusted container entrypoint. Copies the read-only snapshot into bounded tmpfs.
use std::{
    fs,
    path::Path,
    process::{Command, ExitCode},
};
fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(std::io::Error::other("symlink in snapshot"));
        }
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            return Err(std::io::Error::other("special file in snapshot"));
        }
    }
    Ok(())
}
fn run() -> std::io::Result<i32> {
    if std::env::args_os().len() != 1 {
        return Err(std::io::Error::other("entrypoint takes no arguments"));
    }
    copy(Path::new("/snapshot"), Path::new("/tmp/work"))?;
    let status = Command::new("/usr/local/cargo/bin/cargo")
        .env_clear()
        .env("PATH", "/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin")
        .env("HOME", "/tmp")
        .env("CARGO_HOME", "/tmp/cargo")
        .env("RUSTUP_HOME", "/usr/local/rustup")
        .env("CARGO_TARGET_DIR", "/tmp/target")
        .env("CARGO_NET_OFFLINE", "true")
        .current_dir("/tmp/work")
        .args([
            "test",
            "--offline",
            "--locked",
            "--config",
            "source.crates-io.replace-with=\"vendored-sources\"",
            "--config",
            "source.vendored-sources.directory=\"/vendor\"",
            "--",
            "--test-threads=1",
        ])
        .status()?;
    Ok(status.code().unwrap_or(125))
}
fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("Sandbox preparation failed: {e}");
            ExitCode::from(125)
        }
    }
}
