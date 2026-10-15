use super::*;
use std::time::{Duration, Instant};

pub(super) const GREEN: egui::Color32 = egui::Color32::from_rgb(24, 112, 82);
pub(super) const AMBER: egui::Color32 = egui::Color32::from_rgb(155, 83, 22);
const BLUE: egui::Color32 = egui::Color32::from_rgb(48, 81, 148);

pub(super) fn card(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .fill(egui::Color32::WHITE)
        .inner_margin(egui::Margin::same(18.0))
        .rounding(10.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            content(ui);
        });
}

pub(super) fn friendly_error(detail: &str) -> &str {
    let lower = detail.to_lowercase();
    if lower.contains("connect") || lower.contains("model") {
        "GPUFold could not use your model. Open Setup, start or load the model, and check the connection before trying again."
    } else if lower.contains("podman") || lower.contains("sandbox") || lower.contains("image") {
        "The test environment is not ready. Open Setup → Test environment and prepare it, then retry this step."
    } else if lower.contains("cargo") || lower.contains("rust source") {
        "This version supports Rust / Cargo repositories with a committed Cargo.lock. Choose a supported repository."
    } else if lower.contains("403") || lower.contains("rate limit") {
        "GitHub temporarily refused the request. Wait for its request limit to reset, then retry."
    } else {
        "This step could not finish. Your saved work is in History. Read the details below, correct the problem, then retry."
    }
}

pub(super) fn review_guidance(state: &State) -> &'static str {
    match state {
        State::AwaitPlan => "Next: read the explanation and planned files, then approve the plan to review the changes.",
        State::AwaitPatch => "Next: review each changed file, tick its Reviewed box, then apply the repair to the local copy. Preparing the test environment lets you first reproduce the bug.",
        State::Patched => "Next: approve the isolated test run below to check whether this repair works.",
        State::Review => "Inspect the test results below. A verified repair can be shared as a draft pull request after your final approval.",
        State::NoFix => "The model could not propose a supported repair. Read its findings or choose another issue.",
        State::AwaitTools => "Next: review the requested optional tool and its exact arguments below.",
        State::Published => "Your draft pull request is ready. Open its GitHub link below to continue the review.",
        State::Cancelled => "This run was stopped. Its saved information is available below.",
        _ => "Follow the next available action below. All code changes stay in your local working copy until publishing is approved.",
    }
}

impl App {
    pub(super) fn model_ready(&self) -> bool {
        self.connection_key.as_deref() == Some(runtime_approval_key(&self.settings).as_str())
            && self.connected_models.contains(&self.settings.model_id)
    }

    fn normalized_settings(&self) -> anyhow::Result<Settings> {
        let mut settings = self.settings.clone();
        let endpoint = settings.endpoint.trim().replace("localhost", "127.0.0.1");
        let mut url = repair_core::model::local_url(&endpoint)?;
        match url.path().trim_end_matches('/') {
            "" | "/" | "/v1" => url.set_path("/v1/chat/completions"),
            "/v1/chat/completions" => {},
            _ => anyhow::bail!("Use the local server base URL, such as http://127.0.0.1:1234, or its /v1/chat/completions address."),
        }
        settings.endpoint = url.to_string();
        Ok(settings)
    }

    pub(super) fn check_model(&mut self) {
        self.connection_key = None;
        self.connection_error = None;
        let settings = match self.normalized_settings() {
            Ok(s) => s,
            Err(error) => {
                self.connection_error = Some(format!("{error:#}"));
                return;
            }
        };
        self.job(move |engine, cancel| {
            process::check(&cancel)?;
            match repair_core::model::available_models(&settings) {
                Ok(models) => {
                    let mut settings = settings;
                    if !models.contains(&settings.model_id) {
                        settings.model_id = models[0].clone();
                    }
                    engine.store.save_settings(&settings)?;
                    Ok(JobResult::ModelStatus(settings, models))
                }
                Err(error) => Ok(JobResult::ConnectionFailed(format!("{error:#}"))),
            }
        });
        self.message = "Checking your local model app…".into();
    }

    fn start_model(&mut self) {
        let settings = match repair_core::runtime::local_gguf_settings(&self.settings) {
            Ok(settings) => settings,
            Err(error) => {
                self.connection_error = Some(format!("{error:#}"));
                return;
            }
        };
        self.connection_key = None;
        self.connection_error = None;
        self.progress_job(move |engine, cancel, progress| {
            let mut started = false;
            let result = (|| -> anyhow::Result<JobResult> {
                engine.store.save_settings(&settings)?;
                engine.runtime.lock().map_err(|_| anyhow::anyhow!("Model engine state is unavailable"))?
                    .load(&settings, &engine.store.root.join("model-runtime"), true)?;
                started = true;
                let start = Instant::now();
                loop {
                    process::check(&cancel)?;
                    if !engine.runtime.lock().map_err(|_| anyhow::anyhow!("Model engine state is unavailable"))?.running() {
                        let log = engine.runtime.lock().ok().and_then(|r| r.log.lock().ok().map(|l| l.clone())).unwrap_or_default();
                        anyhow::bail!("The model engine stopped while loading. Check that the engine matches your GPU and the model fits memory.\n{log}");
                    }
                    if let Ok(models) = repair_core::model::available_models(&settings) {
                        let mut settings = settings.clone();
                        if !models.contains(&settings.model_id) { settings.model_id = models[0].clone(); }
                        engine.store.save_settings(&settings)?;
                        return Ok(JobResult::ModelStatus(settings, models));
                    }
                    anyhow::ensure!(start.elapsed() < Duration::from_secs(180), "The model did not become ready within 3 minutes. Try a smaller model or CPU mode, or connect to an already running model app.");
                    let _ = progress.send(format!("Loading your model… {} seconds. Large models can take a few minutes.", start.elapsed().as_secs()));
                    std::thread::sleep(Duration::from_millis(300));
                }
            })();
            match result {
                Ok(ready) => Ok(ready),
                Err(error) => {
                    if started { if let Ok(mut runtime) = engine.runtime.lock() { runtime.unload(); } }
                    Ok(JobResult::ConnectionFailed(format!("{error:#}")))
                }
            }
        });
    }

    fn check_test_environment(&mut self) {
        let image = self.settings.image.clone();
        self.job(move |_, cancel| {
            let result = repair_core::sandbox::check_runtime(&cancel)
                .and_then(|_| repair_core::sandbox::image_id(&image, &cancel));
            Ok(match result {
                Ok(_) => JobResult::SandboxStatus(
                    true,
                    "Test environment ready. You can verify reviewed repairs.".into(),
                ),
                Err(error) => JobResult::SandboxStatus(
                    false,
                    format!("Test environment needs setup: {error:#}"),
                ),
            })
        });
    }

    pub(super) fn navigation(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navigation")
            .exact_width(174.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(16.0);
                ui.small("WORKSPACE");
                ui.add_space(8.0);
                for (label, page) in [
                    ("Dashboard", Page::Home),
                    ("Issues", Page::Task),
                    ("Results", Page::Queue),
                    ("Review repair", Page::Review),
                    ("History", Page::History),
                ] {
                    if ui
                        .add_sized(
                            [150.0, 38.0],
                            egui::SelectableLabel::new(self.page == page, label),
                        )
                        .clicked()
                    {
                        self.page = page;
                    }
                }
                ui.add_space(20.0);
                ui.small("CONFIGURATION");
                if ui
                    .add_sized(
                        [150.0, 38.0],
                        egui::SelectableLabel::new(self.page == Page::Settings, "Setup & settings"),
                    )
                    .clicked()
                {
                    self.page = Page::Settings;
                }
                ui.collapsing("Advanced tools", |ui| {
                    if ui.button("MCP tools").clicked() {
                        self.page = Page::Mcp;
                    }
                });
                ui.add_space(24.0);
                ui.separator();
                ui.small("Rust / Cargo projects");
                ui.small("Code stays local until you approve a draft PR.");

            });
    }

    pub(super) fn model_banner(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            if self.model_ready() {
                ui.colored_label(GREEN, format!("Connected · {}", self.settings.model_id));
            } else {
                ui.colored_label(AMBER, "Connect a model before starting issues");
                ui.label("You can browse issues now. Setup helps you start a local model and checks it is ready.");
                if ui.button("Open setup").clicked() {
                    self.setup_step = 0;
                    self.page = Page::Settings;
                }
            }
        });
        ui.add_space(12.0);
    }

    pub(super) fn home_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Put your local AI to work");
        ui.label("Choose GitHub issues, generate repair proposals, and review the results.");
        ui.add_space(18.0);
        self.model_banner(ui);
        card(ui, |ui| {
            ui.heading(if self.model_ready() {
                "Start a new batch"
            } else {
                "Let's get your model ready"
            });
            ui.label(if self.model_ready() { "Paste a public Rust repository URL in Issues, scan for open issues, then select the ones you want GPUFold to work on." } else { "Choose your local GGUF model in Setup and click Load model. GPUFold includes the engine and starts it for you." });
            ui.add_space(8.0);
            let ready = self.model_ready();
            if ui
                .add_sized(
                    [240.0, 42.0],
                    egui::Button::new(
                        egui::RichText::new(if ready {
                            "Choose issues to work on"
                        } else {
                            "Continue setup"
                        })
                        .color(egui::Color32::WHITE),
                    )
                    .fill(BLUE),
                )
                .clicked()
            {
                self.page = if ready { Page::Task } else { Page::Settings };
                self.setup_step = 0;
            }
        });
        ui.add_space(16.0);
        card(ui, |ui| {
            ui.strong("How a repair moves through GPUFold");
            ui.label("1. Choose issues     |     2. Generate proposals     |     3. Review changes     |     4. Test and share");
            ui.small("Each issue gets its own saved result. You approve code changes and any draft pull request.");
        });
        ui.add_space(18.0);
        ui.heading("Recent work");
        match self.engine.store.list() {
            Ok(runs) if !runs.is_empty() => {
                for r in runs.into_iter().take(5) {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .button(format!(
                                "Open {}/{}{}",
                                r.owner,
                                r.repo,
                                r.issue_number.map(|n| format!(" #{n}")).unwrap_or_default()
                            ))
                            .clicked()
                        {
                            self.run = Some(r.clone());
                            self.reviewed.clear();
                            self.confirm_publish = false;
                            self.page = Page::Review;
                        }
                        ui.label(format!("{:?}", r.state));
                    });
                }
            }
            _ => {
                ui.label("No saved work yet. Your first batch will appear here.");
            }
        }
        ui.add_space(16.0);
        ui.collapsing("Try a sample repair without a model", |ui| {
            ui.label("This demonstration uses a supplied fix. It does not test model quality or publish anything.");
            if ui.button("Open sample repair").clicked() {
                self.job(|e, _| { let mut run = e.fixture()?; let p: Proposal = serde_json::from_str(include_str!("../../../fixtures/fix-add.json"))?; e.replace_proposal(&mut run, p)?; Ok(JobResult::Run(run)) });
            }
        });
    }

    pub(super) fn settings_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Set up GPUFold");
        ui.label("Choose your local GGUF file, load it, then select a repository and issues.");
        ui.small("Your choices are saved automatically.");
        ui.add_space(14.0);
        ui.horizontal_wrapped(|ui| {
            for (step, label) in [
                (0, "1  Load model"),
                (1, "2  Test environment"),
                (2, "3  Ready to start"),
            ] {
                ui.selectable_value(&mut self.setup_step, step, label);
            }
        });
        ui.add_space(14.0);
        match self.setup_step {
            0 => self.model_setup(ui),
            1 => {
                card(ui, |ui| {
                    ui.heading("Prepare a safe place to test repairs");
                    ui.label("GPUFold uses Podman to run project tests in an isolated container. You can generate proposals before preparing this; you will need it when testing changes.");
                    ui.add_space(10.0);
                    ui.colored_label(
                        if self.sandbox_ready { GREEN } else { AMBER },
                        &self.sandbox_status,
                    );
                    if ui.button("Check test environment").clicked() {
                        self.check_test_environment();
                    }
                    ui.separator();
                    self.build_sandbox_button(ui);
                    ui.small("Initial setup needs internet access to download the Rust base image. It can take several minutes.");
                    ui.collapsing("If Podman cannot start", |ui| {
                        ui.label("Run GPUFold as your normal desktop user. Your Linux account needs rootless Podman support. If you just installed Podman, log out and back in before trying again.");
                        ui.hyperlink_to("Podman rootless setup guide", "https://github.com/containers/podman/blob/main/docs/tutorials/rootless_tutorial.md");
                    });
                });
                ui.add_space(12.0);
                if ui.button("Continue to finish setup").clicked() {
                    self.setup_step = 2;
                }
                if !self.sandbox_ready {
                    ui.small("You can return here to prepare testing later.");
                }
            }
            _ => {
                card(ui, |ui| {
                    ui.heading("You're almost ready");
                    if self.model_ready() {
                        ui.colored_label(
                            GREEN,
                            format!("Model connected: {}", self.settings.model_id),
                        );
                    } else {
                        ui.colored_label(AMBER, "Connect your model to finish setup.");
                        if ui.button("Back to model setup").clicked() {
                            self.setup_step = 0;
                        }
                    }
                    ui.label(if self.sandbox_ready {
                        "Test environment: ready"
                    } else {
                        "Test environment: prepare later, before running project tests"
                    });
                    ui.label("GitHub sign-in: not needed to read public issues. Sign in later if you want to publish a draft PR.");
                    ui.add_space(12.0);
                    ui.label("Repository URL (optional for now)");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.url)
                            .hint_text("https://github.com/owner/rust-project")
                            .desired_width(f32::INFINITY),
                    );
                    ui.small("Current support: Rust / Cargo projects with a committed Cargo.lock.");
                    if ui
                        .add_enabled(
                            self.model_ready(),
                            egui::Button::new("Finish setup and choose issues"),
                        )
                        .clicked()
                    {
                        self.settings.setup_complete = true;
                        self.settings.last_repository = self.url.clone();
                        match self.engine.store.save_settings(&self.settings) {
                            Ok(()) => {
                                self.page = Page::Task;
                                self.message =
                                    "Setup saved. Find issues, select a few, then click Start."
                                        .into();
                            }
                            Err(error) => {
                                self.message_error = true;
                                self.message = format!("Could not save setup: {error}");
                            }
                        }
                    }
                });
            }
        }
        ui.add_space(18.0);
        ui.collapsing("Advanced settings & GitHub sign-in", |ui| {
            self.advanced_settings_page(ui);
        });
        ui.add_space(12.0);
        if ui.button("Explore the dashboard").clicked() {
            self.page = Page::Home;
        }
    }

    fn model_setup(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.heading("How would you like to run your model?");
            ui.label("For a GGUF model, choose the file and click Load model. The model engine is included with GPUFold.");
            let before = self.settings.model_service.clone();
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.settings.model_service, "file".into(), "Local GGUF file");
                ui.selectable_value(&mut self.settings.model_service, "lm_studio".into(), "LM Studio");
                ui.selectable_value(&mut self.settings.model_service, "ollama".into(), "Ollama");
                ui.selectable_value(&mut self.settings.model_service, "other".into(), "Other local server");
            });
            if before != self.settings.model_service {
                self.connection_key = None;
                self.connection_error = None;

                self.settings.endpoint = match self.settings.model_service.as_str() {
                    "ollama" => "http://127.0.0.1:11434/v1/chat/completions",
                    "file" => "http://127.0.0.1:8080/v1/chat/completions",
                    _ => "http://127.0.0.1:1234/v1/chat/completions",
                }
                .into();
                if self.settings.model_service == "file" {
                    self.settings.runtime_kind = "bundled".into();
                    self.settings.runtime_path.clear();
                }
            }
            ui.add_space(12.0);
            match self.settings.model_service.as_str() {
                "lm_studio" => {
                    ui.strong("In LM Studio");
                    ui.label("1. Download or select a coding model and load it.\n2. Open Developer / Local Server and start the server.\n3. Leave LM Studio open, then click Check connection below.");
                    ui.hyperlink_to(
                        "LM Studio local server guide",
                        "https://lmstudio.ai/docs/developer/core/server",
                    );
                }
                "ollama" => {
                    ui.strong("In Ollama");
                    ui.label("1. Open Ollama and install a coding model.\n2. Keep the Ollama service running.\n3. Click Check connection, then choose the installed model below.");
                    ui.small("An installed Ollama model may load on the first request. Choose a model that supports coding and JSON output.");
                    ui.hyperlink_to("Ollama documentation", "https://docs.ollama.com/openai");
                }
                "file" => {
                    self.model_file_fields(ui);
                }
                _ => {
                    ui.label("Start an OpenAI-compatible model server on this computer and load your coding model. Enter its local address below.");
                }
            }
            ui.add_space(10.0);
            if self.settings.model_service == "file" {
                let running = self.engine.runtime.lock().map(|mut r| r.running()).unwrap_or(false);
                let has_model = std::path::Path::new(&self.settings.model_path).is_file();
                if ui.add_enabled(has_model && !running, egui::Button::new("Load model").min_size(egui::vec2(180.0, 40.0))).clicked() {
                    self.start_model();
                }
                if running { ui.label("Your model is running. Stop it below before choosing another model."); }
                ui.small("GPUFold starts its included engine and checks the connection automatically. Large models may take a few minutes to load.");
            } else {
                ui.label("Local server address");
                ui.add(egui::TextEdit::singleline(&mut self.settings.endpoint).desired_width(f32::INFINITY).hint_text("http://127.0.0.1:1234"));
                ui.small("Base URLs and localhost are accepted. The server must run on this computer.");
            }
            ui.horizontal_wrapped(|ui| {
                if self.settings.model_service != "file" && ui.button("Check connection").clicked() {
                    self.check_model();
                }
                let running = self
                    .engine
                    .runtime
                    .lock()
                    .map(|mut r| r.running())
                    .unwrap_or(false);
                if running && ui.button("Stop model engine").clicked() {
                    if let Ok(mut runtime) = self.engine.runtime.lock() {
                        runtime.unload();
                    }
                    self.connection_key = None;
                    self.message =
                        "Model engine stopped. You can choose another model and start it again."
                            .into();
                }
            });

            if self.model_ready() {
                if self.settings.model_service == "file" { ui.colored_label(GREEN, "Model loaded. Continue to the next step when ready."); }
                else { ui.colored_label(GREEN, "Connected. Choose the model you want to use:");
                let old = self.settings.model_id.clone();
                egui::ComboBox::from_id_salt("connected_model")
                    .selected_text(&self.settings.model_id)
                    .width(300.0)
                    .show_ui(ui, |ui| {
                        for model in &self.connected_models {
                            ui.selectable_value(&mut self.settings.model_id, model.clone(), model);
                        }
                    });
                if old != self.settings.model_id {
                    self.connection_key = Some(runtime_approval_key(&self.settings));
                    let _ = self.engine.store.save_settings(&self.settings);
                }
            }
            }
            if let Some(error) = &self.connection_error {
                ui.add_space(10.0);
                ui.colored_label(AMBER, "Connection needs attention");
                if self.settings.model_service == "file" {
                    ui.label("Check that the GGUF file is complete and supported by the included engine. If GPU loading fails, try CPU mode under Model options.");
                } else { ui.label(if error.contains("no model") { "The app is running, but no model is loaded. Load a coding model in your model app and check again." } else { "Make sure the model app is open, a model is loaded, and its local server is started. Check that the port above matches the app." }); }
                ui.collapsing("Connection details", |ui| {
                    ui.add(egui::Label::new(error).wrap().selectable(true));
                });
            }
        });
        ui.add_space(12.0);
        if ui
            .add_enabled(
                self.model_ready(),
                egui::Button::new("Continue to test environment"),
            )
            .clicked()
        {
            self.setup_step = 1;
        }
    }

    fn model_file_fields(&mut self, ui: &mut egui::Ui) {
        ui.strong("Choose your local GGUF model");
        ui.label("The inference engine is included. You do not need to install an engine, start a server, or enter a port.");
        ui.label("Model file");
        ui.add(egui::TextEdit::singleline(&mut self.settings.model_path).desired_width(f32::INFINITY).hint_text("Choose your .gguf model file"));
        if ui.button("Choose GGUF file").clicked() {
            if let Some(path) = rfd::FileDialog::new().add_filter("GGUF model", &["gguf"]).pick_file() {
                self.settings.model_path = path.display().to_string();
                self.settings.runtime_kind = "bundled".into();
                self.settings.runtime_path.clear();
                self.connection_key = None;
                self.connection_error = None;
            }
        }
        ui.small("GPU acceleration uses the included Vulkan backend when a compatible GPU and driver are available. CPU mode is also included.");
        ui.collapsing("Model options", |ui| {
            ui.checkbox(&mut self.settings.cpu, "Use CPU only (slower; useful if GPU memory is limited)");
            if ui.button("Estimate model memory").clicked() {
                let path = PathBuf::from(&self.settings.model_path);
                let context = self.settings.context_tokens + self.settings.max_output_tokens + 512;
                self.job(move |_, _| Ok(JobResult::Message(repair_core::runtime::memory_estimate(&path, context)?)));
            }
        });
        if let Ok(runtime) = self.engine.runtime.lock() {
            if let Ok(log) = runtime.log.lock() { ui.collapsing("Model loading details", |ui| { ui.add(egui::Label::new(log.as_str()).wrap().selectable(true)); }); }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let root = std::env::temp_dir().join(format!(
            "gpufold-onboarding-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        App::new(Engine::new(Store::open(root).unwrap()))
    }

    #[test]
    #[ignore = "requires the included engine and CI GGUF test fixture"]
    fn gguf_setup_loads_from_a_model_path_without_an_engine_selection() {
        let mut app = app();
        app.settings.model_path = std::env::var("GPUFOLD_TEST_GGUF").unwrap();
        app.settings.cpu = true;
        app.settings.context_tokens = 128;
        app.settings.max_output_tokens = 64;
        assert!(app.settings.runtime_path.is_empty());
        app.start_model();
        let started = Instant::now();
        while app.pending.is_some() {
            app.observe();
            assert!(started.elapsed() < Duration::from_secs(45), "onboarding did not finish: {}", app.message);
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(app.model_ready(), "GGUF setup failed: {:?}", app.connection_error);
        assert!(app.settings.runtime_path.is_empty());
        assert_eq!(app.engine.store.settings().unwrap().runtime_kind, "bundled");
        app.engine.runtime.lock().unwrap().unload();
        std::fs::remove_dir_all(&app.engine.store.root).unwrap();
    }

    #[test]
    fn live_activity_and_partial_model_summary_render_at_minimum_window_size() {
        let mut app = app();
        let (tx, rx) = mpsc::channel();
        app.pending = Some(rx);
        app.engine.report_activity("Attempt 4: checking citations after a malformed JSON response");
        *app.engine.model_output.lock().unwrap() = "{\"explanation\":\"Checking the arithmetic replacement".into();
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(760.0,560.0))), ..Default::default() }, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.conversation_body(ui));
        });
        let shapes = format!("{:?}", output.shapes);
        assert!(shapes.contains("Activity") && shapes.contains("Attempt 4") && shapes.contains("Checking the arithmetic"));
        drop(tx);
        std::fs::remove_dir_all(&app.engine.store.root).unwrap();
    }

    #[test]
    fn conversation_is_opened_on_demand_and_controls_are_on_the_right_without_a_footer() {
        let mut app=app(); let (tx,rx)=mpsc::channel(); app.pending=Some(rx);
        app.engine.report_activity("Inspecting the stylus implementation");
        app.engine.say(repair_core::types::DialogueRole::Assistant,"The issue is in the input event translation.");
        let ctx=egui::Context::default();
        let mut tick=0u64;
        for open in [false,true] {
            app.show_conversation=open;
            let mut rendered=String::new();
            // Windows measure their first frame before painting; exercise steady state.
            for _ in 0..6 {
                tick+=1;
                let output=ctx.run(egui::RawInput {time:Some(tick as f64 * 0.1),screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1100.0,800.0))),..Default::default()},|ctx| {
                    app.task_controls(ctx);
                    egui::CentralPanel::default().show(ctx,|ui| { ui.label("Workspace"); });
                    app.conversation_window(ctx);
                });
                rendered=format!("{:?}",output.shapes);
            }
            assert!(rendered.contains("AI conversation") && rendered.contains("Stop current task"));
            assert!(!rendered.contains("Working · live activity"));
            if open { assert!(rendered.contains("input event translation")); }
            else { assert!(!rendered.contains("input event translation")); }
        }
        drop(tx); std::fs::remove_dir_all(&app.engine.store.root).unwrap();
    }

    #[test]
    fn new_install_opens_setup_and_model_readiness_is_never_assumed() {
        let mut app = app();
        assert_eq!(app.page, Page::Settings);
        assert!(!app.model_ready());
        app.connected_models = vec![app.settings.model_id.clone()];
        app.connection_key = Some(runtime_approval_key(&app.settings));
        assert!(app.model_ready());
        app.settings.endpoint = "http://127.0.0.1:11434/v1/chat/completions".into();
        assert!(
            !app.model_ready(),
            "changing servers must disable Start until checked again"
        );
        let _ = std::fs::remove_dir_all(&app.engine.store.root);
    }

    #[test]
    fn setup_accepts_common_local_base_urls_but_rejects_remote_servers() {
        let mut app = app();
        for address in [
            "http://localhost:1234",
            "http://127.0.0.1:1234/",
            "http://127.0.0.1:1234/v1",
        ] {
            app.settings.endpoint = address.into();
            assert_eq!(
                app.normalized_settings().unwrap().endpoint,
                "http://127.0.0.1:1234/v1/chat/completions"
            );
        }
        for address in [
            "https://api.example.com/v1",
            "http://127.0.0.1:1234/v1?key=secret",
            "http://localhost.evil.test:1234",
        ] {
            app.settings.endpoint = address.into();
            assert!(app.normalized_settings().is_err());
        }
        let old: Settings =
            serde_json::from_str(r#"{"endpoint":"http://127.0.0.1:1234/v1/chat/completions"}"#)
                .unwrap();
        assert!(!old.setup_complete);
        assert_eq!(old.runtime_kind, "bundled");
        let _ = std::fs::remove_dir_all(&app.engine.store.root);
    }

    #[test]
    fn setup_and_primary_pages_render_at_minimum_window_size() {
        let mut app = app();
        let ctx = egui::Context::default();
        for page in [
            Page::Settings,
            Page::Home,
            Page::Task,
            Page::Queue,
            Page::Review,
            Page::History,
        ] {
            app.page = page;
            for step in 0..3 {
                app.setup_step = step;
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(760.0, 560.0),
                    )),
                    ..Default::default()
                };
                let output = ctx.run(input, |ctx| {
                    app.navigation(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| match app.page {
                            Page::Settings => app.settings_page(ui),
                            Page::Home => app.home_page(ui),
                            Page::Task => app.task_page(ui),
                            Page::Queue => app.queue_page(ui),
                            Page::Review => app.review_page(ui),
                            Page::History => app.history_page(ui),
                            _ => {}
                        });
                    });
                });
                assert!(!output.shapes.is_empty());
            }
        }
        let _ = std::fs::remove_dir_all(&app.engine.store.root);
    }
}
