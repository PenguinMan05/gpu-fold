use crate::{process, types::Settings};
use anyhow::{ensure, Context, Result};
use std::{
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Default)]
pub struct ManagedRuntime {
    child: Option<Child>,
    pub log: Arc<Mutex<String>>,
}
impl ManagedRuntime {
    pub fn running(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|c| c.try_wait().ok().flatten().is_none())
    }
    pub fn load(&mut self, settings: &Settings, workspace: &Path, approved: bool) -> Result<()> {
        ensure!(
            approved,
            "Approve the exact local runtime command before loading a model"
        );
        ensure!(!self.running(), "Unload the current managed model first");
        let exe = Path::new(&settings.runtime_path);
        let model = Path::new(&settings.model_path);
        ensure!(
            exe.is_absolute() && exe.is_file(),
            "Choose the installed mistralrs executable"
        );
        ensure!(
            model.is_absolute() && model.exists(),
            "Choose a complete local model directory or GGUF file"
        );
        let endpoint = crate::model::local_url(&settings.endpoint)?;
        ensure!(
            endpoint.host_str() == Some("127.0.0.1"),
            "Managed runtime requires 127.0.0.1"
        );
        let port = endpoint
            .port()
            .context("Specify the runtime port in the endpoint")?;
        std::fs::create_dir_all(workspace)?;
        let mut command = Command::new(exe);
        command
            .args([
                "serve",
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "-m",
            ])
            .arg(model)
            .arg("--max-seq-len")
            .arg((settings.context_tokens + settings.max_output_tokens + 512).to_string())
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .env("HOME", workspace)
            .env("HF_HOME", workspace.join("hf-cache"))
            .env("HF_HUB_OFFLINE", "1")
            .env("HF_HUB_DISABLE_TELEMETRY", "1")
            .current_dir(workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if settings.cpu {
            command.arg("--cpu");
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .context("Could not start the selected Rust inference runtime")?;
        if let Ok(mut log) = self.log.lock() {
            log.clear();
        }
        for stream in [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ] {
            if let Some(mut stream) = stream {
                let log = self.log.clone();
                std::thread::spawn(move || {
                    let mut bytes = [0; 4096];
                    while let Ok(n) = stream.read(&mut bytes) {
                        if n == 0 {
                            break;
                        }
                        if let Ok(mut log) = log.lock() {
                            log.push_str(&process::scrub(&String::from_utf8_lossy(&bytes[..n])));
                            if log.len() > 64000 {
                                let mut offset = log.len() - 64000;
                                while !log.is_char_boundary(offset) {
                                    offset += 1
                                }
                                log.drain(..offset);
                            }
                        }
                    }
                });
            }
        }
        self.child = Some(child);
        Ok(())
    }
    pub fn unload(&mut self) {
        if let Some(mut child) = self.child.take() {
            #[cfg(unix)]
            unsafe {
                unsafe extern "C" {
                    fn kill(pid: i32, signal: i32) -> i32;
                }
                kill(-(child.id() as i32), 9);
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Drop for ManagedRuntime {
    fn drop(&mut self) {
        self.unload();
    }
}

pub fn gpu_status(cancel: &process::Cancel) -> Result<String> {
    let mut cmd = Command::new("nvidia-smi");
    cmd.env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .args([
            "--query-gpu=name,memory.total,memory.free,utilization.gpu,temperature.gpu",
            "--format=csv,noheader",
        ]);
    Ok(process::success(process::run(
        cmd,
        vec!["nvidia-smi".into()],
        Duration::from_secs(5),
        cancel,
    )?)?
    .output)
}
