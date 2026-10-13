use crate::types::Observation;
use anyhow::{bail, Context, Result};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc,atomic::{AtomicBool,Ordering}};
use std::time::{Duration,Instant};

#[cfg(unix)]
extern "C" {fn kill(pid:i32,signal:i32)->i32;}

pub type Cancel=Arc<AtomicBool>;
pub fn cancel_token()->Cancel{Arc::new(AtomicBool::new(false))}
pub fn check(cancel:&Cancel)->Result<()>{if cancel.load(Ordering::SeqCst){bail!("Cancelled by user")}Ok(())}
fn secret_pattern()->&'static regex::Regex{static RE:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();RE.get_or_init(||regex::Regex::new(r"(?i)(github_pat_[A-Za-z0-9_]+|gh[pousr]_[A-Za-z0-9_]+|sk-[A-Za-z0-9_-]{16,}|AKIA[A-Z0-9]{16})").expect("Static redaction expression"))}
pub fn has_secret(s:&str)->bool{secret_pattern().is_match(s)||s.contains("-----BEGIN PRIVATE KEY")}
pub fn scrub(s:&str)->String{let s:String=s.chars().filter(|c|(!c.is_control()||matches!(*c,'\n'|'\t'))&&!matches!(*c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')).collect();secret_pattern().replace_all(&s,"[REDACTED]").into_owned()}
pub fn run(mut cmd:Command,display:Vec<String>,timeout:Duration,cancel:&Cancel)->Result<Observation>{
    check(cancel)?;let started=Instant::now();cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)] {use std::os::unix::process::CommandExt;cmd.process_group(0);}
    let mut child=cmd.spawn().context("Could not start process")?;
    let stdout=child.stdout.take().context("Missing stdout")?;let stderr=child.stderr.take().context("Missing stderr")?;
    fn reader<R:Read+Send+'static>(mut r:R)->std::thread::JoinHandle<(Vec<u8>,bool)>{std::thread::spawn(move||{let mut out=Vec::new();let mut buf=[0;8192];let mut cut=false;while let Ok(n)=r.read(&mut buf){if n==0{break}let room=(1_000_000usize).saturating_sub(out.len());out.extend_from_slice(&buf[..n.min(room)]);if n>room{cut=true;}}(out,cut)})}
    let a=reader(stdout);let b=reader(stderr);
    loop{
        if cancel.load(Ordering::SeqCst)||started.elapsed()>timeout{#[cfg(unix)] unsafe {kill(-(child.id() as i32),9);}
            let _=child.kill();let _=child.wait();bail!("Process cancelled or exceeded {} second limit",timeout.as_secs());}
        if let Some(status)=child.try_wait()?{let (o,ot)=a.join().map_err(|_|anyhow::anyhow!("Output thread panicked"))?;let(e,et)=b.join().map_err(|_|anyhow::anyhow!("Output thread panicked"))?;
            return Ok(Observation{command:display,exit_code:status.code(),output:scrub(&format!("{}\n{}",String::from_utf8_lossy(&o),String::from_utf8_lossy(&e))),elapsed_ms:started.elapsed().as_millis(),truncated:ot||et});}
        std::thread::sleep(Duration::from_millis(50));
    }
}
pub fn success(o:Observation)->Result<Observation>{if o.exit_code!=Some(0){bail!("Command failed: {:?}\n{}",o.command,o.output)}Ok(o)}
pub fn git()->Command{let mut c=Command::new("git");c.env_clear().env("PATH","/usr/local/bin:/usr/bin:/bin").env("GIT_CONFIG_NOSYSTEM","1").env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_SYSTEM","/dev/null").env("GIT_TERMINAL_PROMPT","0").env("GIT_LFS_SKIP_SMUDGE","1");c.args(["-c","core.hooksPath=/dev/null","-c","core.autocrlf=false","-c","protocol.file.allow=never","-c","credential.helper=","-c","core.fsmonitor=false","-c","diff.external="]);c}
