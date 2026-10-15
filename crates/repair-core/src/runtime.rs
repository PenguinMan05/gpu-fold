use crate::{process, types::Settings};
use anyhow::{ensure, Context, Result};
use std::{
    io::Read,
    path::{Path, PathBuf},
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
        let endpoint = crate::model::local_url(&settings.endpoint)?;
        let port = endpoint
            .port()
            .context("Specify the runtime port in the endpoint")?;
        ensure!(
            std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_millis(250)
            ).is_err(),
            "Another model server already uses this port. Choose Connect to a model app, or change the port in Advanced settings."
        );
        std::fs::create_dir_all(workspace)?;
        let mut command = launch_command(settings)?;
        command
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
        if settings.runtime_kind == "bundled" {
            let engine = bundled_engine_path()?;
            let directory = engine.parent().context("Bundled engine directory is missing")?;
            command.env("LD_LIBRARY_PATH", directory).env("GGML_BACKEND_PATH", directory);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .context("Could not start the local model engine")?;
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
/// Build arguments without a shell. Selecting a GGUF must use mistralrs -f, not -m.
pub fn launch_command(settings: &Settings) -> Result<Command> {
    let bundled = if settings.runtime_kind == "bundled" { Some(bundled_engine_path()?) } else { None };
    let exe = bundled.as_deref().unwrap_or_else(|| Path::new(&settings.runtime_path));
    let model = Path::new(&settings.model_path);
    ensure!(
        exe.is_absolute() && exe.is_file(),
        "Choose the model engine executable first."
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(exe.metadata()?.permissions().mode() & 0o111 != 0, "The selected engine is not executable. Select the installed engine binary, not an archive or installer.");
    }
    ensure!(
        model.is_absolute() && model.exists(),
        "Choose an existing model folder or GGUF file first."
    );
    let endpoint = crate::model::local_url(&settings.endpoint)?;
    ensure!(
        endpoint.host_str() == Some("127.0.0.1"),
        "Managed models require the address 127.0.0.1."
    );
    let port = endpoint
        .port()
        .context("Set a port for the model engine.")?
        .to_string();
    let context = (settings.context_tokens + settings.max_output_tokens + 512).to_string();
    let mut command = Command::new(exe);
    match settings.runtime_kind.as_str() {
        "llama_cpp" | "bundled" => {
            ensure!(
                model.is_file()
                    && model
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("gguf")),
                "llama.cpp needs a GGUF model file. Use Choose GGUF file."
            );
            command
                .args(["--host", "127.0.0.1", "--port", &port, "--model"])
                .arg(model)
                .args([
                    "--ctx-size",
                    &context,
                    "--n-gpu-layers",
                    if settings.cpu { "0" } else if settings.runtime_kind == "bundled" { "auto" } else { "99" },
                ]);
            if settings.runtime_kind == "bundled" {
                validate_gguf_model(model)?;
                command.args(["--alias", "GPUFold-local", "--no-webui", "--parallel", "1", "--jinja"]);
                if settings.cpu { command.args(["--device", "none"]); }
            }
        }
        "mistralrs" => {
            command
                .args([
                    "serve",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    &port,
                    if model.is_file() { "-f" } else { "-m" },
                ])
                .arg(model)
                .args(["--max-seq-len", &context]);
            if settings.cpu {
                command.arg("--cpu");
            }
        }
        _ => anyhow::bail!("Choose a supported model engine in Setup."),
    }
    Ok(command)
}
/// Resolve only app-owned engine locations. No executable path is required from the user.
pub fn bundled_engine_path() -> Result<PathBuf> {
    let installed = PathBuf::from("/usr/lib/gpufold/engine/llama-server");
    if installed.is_file() { return Ok(installed); }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            let adjacent = parent.join("engine/llama-server");
            if adjacent.is_file() { return Ok(adjacent); }
        }
    }
    let development = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/engine/llama-server");
    if development.is_file() { return Ok(development.canonicalize()?); }
    anyhow::bail!("GPUFold's included model engine is missing. Reinstall the complete GPUFold package.")
}

pub fn validate_gguf_model(model: &Path) -> Result<()> {
    ensure!(model.is_absolute() && model.is_file(), "Choose an existing local GGUF model file.");
    ensure!(model.extension().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case("gguf")), "Choose a .gguf model file.");
    let mut magic = [0u8; 4];
    std::fs::File::open(model)?.read_exact(&mut magic).context("The model file is empty or incomplete")?;
    ensure!(&magic == b"GGUF", "This is not a GGUF model file. Select the model weights, not a download page or an archive.");
    Ok(())
}

/// Choose an unused loopback port and configure the bundled engine from a model file alone.
pub fn local_gguf_settings(settings: &Settings) -> Result<Settings> {
    validate_gguf_model(Path::new(&settings.model_path))?;
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
    let mut settings = settings.clone();
    settings.runtime_kind = "bundled".into();
    settings.runtime_path.clear();
    settings.model_service = "file".into();
    settings.model_id = "GPUFold-local".into();
    settings.endpoint = format!("http://127.0.0.1:{port}/v1/chat/completions");
    Ok(settings)
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

pub fn memory_estimate(model: &Path, context: usize) -> Result<String> {
    ensure!(model.exists(), "Select a local model first");
    let mut weights = 0u64;
    if model.is_file() {
        weights = model.metadata()?.len();
    } else {
        for entry in walkdir::WalkDir::new(model).max_depth(2) {
            let entry = entry?;
            if entry.file_type().is_file()
                && entry
                    .path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| matches!(s, "safetensors" | "gguf" | "uqff"))
            {
                weights = weights.saturating_add(entry.metadata()?.len());
            }
        }
    }
    ensure!(
        weights > 0,
        "No supported weight files in the selected model folder"
    );
    let config = model.join("config.json");
    let kv = if config.is_file() && config.metadata()?.len() < 1_000_000 {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(config)?)?;
        let get = |key: &str| v[key].as_u64().or_else(|| v["text_config"][key].as_u64());
        match (
            get("num_hidden_layers"),
            get("num_key_value_heads"),
            get("hidden_size"),
            get("num_attention_heads"),
        ) {
            (Some(layers), Some(kv_heads), Some(hidden), Some(heads)) if heads > 0 => Some(
                2u64.saturating_mul(layers)
                    .saturating_mul(kv_heads)
                    .saturating_mul(hidden / heads)
                    .saturating_mul(context as u64)
                    .saturating_mul(2),
            ),
            _ => None,
        }
    } else {
        None
    };
    let gib = 1024f64 * 1024f64 * 1024f64;
    Ok(match kv {Some(kv)=>format!("Weights {:.2} GiB + estimated FP16 KV cache {:.2} GiB at {context} tokens + 1.5 GiB runtime reserve = {:.2} GiB. Compare with free VRAM; runtime/quantization can change this estimate.",weights as f64/gib,kv as f64/gib,(weights+kv)as f64/gib+1.5),None=>format!("Local weights: {:.2} GiB. KV-cache metadata unavailable; do not assume this fits VRAM. Use runtime diagnostics or CPU mode.",weights as f64/gib)})
}

#[cfg(test)]
mod launch_tests {
    use super::*;

    #[test]
    fn local_gguf_launch_uses_file_flags_and_preserves_paths_as_single_arguments() {
        let root = std::env::temp_dir().join(format!(
            "gpufold-launch-{}-{}",
            std::process::id(),
            crate::types::now()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let model = root.join("my coder; model.gguf");
        std::fs::write(&model, b"GGUF").unwrap();
        let mut settings = Settings::default();
        settings.runtime_kind = "mistralrs".into();
        settings.runtime_path = "/bin/true".into();
        settings.model_path = model.display().to_string();
        let args = |s: &Settings| {
            launch_command(s)
                .unwrap()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        let mistral = args(&settings);
        let file_index = mistral.iter().position(|s| s == "-f").unwrap();
        assert_eq!(mistral[file_index + 1], settings.model_path);
        assert!(!mistral.contains(&"-m".to_owned()));
        settings.runtime_kind = "llama_cpp".into();
        settings.cpu = true;
        let llama = args(&settings);
        assert!(llama.windows(2).any(|w| w == ["--n-gpu-layers", "0"]));
        assert!(llama
            .windows(2)
            .any(|w| w[0] == "--model" && w[1] == settings.model_path));
        settings.cpu = false;
        assert!(args(&settings)
            .windows(2)
            .any(|w| w == ["--n-gpu-layers", "99"]));
        settings.runtime_kind = "unknown".into();
        assert!(launch_command(&settings).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
