use crate::{github,process,repository,types::*,validation};
use anyhow::{bail,ensure,Context,Result};
use flate2::read::GzDecoder;
use serde_json::json;
use std::{collections::{BTreeMap,BTreeSet},fs,io::Read,path::Path,process::Command,time::Duration};

pub trait SandboxRunner {fn test(&self,source:&Path,vendor:&Path,image:&str,cancel:&process::Cancel)->Result<Observation>;}
pub struct PodmanRunner;
fn podman()->Command{let mut c=Command::new("podman");c.env_clear().env("PATH","/usr/local/bin:/usr/bin:/bin");for n in ["HOME","XDG_RUNTIME_DIR","XDG_CONFIG_HOME","XDG_DATA_HOME"]{if let Some(v)=std::env::var_os(n){c.env(n,v);}}c}
pub fn check_runtime(cancel:&process::Cancel)->Result<()> {let mut c=podman();c.args(["info","--format","{{.Host.Security.Rootless}}"]);let o=process::success(process::run(c,vec!["podman info rootless".into()],Duration::from_secs(20),cancel)?)?;ensure!(o.output.trim()=="true","A working rootless Podman installation is required. No unsafe host test fallback.");Ok(())}
pub fn image_id(name:&str,cancel:&process::Cancel)->Result<String>{ensure!(!name.is_empty()&&!name.starts_with('-')&&!name.chars().any(char::is_whitespace),"Invalid image name");let mut c=podman();c.args(["image","inspect","--format","{{.Id}}",name]);let o=process::success(process::run(c,vec!["podman inspect sandbox image".into()],Duration::from_secs(20),cancel)?)?;let id=o.output.trim();ensure!(id.starts_with("sha256:")&&id.len()==71,"Invalid local image digest");Ok(id.into())}
pub fn fetch_vendor(source:&Path,vendor:&Path,cancel:&process::Cancel)->Result<()> {
    ensure!(fs::metadata(source.join("Cargo.lock")).context("A committed Cargo.lock is required")?.len()<=2_000_000,"Lockfile exceeds size cap");let lock=fs::read_to_string(source.join("Cargo.lock")).context("A committed Cargo.lock is required for verified offline execution")?;let data:toml::Value=toml::from_str(&lock)?;let packages=data.get("package").and_then(toml::Value::as_array).context("Malformed lockfile")?;ensure!(packages.len()<=1000,"Dependency count over cap");
    if vendor.exists(){fs::remove_dir_all(vendor)?;}fs::create_dir_all(vendor)?;let mut total=0u64;
    for p in packages{process::check(cancel)?;let Some(source)=p.get("source").and_then(toml::Value::as_str) else {continue};ensure!(source=="registry+https://github.com/rust-lang/crates.io-index","Only crates.io registry dependencies are supported; git/custom registries are blocked");let name=p.get("name").and_then(toml::Value::as_str).context("Dependency name")?;let version=p.get("version").and_then(toml::Value::as_str).context("Dependency version")?;ensure!(name.chars().all(|c|c.is_ascii_alphanumeric()||matches!(c,'_'|'-'))&&version.chars().all(|c|c.is_ascii_alphanumeric()||matches!(c,'.'|'-'|'+')),"Unsafe dependency name/version");let checksum=p.get("checksum").and_then(toml::Value::as_str).context("Dependency missing SHA256")?;ensure!(checksum.len()==64&&checksum.chars().all(|c|c.is_ascii_hexdigit()),"Invalid package checksum");
        // Fixed destination; redirects disabled. Repository configuration never chooses a host.
        let url=format!("https://static.crates.io/crates/{name}/{name}-{version}.crate");let mut r=github::client()?.get(url).send()?;ensure!(r.status().is_success(),"Registry download failed for {name}-{version}");let mut bytes=vec![];r.by_ref().take(32_000_001).read_to_end(&mut bytes)?;ensure!(bytes.len()<=32_000_000,"Crate exceeds download cap");total+=bytes.len() as u64;ensure!(total<=300_000_000,"Dependency download budget exceeded");ensure!(validation::digest(&bytes)==checksum,"Lockfile checksum mismatch for {name}");let dest=vendor.join(format!("{name}-{version}"));repository::unpack(GzDecoder::new(&bytes[..]),&dest,true,64_000_000,10000)?;
        let mut hashes=BTreeMap::new();for e in walkdir::WalkDir::new(&dest){let e=e?;if e.file_type().is_file(){let rel=e.path().strip_prefix(&dest)?.to_string_lossy().replace('\\',"/");if rel!=".cargo-checksum.json"{hashes.insert(rel,validation::digest(&fs::read(e.path())?));}}}
        fs::write(dest.join(".cargo-checksum.json"),serde_json::to_vec(&json!({"files":hashes,"package":checksum}))?)?;
        let expanded=walkdir::WalkDir::new(vendor).into_iter().filter_map(|e|e.ok()).filter(|e|e.file_type().is_file()).try_fold(0u64,|sum,e|->Result<u64>{Ok(sum+e.metadata()?.len())})?;ensure!(expanded<=500_000_000,"Expanded dependency budget exceeded");
    }Ok(())
}
pub fn sanitized_copy(source:&Path,dest:&Path)->Result<()> {repository::copy_tree(source,dest)?;let configs=walkdir::WalkDir::new(dest).into_iter().filter_map(|e|e.ok()).filter(|e|e.file_type().is_file()).filter(|e|{let n=e.file_name().to_string_lossy();n=="rust-toolchain"||n=="rust-toolchain.toml"||((n=="config"||n=="config.toml")&&e.path().parent().is_some_and(|p|p.file_name().is_some_and(|n|n==".cargo")))}).map(|e|e.path().to_path_buf()).collect::<Vec<_>>();for p in configs{fs::remove_file(p)?;}Ok(())}
impl SandboxRunner for PodmanRunner {
    fn test(&self,source:&Path,vendor:&Path,image:&str,cancel:&process::Cancel)->Result<Observation>{
        let name=format!("rust-repair-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos());let src=source.canonicalize()?;let vendor=vendor.canonicalize()?;ensure!(!src.to_string_lossy().contains(':')&&!vendor.to_string_lossy().contains(':'),"Container paths cannot contain colons");
        let mut c=podman();c.args(["run","--rm","--pull=never","--name",&name,"--network=none","--read-only","--user","65534:65534","--cap-drop=ALL","--security-opt=no-new-privileges","--pids-limit=256","--memory=4g","--cpus=2","--tmpfs","/tmp:rw,nosuid,nodev,size=1073741824,mode=1777","--volume"]).arg(format!("{}:/workspace:ro",src.display())).arg("--volume").arg(format!("{}:/vendor:ro",vendor.display())).args(["--workdir","/workspace","--env","CARGO_HOME=/tmp/cargo","--env","RUSTUP_HOME=/usr/local/rustup","--env","CARGO_TARGET_DIR=/tmp/target","--env","CARGO_NET_OFFLINE=true","--env","HOME=/tmp","--entrypoint","/usr/local/cargo/bin/cargo",image,"test","--offline","--locked","--config","source.crates-io.replace-with=\"vendored-sources\"","--config","source.vendored-sources.directory=\"/vendor\"","--","--test-threads=1"]);
        let result=process::run(c,vec!["cargo".into(),"test".into(),"--offline".into(),"--locked".into(),"--".into(),"--test-threads=1".into()],Duration::from_secs(600),cancel);
        if result.is_err(){let mut cleanup=podman();cleanup.args(["rm","--force",&name]);let _=process::run(cleanup,vec!["podman stop sandbox".into()],Duration::from_secs(10),&process::cancel_token());}result
    }
}
fn failing_names(o:&Observation)->BTreeSet<String>{o.output.lines().filter_map(|l|{let l=l.trim();if l.starts_with("test ")&&l.ends_with(" ... FAILED"){Some(l.trim_start_matches("test ").trim_end_matches(" ... FAILED").into())}else{None}}).collect()}
pub fn evaluate(reproduction:&Reproduction,baseline:Vec<Observation>,after:Vec<Observation>)->Evidence {
    let mut e=Evidence{level:1,reason:"Located claim; tests did not prove the fix".into(),baseline,after};
    if e.baseline.len()!=2||e.after.len()!=2{e.reason="Expected two baseline and two verification runs".into();return e}
    if e.baseline.iter().chain(&e.after).any(|o|o.truncated){e.reason="Output was truncated; evidence withheld".into();return e}
    if e.baseline.iter().any(|o|o.exit_code.is_none()||o.output.contains("could not compile")||o.output.contains("error[E")){e.reason="Baseline did not compile or was interrupted".into();return e}
    let names0=failing_names(&e.baseline[0]);let names1=failing_names(&e.baseline[1]);if names0!=names1{e.reason="Baseline failure set changed: possible flaky tests".into();return e}
    if !names0.contains(&reproduction.test_name)||e.baseline.iter().any(|o|o.exit_code==Some(0)||!o.output.contains(&reproduction.failure_marker)){e.reason="The new reproduction did not fail consistently for its asserted reason".into();return e}
    e.level=2;e.reason="Reproduction failed on pinned source twice".into();
    if e.after.iter().all(|o|o.exit_code==Some(0)&&o.output.contains("test result: ok.")&&failing_names(o).is_empty()){e.level=3;e.reason="Reproduction failed before the fix twice; the full Cargo test suite passed after the fix twice".into();}e
}
