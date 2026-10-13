use crate::{github, process, repository, types::*, validation};
use anyhow::{ensure, Context, Result};
use flate2::read::GzDecoder;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
    process::Command,
    time::Duration,
};

pub trait SandboxRunner {
    fn test(
        &self,
        source: &Path,
        vendor: &Path,
        image: &str,
        cancel: &process::Cancel,
    ) -> Result<Observation>;
}
pub struct PodmanRunner;
fn podman() -> Command {
    let mut c = Command::new("podman");
    c.env_clear().env("PATH", "/usr/local/bin:/usr/bin:/bin");
    for n in [
        "HOME",
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
    ] {
        if let Some(v) = std::env::var_os(n) {
            c.env(n, v);
        }
    }
    c
}
pub fn check_runtime(cancel: &process::Cancel) -> Result<()> {
    let mut c = podman();
    c.args(["info", "--format", "{{.Host.Security.Rootless}}"]);
    let o = process::success(process::run(
        c,
        vec!["podman info rootless".into()],
        Duration::from_secs(20),
        cancel,
    )?)?;
    ensure!(
        o.output.trim() == "true",
        "A working rootless Podman installation is required. No unsafe host test fallback."
    );
    Ok(())
}
pub fn image_id(name: &str, cancel: &process::Cancel) -> Result<String> {
    ensure!(
        !name.is_empty() && !name.starts_with('-') && !name.chars().any(char::is_whitespace),
        "Invalid image name"
    );
    let mut c = podman();
    c.args(["image", "inspect", "--format", "{{.Id}}", name]);
    let o = process::success(process::run(
        c,
        vec!["podman inspect sandbox image".into()],
        Duration::from_secs(20),
        cancel,
    )?)?;
    normalize_image_id(o.output.trim())
}
pub fn normalize_image_id(id: &str) -> Result<String> {
    let hex = id.strip_prefix("sha256:").unwrap_or(id);
    ensure!(
        hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()),
        "Invalid local image digest"
    );
    Ok(format!("sha256:{hex}"))
}
pub fn fetch_vendor(source: &Path, vendor: &Path, cancel: &process::Cancel) -> Result<()> {
    ensure!(
        fs::metadata(source.join("Cargo.lock"))
            .context("A committed Cargo.lock is required")?
            .len()
            <= 2_000_000,
        "Lockfile exceeds size cap"
    );
    let lock = fs::read_to_string(source.join("Cargo.lock"))
        .context("A committed Cargo.lock is required for verified offline execution")?;
    let data: toml::Value = toml::from_str(&lock)?;
    let packages = data
        .get("package")
        .and_then(toml::Value::as_array)
        .context("Malformed lockfile")?;
    ensure!(packages.len() <= 1000, "Dependency count over cap");
    if vendor.exists() {
        fs::remove_dir_all(vendor)?;
    }
    fs::create_dir_all(vendor)?;
    let mut total = 0u64;
    for p in packages {
        process::check(cancel)?;
        let Some(source) = p.get("source").and_then(toml::Value::as_str) else {
            continue;
        };
        ensure!(
            source == "registry+https://github.com/rust-lang/crates.io-index",
            "Only crates.io registry dependencies are supported; git/custom registries are blocked"
        );
        let name = p
            .get("name")
            .and_then(toml::Value::as_str)
            .context("Dependency name")?;
        let version = p
            .get("version")
            .and_then(toml::Value::as_str)
            .context("Dependency version")?;
        ensure!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
                && version
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')),
            "Unsafe dependency name/version"
        );
        let checksum = p
            .get("checksum")
            .and_then(toml::Value::as_str)
            .context("Dependency missing SHA256")?;
        ensure!(
            checksum.len() == 64 && checksum.chars().all(|c| c.is_ascii_hexdigit()),
            "Invalid package checksum"
        );
        // Fixed destination; redirects disabled. Repository configuration never chooses a host.
        let url = format!("https://static.crates.io/crates/{name}/{name}-{version}.crate");
        let mut r = github::client()?.get(url).send()?;
        ensure!(
            r.status().is_success(),
            "Registry download failed for {name}-{version}"
        );
        let mut bytes = vec![];
        r.by_ref().take(32_000_001).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 32_000_000, "Crate exceeds download cap");
        total += bytes.len() as u64;
        ensure!(total <= 300_000_000, "Dependency download budget exceeded");
        ensure!(
            validation::digest(&bytes) == checksum,
            "Lockfile checksum mismatch for {name}"
        );
        let dest = vendor.join(format!("{name}-{version}"));
        repository::unpack(GzDecoder::new(&bytes[..]), &dest, true, 64_000_000, 10000)?;
        let mut hashes = BTreeMap::new();
        for e in walkdir::WalkDir::new(&dest) {
            let e = e?;
            if e.file_type().is_file() {
                let rel = e
                    .path()
                    .strip_prefix(&dest)?
                    .to_string_lossy()
                    .replace('\\', "/");
                if rel != ".cargo-checksum.json" {
                    hashes.insert(rel, validation::digest(&fs::read(e.path())?));
                }
            }
        }
        fs::write(
            dest.join(".cargo-checksum.json"),
            serde_json::to_vec(&json!({"files":hashes,"package":checksum}))?,
        )?;
        let expanded = walkdir::WalkDir::new(vendor)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .try_fold(0u64, |sum, e| -> Result<u64> {
                Ok(sum + e.metadata()?.len())
            })?;
        ensure!(
            expanded <= 500_000_000,
            "Expanded dependency budget exceeded"
        );
    }
    Ok(())
}
pub fn sanitized_copy(source: &Path, dest: &Path) -> Result<()> {
    repository::copy_tree(source, dest)?;
    let configs = walkdir::WalkDir::new(dest)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            let n = e.file_name().to_string_lossy();
            n == "rust-toolchain"
                || n == "rust-toolchain.toml"
                || ((n == "config" || n == "config.toml")
                    && e.path()
                        .parent()
                        .is_some_and(|p| p.file_name().is_some_and(|n| n == ".cargo")))
        })
        .map(|e| e.path().to_path_buf())
        .collect::<Vec<_>>();
    for p in configs {
        fs::remove_file(p)?;
    }
    Ok(())
}
impl SandboxRunner for PodmanRunner {
    fn test(
        &self,
        source: &Path,
        vendor: &Path,
        image: &str,
        cancel: &process::Cancel,
    ) -> Result<Observation> {
        let name = format!(
            "rust-repair-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let src = source.canonicalize()?;
        let vendor = vendor.canonicalize()?;
        ensure!(
            !src.to_string_lossy().contains(':') && !vendor.to_string_lossy().contains(':'),
            "Container paths cannot contain colons"
        );
        let mut c = podman();
        c.args([
            "run",
            "--rm",
            "--pull=never",
            "--name",
            &name,
            "--network=none",
            "--read-only",
            "--user",
            "65534:65534",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--pids-limit=256",
            "--memory=4g",
            "--cpus=2",
            "--tmpfs",
            "/tmp:rw,nosuid,nodev,size=1073741824,mode=1777",
            "--volume",
        ])
        .arg(format!("{}:/snapshot:ro", src.display()))
        .arg("--volume")
        .arg(format!("{}:/vendor:ro", vendor.display()))
        .args([
            "--workdir",
            "/tmp",
            "--env",
            "CARGO_HOME=/tmp/cargo",
            "--env",
            "RUSTUP_HOME=/usr/local/rustup",
            "--env",
            "CARGO_TARGET_DIR=/tmp/target",
            "--env",
            "CARGO_NET_OFFLINE=true",
            "--env",
            "HOME=/tmp",
            "--entrypoint",
            "/usr/local/bin/repair-sandbox",
            image,
        ]);
        let result = process::run(
            c,
            vec![
                "(sandbox /tmp/work)".into(),
                "cargo".into(),
                "test".into(),
                "--offline".into(),
                "--locked".into(),
                "--".into(),
                "--test-threads=1".into(),
            ],
            Duration::from_secs(600),
            cancel,
        );
        if result.is_err() {
            let mut cleanup = podman();
            cleanup.args(["rm", "--force", &name]);
            let _ = process::run(
                cleanup,
                vec!["podman stop sandbox".into()],
                Duration::from_secs(10),
                &process::cancel_token(),
            );
        }
        result
    }
}
fn failing_names(o: &Observation) -> BTreeSet<String> {
    o.output
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with("test ") && l.ends_with(" ... FAILED") {
                Some(
                    l.trim_start_matches("test ")
                        .trim_end_matches(" ... FAILED")
                        .into(),
                )
            } else {
                None
            }
        })
        .collect()
}
pub fn evaluate_baseline(reproduction: &Reproduction, baseline: Vec<Observation>) -> Evidence {
    let mut e = Evidence {
        level: 1,
        reason: "Located claim; tests did not reproduce the bug".into(),
        baseline,
        after: vec![],
    };
    if e.baseline.len() != 2 {
        e.reason = "Expected two baseline runs".into();
        return e;
    }
    if e.baseline.iter().any(|o| o.truncated) {
        e.reason = "Output was truncated; evidence withheld".into();
        return e;
    }
    if e.baseline.iter().any(|o| {
        o.exit_code.is_none()
            || o.output.contains("could not compile")
            || o.output.contains("error[E")
    }) {
        e.reason = "Baseline did not compile or was interrupted".into();
        return e;
    }
    let a = failing_names(&e.baseline[0]);
    let b = failing_names(&e.baseline[1]);
    if a != b {
        e.reason = "Baseline failure set changed: possible flaky tests".into();
        return e;
    }
    if !a.contains(&reproduction.test_name)
        || e.baseline
            .iter()
            .any(|o| o.exit_code == Some(0) || !o.output.contains(&reproduction.failure_marker))
    {
        e.reason = "The reproduction did not fail consistently for its asserted reason".into();
        return e;
    }
    e.level = 2;
    e.reason = "Reproduction failed on pinned source twice".into();
    e
}
pub fn evaluate(
    reproduction: &Reproduction,
    baseline: Vec<Observation>,
    after: Vec<Observation>,
) -> Evidence {
    let mut e = evaluate_baseline(reproduction, baseline);
    e.after = after;
    if e.level != 2 {
        return e;
    }
    if e.after.len() != 2 {
        e.reason = "Reproduced, but two verification runs are required".into();
        return e;
    }
    if e.after.iter().any(|o| o.truncated) {
        e.reason = "Reproduced, but verification output was truncated".into();
        return e;
    }
    if e.after.iter().all(|o| {
        o.exit_code == Some(0)
            && o.output.contains("test result: ok.")
            && failing_names(o).is_empty()
    }) {
        e.level = 3;
        e.reason = "Reproduction failed before twice; the Cargo suite passed after twice".into();
    } else {
        e.reason = "Reproduced; patch verification did not pass the full suite twice".into();
    }
    e
}
