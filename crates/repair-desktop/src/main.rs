use eframe::egui;
use repair_core::{github, mcp, process, store, types::*, Engine, Store};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{atomic::Ordering, mpsc},
};

#[derive(PartialEq)]
enum Page {
    Home,
    Task,
    Review,
    Settings,
    Mcp,
    History,
}
enum JobResult {
    Run(Run),
    Message(String),
    Tools(Vec<mcp::ToolPin>),
    Device(github::DeviceCode),
    SignedIn(bool),
    ModelStatus(Settings, String),
}
struct App {
    engine: Engine,
    page: Page,
    settings: Settings,
    url: String,
    task: String,
    issue: String,
    sweep: bool,
    run: Option<Run>,
    proposal_json: String,
    edit_proposal: bool,
    message: String,
    pending: Option<mpsc::Receiver<Result<JobResult, String>>>,
    cancel: process::Cancel,
    approve_exec: bool,
    reviewed: BTreeSet<String>,
    rules: bool,
    duplicates: bool,
    issue_rule: bool,
    responsibility: bool,
    title: String,
    body: String,
    signoff: bool,
    confirm_publish: bool,
    token: String,
    device: Option<github::DeviceCode>,
    last_poll: u64,
    runtime_consent: bool,
    context_preview: String,
    gpu_live: bool,
    gpu_status: String,
    gpu_rx: Option<mpsc::Receiver<String>>,
    gpu_poll: std::time::Instant,
    mcp_path: String,
    mcp_config: Option<mcp::Config>,
    mcp_name: String,
    mcp_consent: bool,
    tools: Vec<mcp::ToolPin>,
    tool_index: usize,
    tool_args: String,
    tool_consent: bool,
    tool_pinned: bool,
    tool_readonly: bool,
}
impl App {
    fn new(engine: Engine) -> Self {
        let settings = engine.store.settings().unwrap_or_default();
        Self{engine,page:Page::Home,settings,url:String::new(),task:String::new(),issue:String::new(),sweep:false,run:None,proposal_json:String::new(),edit_proposal:false,message:"Choose a repository to begin. All changes remain local until you approve publishing.".into(),pending:None,cancel:process::cancel_token(),approve_exec:false,reviewed:BTreeSet::new(),rules:false,duplicates:false,issue_rule:false,responsibility:false,title:String::new(),body:String::new(),signoff:false,confirm_publish:false,token:String::new(),device:None,last_poll:0,runtime_consent:false,context_preview:String::new(),gpu_live:false,gpu_status:String::new(),gpu_rx:None,gpu_poll:std::time::Instant::now(),mcp_path:String::new(),mcp_config:None,mcp_name:String::new(),mcp_consent:false,tools:vec![],tool_index:0,tool_args:"{}".into(),tool_consent:false,tool_pinned:false,tool_readonly:true}
    }
    fn job<F>(&mut self, f: F)
    where
        F: FnOnce(Engine, process::Cancel) -> anyhow::Result<JobResult> + Send + 'static,
    {
        if self.pending.is_some() {
            return;
        }
        let e = self.engine.clone();
        let token = process::cancel_token();
        self.cancel = token.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.message = "Working. You can stop at any time.".into();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(e, token)))
                .map_err(|_| {
                    "Worker stopped unexpectedly; saved state can be inspected in History"
                        .to_owned()
                })
                .and_then(|r| r.map_err(|e| format!("{e:#}")));
            let _ = tx.send(result);
        });
    }
    fn observe(&mut self) {
        let result = self.pending.as_ref().and_then(|r| r.try_recv().ok());
        if let Some(result) = result {
            self.pending = None;
            match result {
                Ok(JobResult::Run(r)) => {
                    let new = self.run.as_ref().map_or(true, |old| old.id != r.id);
                    if new {
                        self.reviewed.clear();
                        self.approve_exec = false;
                        self.confirm_publish = false;
                        self.rules = false;
                        self.duplicates = false;
                        self.issue_rule = false;
                        self.responsibility = false;
                    }
                    self.message = format!("{:?}: {}", r.state, r.evidence.reason);
                    if r.proposal.is_some() {
                        self.page = Page::Review;
                    }
                    self.run = Some(r)
                }
                Ok(JobResult::Message(m)) => self.message = m,
                Ok(JobResult::Tools(t)) => {
                    self.tools = t;
                    self.tool_pinned = false;
                    self.tool_consent = false;
                    self.tool_index = 0;
                    self.message =
                        "Tools discovered. Review and pin a tool before calling it.".into()
                }
                Ok(JobResult::Device(d)) => {
                    self.message = format!("Open {} and enter {}", d.verification_uri, d.user_code);
                    self.device = Some(d)
                }
                Ok(JobResult::ModelStatus(settings, message)) => {
                    self.settings = settings;
                    self.message = message;
                }
                Ok(JobResult::SignedIn(done)) => {
                    if done {
                        self.device = None;
                        self.message = "GitHub credential saved to your OS keyring.".into()
                    } else {
                        self.message = "GitHub is waiting for your approval.".into()
                    }
                }
                Err(e) => {
                    self.message = e;
                    if let Some(r) = &self.run {
                        if let Ok(latest) = self.engine.store.get(&r.id) {
                            self.run = Some(latest);
                        }
                    }
                }
            }
        }
    }
    fn home_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Fix one Rust bug, with evidence");
        ui.label("Investigate a public repository, review a small change, test it in isolation, and decide whether to open a draft pull request.");
        ui.separator();
        ui.heading("First-run setup");
        ui.label("1. In Model & settings, choose your installed Rust inference runtime and complete local model folder. Review its license and enable CPU mode if needed.");
        ui.label("2. Build the test sandbox once with scripts/build-sandbox.sh --approve-image-download. Repository tests never run on your host.");
        ui.label("3. Open a repository or try the supplied addition-bug demo. MCP is optional; import your Desktop JSON file in the MCP tab.");
        if ui.button("Configure local model").clicked() {
            self.page = Page::Settings;
        }
        if ui.button("Investigate a repository").clicked() {
            self.page = Page::Task;
        }
        if ui.button("Try the known-bug demo").clicked() {
            self.job(|e, _| {
                let mut r = e.fixture()?;
                let p: Proposal =
                    serde_json::from_str(include_str!("../../../fixtures/fix-add.json"))?;
                e.replace_proposal(&mut r, p)?;
                Ok(JobResult::Run(r))
            });
        }
        ui.separator();
        ui.heading("Status");
        let running = self
            .engine
            .runtime
            .lock()
            .map(|mut r| r.running())
            .unwrap_or(false);
        ui.label(if running {
            "Managed model process: running"
        } else {
            "Managed model process: stopped (an external local provider may still be available)"
        });
        ui.label(format!("Inference endpoint: {}", self.settings.endpoint));
        if let Ok(runs) = self.engine.store.list() {
            ui.separator();
            ui.heading("Recent runs");
            for r in runs.into_iter().take(5) {
                if ui
                    .button(format!(
                        "{}/{} — {:?} — evidence {}",
                        r.owner, r.repo, r.state, r.evidence.level
                    ))
                    .clicked()
                {
                    self.run = Some(r);
                    self.reviewed.clear();
                    self.confirm_publish = false;
                    self.page = Page::Review;
                }
            }
        }
    }
    fn task_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Investigate a Rust repository");
        ui.label("Public GitHub repositories only. A bug sweep produces a findings report.");
        ui.label("Repository URL");
        ui.text_edit_singleline(&mut self.url);
        ui.checkbox(&mut self.sweep, "Bug sweep (report only)");
        if !self.sweep {
            ui.horizontal(|ui| {
                ui.label("Open issue number (optional)");
                ui.text_edit_singleline(&mut self.issue);
            });
        }
        ui.label("Problem or scan focus");
        ui.add(
            egui::TextEdit::multiline(&mut self.task)
                .desired_rows(5)
                .desired_width(f32::INFINITY),
        );
        if ui.button("Open and pin repository").clicked() {
            let url = self.url.clone();
            let task = self.task.clone();
            let mode = if self.sweep { Mode::Sweep } else { Mode::Issue };
            match if self.issue.trim().is_empty() || self.sweep {
                Ok(None)
            } else {
                self.issue.trim().parse::<u64>().map(Some)
            } {
                Ok(issue) => self
                    .job(move |e, c| Ok(JobResult::Run(e.create(&url, &task, mode, issue, &c)?))),
                Err(_) => self.message = "Issue number must be a positive integer".into(),
            }
        }
        if ui.button("Try the local addition-bug fixture").clicked() {
            self.job(|e, _| {
                let mut r = e.fixture()?;
                let p: Proposal =
                    serde_json::from_str(include_str!("../../../fixtures/fix-add.json"))?;
                e.replace_proposal(&mut r, p)?;
                Ok(JobResult::Run(r))
            });
        }
        if let Some(r) = self.run.clone() {
            ui.separator();
            ui.heading(format!("{}/{}", r.owner, r.repo));
            ui.monospace(format!("Pinned {}", r.commit));
            ui.label(format!(
                "{} files, {:.1} MB; license {}",
                r.preflight.file_count,
                r.preflight.bytes as f64 / 1_000_000.,
                r.preflight.license
            ));
            egui::CollapsingHeader::new("Execution preflight flags")
                .default_open(true)
                .show(ui, |ui| {
                    for f in &r.preflight.flags {
                        ui.label(f);
                    }
                });
            egui::CollapsingHeader::new("Contribution rules and policies").show(ui, |ui| {
                for (name, text) in &r.preflight.policies {
                    ui.collapsing(name, |ui| {
                        ui.add(egui::Label::new(text).selectable(true));
                    });
                }
            });
            egui::CollapsingHeader::new("Recent issues and pull requests (check duplicates)").show(
                ui,
                |ui| {
                    for (title, url) in &r.preflight.related {
                        ui.hyperlink_to(title, url);
                    }
                },
            );
            ui.label(format!(
                "Inference destination: local model at {}",
                self.settings.endpoint
            ));
            ui.label(format!(
                "Input budget: {} tokens; output cap: {} tokens",
                self.settings.context_tokens, self.settings.max_output_tokens
            ));
            if ui.button("Preview source scope before inference").clicked() {
                match repair_core::model::preview(&r, &self.settings) {
                    Ok((prompt, paths, tokens, exact)) => {
                        self.context_preview = format!(
                            "{} input {}\nFiles: {}\n{}",
                            tokens,
                            if exact {
                                "tokens"
                            } else {
                                "conservative byte budget"
                            },
                            paths.join(", "),
                            prompt
                        );
                    }
                    Err(e) => self.message = e.to_string(),
                }
            }
            ui.collapsing("Exact source preview (untrusted)", |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(&self.context_preview).monospace())
                        .selectable(true),
                );
            });
            if ui
                .add_enabled(
                    matches!(
                        r.state,
                        State::Pinned
                            | State::AwaitPlan
                            | State::AwaitTools
                            | State::NoFix
                            | State::Cancelled
                            | State::Review
                    ) && r.attempts < 3,
                    egui::Button::new("Analyze with my local model"),
                )
                .clicked()
            {
                let settings = self.settings.clone();
                self.job(move |e, c| {
                    let mut r = r;
                    e.recover(&mut r)?;
                    e.analyze(&mut r, &settings, &c)?;
                    Ok(JobResult::Run(r))
                });
            }
        }
    }
    fn review_page(&mut self, ui: &mut egui::Ui) {
        let Some(r) = self.run.clone() else {
            ui.label("Open a repository first.");
            return;
        };
        ui.heading(format!("Review - {:?}", r.state));
        ui.label(format!(
            "Evidence level {}: {}",
            r.evidence.level, r.evidence.reason
        ));
        if r.proposal.is_some() && !matches!(r.state, State::Publishing | State::Published) {
            ui.horizontal(|ui| {
                if ui.button("Edit proposal JSON").clicked() {
                    self.proposal_json =
                        serde_json::to_string_pretty(&r.proposal).unwrap_or_default();
                    self.edit_proposal = true;
                }
                if ui.button("Reject proposal").clicked() {
                    let r = r.clone();
                    self.job(move |e, _| {
                        let mut r = r;
                        e.reject(&mut r)?;
                        Ok(JobResult::Run(r))
                    });
                }
            });
            if self.edit_proposal {
                ui.label(
                    "Saving edits resets the working copy and invalidates previous test evidence.",
                );
                ui.add(
                    egui::TextEdit::multiline(&mut self.proposal_json)
                        .code_editor()
                        .desired_rows(16)
                        .desired_width(f32::INFINITY),
                );
                if ui.button("Validate and save edited proposal").clicked() {
                    match serde_json::from_str::<Proposal>(&self.proposal_json) {
                        Ok(p) => {
                            let r = r.clone();
                            self.edit_proposal = false;
                            self.reviewed.clear();
                            self.confirm_publish = false;
                            self.job(move |e, _| {
                                let mut r = r;
                                e.replace_proposal(&mut r, p)?;
                                Ok(JobResult::Run(r))
                            });
                        }
                        Err(e) => self.message = e.to_string(),
                    }
                }
            }
        }
        if r.state == State::Pinned && !r.tool_results.is_empty() {
            if ui
                .button("Continue analysis with reviewed tool results")
                .clicked()
            {
                let settings = self.settings.clone();
                self.job(move |e, c| {
                    let mut r = r;
                    e.analyze(&mut r, &settings, &c)?;
                    Ok(JobResult::Run(r))
                });
                return;
            }
        }
        if r.state == State::Review && r.evidence.level < 3 && r.attempts < 3 {
            if ui
                .button("Revise with test feedback (bounded retry)")
                .clicked()
            {
                let settings = self.settings.clone();
                self.reviewed.clear();
                self.job(move |e, c| {
                    let mut r = r;
                    e.analyze(&mut r, &settings, &c)?;
                    Ok(JobResult::Run(r))
                });
                return;
            }
        }
        if let Some(p) = r.proposal.clone() {
            if r.state == State::AwaitTools {
                ui.heading("Model-requested tools");
                ui.label(
                    "The model cannot execute these. Review the approved tool and exact arguments.",
                );
                for (i, request) in p.tool_requests.iter().enumerate() {
                    ui.label(format!("{} / {}", request.server, request.name));
                    ui.monospace(
                        serde_json::to_string_pretty(&request.arguments).unwrap_or_default(),
                    );
                    let read_only = r.mcp_approvals.iter().any(|a| {
                        a.enabled
                            && a.read_only
                            && a.server == request.server
                            && a.pin.name == request.name
                    });
                    if !read_only {
                        ui.checkbox(
                            &mut self.tool_consent,
                            "I approve these exact arguments and possible writes or egress",
                        );
                    }
                    if ui
                        .add_enabled(
                            read_only || self.tool_consent,
                            egui::Button::new("Run this approved tool"),
                        )
                        .clicked()
                    {
                        let r = r.clone();
                        let consent = self.tool_consent;
                        self.tool_consent = false;
                        self.job(move |e, c| {
                            let mut r = r;
                            e.requested_tool(&mut r, i, consent, &c)?;
                            Ok(JobResult::Run(r))
                        });
                        return;
                    }
                }
                return;
            }

            ui.heading("Model proposal (untrusted claim)");
            ui.add(egui::Label::new(&p.explanation).selectable(true));
            for cite in &p.citations {
                ui.collapsing(format!("{}:{}", cite.path, cite.start_line), |ui| {
                    ui.monospace(&cite.snippet);
                });
            }
            ui.label(format!("Planned files: {}", p.planned_files.join(", ")));
            if r.state == State::AwaitPlan && ui.button("Approve plan").clicked() {
                self.job(move |e, _| {
                    let mut r = r;
                    e.approve_plan(&mut r)?;
                    Ok(JobResult::Run(r))
                });
                return;
            }
            for edit in &p.edits {
                ui.separator();
                let mut checked = self.reviewed.contains(&edit.path);
                if ui
                    .checkbox(&mut checked, format!("Reviewed {}", edit.path))
                    .changed()
                {
                    if checked {
                        self.reviewed.insert(edit.path.clone());
                    } else {
                        self.reviewed.remove(&edit.path);
                    }
                }
                ui.columns(2, |columns| {
                    columns[0].strong("Before");
                    columns[0].add(
                        egui::Label::new(egui::RichText::new(&edit.old).monospace())
                            .selectable(true),
                    );
                    columns[1].strong("After");
                    columns[1].add(
                        egui::Label::new(egui::RichText::new(&edit.new).monospace())
                            .selectable(true),
                    );
                });
            }
            if let Some(test) = &p.reproduction {
                ui.separator();
                let mut checked = self.reviewed.contains(&test.path);
                if ui
                    .checkbox(&mut checked, format!("Reviewed new test: {}", test.path))
                    .changed()
                {
                    if checked {
                        self.reviewed.insert(test.path.clone());
                    } else {
                        self.reviewed.remove(&test.path);
                    }
                }
                ui.collapsing("Reproduction source", |ui| {
                    ui.monospace(&test.content);
                });
            }
            if r.state == State::AwaitPatch {
                ui.separator();
                ui.label("Reproduce on the unchanged source before applying the production patch.");
                ui.checkbox(
                    &mut self.approve_exec,
                    "I approve checksum-verified dependency downloads and offline sandbox tests",
                );
                if ui
                    .add_enabled(
                        self.approve_exec,
                        egui::Button::new("Run original-source baseline twice"),
                    )
                    .clicked()
                {
                    let settings = self.settings.clone();
                    self.job(move |e, c| {
                        let mut r = r;
                        e.baseline(&mut r, &settings, true, &c)?;
                        Ok(JobResult::Run(r))
                    });
                    return;
                }
            }
            if r.state == State::AwaitPatch
                && ui
                    .add_enabled(
                        github::changed_files(&r)
                            .iter()
                            .all(|f| self.reviewed.contains(f)),
                        egui::Button::new(if r.evidence.level >= 2 {
                            "Apply reviewed patch locally"
                        } else {
                            "Apply without reproduction evidence (level 1)"
                        }),
                    )
                    .clicked()
            {
                self.job(move |e, _| {
                    let mut r = r;
                    e.apply_patch(&mut r)?;
                    Ok(JobResult::Run(r))
                });
                return;
            }
            if matches!(r.state, State::Patched | State::Review | State::Cancelled)
                && r.mode == Mode::Issue
                && !p.edits.is_empty()
            {
                ui.separator();
                ui.heading("Run tests in rootless Podman");
                ui.label("Command: cargo test --offline --locked -- --test-threads=1 (twice before, twice after)");
                ui.label("No credentials, network or GPU in the container. 4 GB RAM, 2 CPUs, 256 processes, 1 GB temporary disk, 10 minutes per run. Repository Cargo config and toolchain files excluded from execution copies.");
                ui.checkbox(&mut self.approve_exec,"I approve downloading checksum-verified crates.io dependencies and executing this repository in the sandbox");
                if ui
                    .add_enabled(
                        self.approve_exec,
                        egui::Button::new("Run baseline and verification"),
                    )
                    .clicked()
                {
                    let settings = self.settings.clone();
                    self.job(move |e, c| {
                        let mut r = r;
                        e.recover(&mut r)?;
                        e.verify(&mut r, &settings, true, &c)?;
                        Ok(JobResult::Run(r))
                    });
                    return;
                }
            }
        }
        for (label, observations) in [
            ("Baseline observations", &r.evidence.baseline),
            ("After-patch observations", &r.evidence.after),
        ] {
            ui.collapsing(label, |ui| {
                for (i, o) in observations.iter().enumerate() {
                    ui.collapsing(
                        format!(
                            "Run {} - exit {:?}, {} ms",
                            i + 1,
                            o.exit_code,
                            o.elapsed_ms
                        ),
                        |ui| {
                            ui.add(
                                egui::Label::new(egui::RichText::new(&o.output).monospace())
                                    .selectable(true),
                            );
                        },
                    );
                }
            });
        }
        if ui.button("Export full run record").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name(format!("{}.json", r.id))
                .save_file()
            {
                self.job(move|e,_|{e.export(&r,&path)?;Ok(JobResult::Message("Run record exported. It includes source context and test logs; review before sharing.".into()))});
            }
            return;
        }
        if r.state == State::Review && r.evidence.level == 3 {
            ui.separator();
            ui.heading("Publish one draft pull request");
            ui.label("Read the project's rules above. Policy interpretation is yours; the model cannot approve this checklist.");
            ui.checkbox(
                &mut self.rules,
                "I read the contribution and AI policies; this project permits this contribution",
            );
            ui.checkbox(
                &mut self.duplicates,
                "I checked existing issues and PRs; this contribution is not a duplicate",
            );
            ui.checkbox(
                &mut self.issue_rule,
                "Any issue/discussion prerequisite required by the project is satisfied",
            );
            ui.checkbox(
                &mut self.responsibility,
                "I reviewed the code and accept responsibility for the contribution",
            );
            ui.checkbox(
                &mut self.signoff,
                "The project requires DCO; I personally certify the sign-off",
            );
            ui.label("PR title");
            ui.text_edit_singleline(&mut self.title);
            ui.label("Optional additional description");
            ui.add(
                egui::TextEdit::multiline(&mut self.body)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            ui.collapsing(
                "Mandatory AI disclosure and evidence appended to the PR",
                |ui| {
                    ui.label(github::draft_body(&r));
                },
            );
            ui.label(format!(
                "Target: {} | base: {} | branch: repair/{}",
                r.source, r.commit, r.id
            ));
            ui.checkbox(&mut self.confirm_publish,"Confirm: create/reuse my fork, push this new branch, and create one DRAFT pull request");
            if github::policy_blocks_ai(&r) {
                ui.colored_label(
                    egui::Color32::RED,
                    "An AI-contribution prohibition was detected. Publishing is blocked.",
                );
            }
            let ready = !github::policy_blocks_ai(&r)
                && self.confirm_publish
                && self.rules
                && self.duplicates
                && self.issue_rule
                && self.responsibility
                && !self.title.trim().is_empty()
                && github::changed_files(&r)
                    .iter()
                    .all(|f| self.reviewed.contains(f));
            if ui
                .add_enabled(ready, egui::Button::new("Publish reviewed draft PR"))
                .clicked()
            {
                let settings = self.settings.clone();
                let a = PublishApproval {
                    reviewed_files: self.reviewed.iter().cloned().collect(),
                    rules_allow_ai: self.rules,
                    duplicates_checked: self.duplicates,
                    issue_requirement_met: self.issue_rule,
                    human_responsibility: self.responsibility,
                    title: self.title.clone(),
                    body: self.body.clone(),
                    signoff: self.signoff,
                };
                let publishing = r.clone();
                self.job(move |e, c| {
                    let mut r = publishing;
                    e.publish(&mut r, &a, &settings, &c)?;
                    Ok(JobResult::Run(r))
                });
            }
        }
        if let Some(url) = &r.pr_url {
            ui.hyperlink_to("Open created pull request", url);
        }
    }
    fn settings_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Local model and GPU");
        ui.label("Connect to an existing local model, or explicitly start your installed mistralrs runtime with a complete local model folder. Model downloads are never automatic.");
        ui.label("Installed mistralrs executable");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.settings.runtime_path);
            if ui.button("Browse runtime").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    self.settings.runtime_path = p.display().to_string();
                }
            }
        });
        ui.label("Complete local model folder");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.settings.model_path);
            if ui.button("Browse model folder").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    self.settings.model_path = p.display().to_string();
                }
            }
        });
        if ui.button("Estimate selected model memory").clicked() {
            let model = PathBuf::from(&self.settings.model_path);
            let context = self.settings.context_tokens + self.settings.max_output_tokens + 512;
            self.job(move |_, _| {
                Ok(JobResult::Message(repair_core::runtime::memory_estimate(
                    &model, context,
                )?))
            });
        }
        ui.checkbox(
            &mut self.gpu_live,
            "Show live NVIDIA load, free VRAM and temperature",
        );
        if !self.gpu_status.is_empty() {
            ui.label(&self.gpu_status);
        }
        ui.label("Model license (record the license supplied with your model)");
        ui.text_edit_singleline(&mut self.settings.model_license);
        ui.checkbox(
            &mut self.settings.cpu,
            "Use CPU instead of GPU (generation will be slower)",
        );
        ui.monospace(format!("Runtime command: {:?} serve --host 127.0.0.1 --port <endpoint port> -m {:?} --max-seq-len {}{}",self.settings.runtime_path,self.settings.model_path,self.settings.context_tokens+self.settings.max_output_tokens+512,if self.settings.cpu {" --cpu"}else{""}));
        ui.checkbox(
            &mut self.runtime_consent,
            "I approve launching this local executable with the selected local model",
        );
        if ui
            .add_enabled(self.runtime_consent, egui::Button::new("Load local model"))
            .clicked()
        {
            let settings = self.settings.clone();
            self.job(move |e, _| {
                e.store.save_settings(&settings)?;
                e.runtime
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Runtime state unavailable"))?
                    .load(&settings, &e.store.root.join("model-runtime"), true)?;
                Ok(JobResult::Message(
                    "Local model process started. Check connection when loading finishes.".into(),
                ))
            });
        }
        if ui.button("Unload managed model").clicked() {
            if let Ok(mut r) = self.engine.runtime.lock() {
                r.unload();
            }
            self.message = "Managed model stopped and its GPU resources released.".into();
        }
        if ui.button("Check local model connection").clicked() {
            let settings = self.settings.clone();
            self.job(move |e, _| {
                let models = repair_core::model::health(&settings)?;
                let mut settings = settings;
                let ids = models
                    .split(", ")
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>();
                if ids.len() == 1 {
                    settings.model_id = ids[0].into();
                    e.store.save_settings(&settings)?;
                }
                Ok(JobResult::ModelStatus(
                    settings,
                    format!("Available local models: {models}"),
                ))
            });
        }
        if let Ok(r) = self.engine.runtime.lock() {
            if let Ok(log) = r.log.lock() {
                ui.collapsing("Local runtime log", |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&*log).monospace()).selectable(true),
                    );
                });
            }
        }
        ui.label(
            "Tokenizer JSON for accurate input budgeting (otherwise conservative byte budget)",
        );
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.settings.tokenizer_path);
            if ui.button("Browse tokenizer").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Tokenizer JSON", &["json"])
                    .pick_file()
                {
                    self.settings.tokenizer_path = p.display().to_string();
                }
            }
        });
        ui.add(
            egui::Slider::new(&mut self.settings.context_tokens, 2048..=16384)
                .text("Input token budget"),
        );
        ui.label("OpenAI-compatible chat endpoint");
        ui.text_edit_singleline(&mut self.settings.endpoint);
        ui.label("Model identifier served by that endpoint");
        ui.text_edit_singleline(&mut self.settings.model_id);
        ui.label("Weights file (optional; SHA256 recorded, server/model match is user verified)");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.settings.weights_path);
            if ui.button("Browse weights").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    self.settings.weights_path = p.display().to_string();
                }
            }
        });
        ui.add(
            egui::Slider::new(&mut self.settings.context_chars, 2000..=64000)
                .text("Context character budget"),
        );
        ui.add(
            egui::Slider::new(&mut self.settings.max_output_tokens, 512..=8192)
                .text("Maximum output tokens"),
        );
        if ui.button("Check NVIDIA GPU").clicked() {
            self.job(|_, c| {
                let mut cmd = std::process::Command::new("nvidia-smi");
                cmd.env_clear()
                    .env("PATH", "/usr/local/bin:/usr/bin:/bin")
                    .args([
                        "--query-gpu=name,memory.total,memory.free,utilization.gpu,temperature.gpu",
                        "--format=csv,noheader",
                    ]);
                let o = process::success(process::run(
                    cmd,
                    vec!["nvidia-smi".into()],
                    std::time::Duration::from_secs(10),
                    &c,
                )?)?;
                Ok(JobResult::Message(format!(
                    "GPU / total VRAM / free VRAM / load / temperature:\n{}",
                    o.output
                )))
            });
        }
        ui.separator();
        ui.heading("Test sandbox");
        ui.label("Local image name (must be built explicitly; app never pulls images)");
        ui.text_edit_singleline(&mut self.settings.image);
        ui.label("See scripts/build-sandbox.sh for the rootless Podman image.");
        ui.separator();
        ui.heading("GitHub sign-in");
        ui.label("Optional OAuth app client ID with device flow enabled (registration required; no app server)");
        ui.text_edit_singleline(&mut self.settings.oauth_client_id);
        if ui.button("Start GitHub device sign-in").clicked() {
            let id = self.settings.oauth_client_id.clone();
            self.job(move |_, _| Ok(JobResult::Device(github::device_start(&id)?)));
        }
        if let Some(d) = self.device.clone() {
            ui.hyperlink_to("Open GitHub device sign-in", &d.verification_uri);
            ui.monospace(&d.user_code);
            if ui
                .add_enabled(
                    now() >= self.last_poll + d.interval.max(10),
                    egui::Button::new("Check sign-in approval"),
                )
                .clicked()
            {
                self.last_poll = now();
                let id = self.settings.oauth_client_id.clone();
                self.job(move |_, _| Ok(JobResult::SignedIn(github::device_poll(&id, &d)?)));
            }
        }
        ui.label("Alternatively supply a GitHub OAuth/classic public_repo token. Fine-grained tokens may not create cross-owner PRs. Token is stored only in the OS keyring.");
        ui.add(egui::TextEdit::singleline(&mut self.token).password(true));
        if ui.button("Save token to keyring").clicked() {
            match github::save_token(&self.token) {
                Ok(()) => {
                    self.token.clear();
                    self.message = "Token saved to the OS keyring".into()
                }
                Err(e) => self.message = e.to_string(),
            }
        }
        if ui.button("Forget local GitHub credential").clicked() {
            self.message = github::revoke_local()
                .map(|_| "Local credential deleted; revoke on GitHub separately if needed".into())
                .unwrap_or_else(|e| e.to_string());
        }
        ui.label("Commit author name");
        ui.text_edit_singleline(&mut self.settings.author_name);
        ui.label("Author email (blank uses GitHub no-reply address)");
        ui.text_edit_singleline(&mut self.settings.author_email);
        if ui.button("Save settings").clicked() {
            self.message = self
                .engine
                .store
                .save_settings(&self.settings)
                .map(|_| "Settings saved".into())
                .unwrap_or_else(|e| e.to_string());
        }
    }
    fn mcp_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Optional Desktop MCP configuration");
        ui.label("MCP supplies tools, not GPU model weights. The repair loop works without MCP.");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.mcp_path);
            if ui.button("Browse JSON").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("JSON", &["json"])
                    .pick_file()
                {
                    self.mcp_path = p.display().to_string();
                }
            }
        });
        if ui.button("Import (does not launch anything)").clicked() {
            match mcp::import(&PathBuf::from(&self.mcp_path)) {
                Ok(c) => {
                    self.mcp_name = c.servers.keys().next().cloned().unwrap_or_default();
                    self.mcp_config = Some(c);
                    self.tools.clear();
                    self.mcp_consent = false;
                    self.message = "Configuration imported; original file not changed".into()
                }
                Err(e) => self.message = e.to_string(),
            }
        }
        let Some(config) = self.mcp_config.clone() else {
            return;
        };
        egui::ComboBox::from_label("Server")
            .selected_text(&self.mcp_name)
            .show_ui(ui, |ui| {
                for name in config.servers.keys() {
                    if ui
                        .selectable_value(&mut self.mcp_name, name.clone(), name)
                        .changed()
                    {
                        self.tools.clear();
                        self.mcp_consent = false;
                        self.tool_pinned = false;
                    }
                }
            });
        let Some(srv) = config.servers.get(&self.mcp_name).cloned() else {
            return;
        };
        ui.monospace(format!(
            "Command: {}\nArguments: {:?}\nEnvironment names: {:?}\nRemote URL: {:?}",
            srv.command.as_deref().unwrap_or("none"),
            srv.args,
            srv.env.keys().collect::<Vec<_>>(),
            srv.url
        ));
        ui.label("This local process runs with your user privileges. Its filesystem/network access cannot be constrained by MCP roots. The app sends no GitHub token; only explicitly approved environment values are passed.");
        ui.checkbox(
            &mut self.mcp_consent,
            "I trust this executable and approve launching it to discover its tools",
        );
        if ui
            .add_enabled(
                self.mcp_consent,
                egui::Button::new("Connect and inspect tool definitions"),
            )
            .clicked()
        {
            let workspace = self.engine.store.root.join("mcp-workspace");
            self.job(move |_, c| {
                std::fs::create_dir_all(&workspace)?;
                Ok(JobResult::Tools(mcp::inspect_cancel(
                    &srv, &workspace, true, &c,
                )?))
            });
            return;
        }
        if !self.tools.is_empty() {
            egui::ComboBox::from_label("Tool")
                .selected_text(
                    self.tools
                        .get(self.tool_index)
                        .map(|t| t.name.as_str())
                        .unwrap_or(""),
                )
                .show_ui(ui, |ui| {
                    for (i, t) in self.tools.iter().enumerate() {
                        if ui
                            .selectable_value(&mut self.tool_index, i, &t.name)
                            .changed()
                        {
                            self.tool_pinned = false;
                            self.tool_consent = false;
                        }
                    }
                });
            if let Some(pin) = self.tools.get(self.tool_index).cloned() {
                ui.collapsing("Full untrusted tool definition", |ui| {
                    ui.monospace(serde_json::to_string_pretty(&pin.definition).unwrap_or_default());
                });
                ui.monospace(format!("Definition SHA256: {}", pin.hash));
                if ui
                    .checkbox(
                        &mut self.tool_pinned,
                        "I reviewed and approve this pinned tool definition",
                    )
                    .changed()
                    && self.tool_pinned
                {
                    let file = self.engine.store.root.join(format!(
                        "mcp-pin-{}.json",
                        repair_core::validation::digest(
                            format!("{}:{}:{}", self.mcp_path, self.mcp_name, pin.name).as_bytes()
                        )
                    ));
                    let result =
                        std::fs::write(file, serde_json::to_vec_pretty(&pin).unwrap_or_default());
                    if let Err(e) = result {
                        self.tool_pinned = false;
                        self.message = e.to_string();
                    }
                }
                ui.checkbox(
                    &mut self.tool_readonly,
                    "Classify this tool as read-only for this run (server hints are ignored)",
                );
                if ui
                    .add_enabled(
                        self.tool_pinned && self.run.is_some(),
                        egui::Button::new("Allow this tool in the current repair run"),
                    )
                    .clicked()
                {
                    if let Some(r) = self.run.clone() {
                        let path = PathBuf::from(&self.mcp_path);
                        let server = self.mcp_name.clone();
                        let pin = pin.clone();
                        let read_only = self.tool_readonly;
                        self.job(move |e, _| {
                            let mut r = r;
                            e.approve_mcp(&mut r, &path, &server, &pin, read_only)?;
                            Ok(JobResult::Run(r))
                        });
                    }
                }
                if ui.button("Disable all tools in this run").clicked() {
                    if let Some(r) = &mut self.run {
                        for a in &mut r.mcp_approvals {
                            a.enabled = false;
                        }
                        if let Err(e) = self.engine.store.save(r) {
                            self.message = e.to_string();
                        }
                    }
                }
                ui.label("Exact tool payload preview (JSON object)");
                ui.add(
                    egui::TextEdit::multiline(&mut self.tool_args)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY),
                );
                ui.checkbox(
                    &mut self.tool_consent,
                    "I approve this call and any data access, writes, or egress it causes",
                );
                if ui
                    .add_enabled(
                        self.tool_pinned && self.tool_consent,
                        egui::Button::new("Call reviewed tool once"),
                    )
                    .clicked()
                {
                    match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                        &self.tool_args,
                    ) {
                        Ok(args) => {
                            let workspace = self.engine.store.root.join("mcp-workspace");
                            self.tool_consent = false;
                            self.job(move |_, c| {
                                let out = mcp::call_cancel(&srv, &workspace, &pin, args, true, &c)?;
                                Ok(JobResult::Message(format!("UNTRUSTED MCP RESULT\n{out}")))
                            });
                        }
                        Err(e) => self.message = e.to_string(),
                    }
                }
            }
        }
    }
    fn history_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Local history");
        match self.engine.store.list() {
            Ok(runs) => {
                for r in runs {
                    if r.root.exists()
                        && ui
                            .button(format!("Delete workspace {} (retain history)", r.id))
                            .clicked()
                    {
                        self.job(move |e, _| {
                            let mut r = r;
                            e.delete_workspace(&mut r)?;
                            Ok(JobResult::Message(
                                "Workspace deleted; history and PR metadata retained.".into(),
                            ))
                        });
                        return;
                    }
                    if ui
                        .button(format!(
                            "{} - {}/{} - {:?} - level {}",
                            r.id, r.owner, r.repo, r.state, r.evidence.level
                        ))
                        .clicked()
                    {
                        self.run = Some(r);
                        self.reviewed.clear();
                        self.confirm_publish = false;
                        self.page = Page::Review;
                    }
                }
            }
            Err(e) => {
                ui.label(e.to_string());
            }
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.observe();
        if let Some(result) = self.gpu_rx.as_ref().and_then(|r| r.try_recv().ok()) {
            self.gpu_status = result;
            self.gpu_rx = None;
        }
        if self.gpu_live
            && self.gpu_rx.is_none()
            && self.gpu_poll.elapsed() > std::time::Duration::from_secs(2)
        {
            self.gpu_poll = std::time::Instant::now();
            let (tx, rx) = mpsc::channel();
            self.gpu_rx = Some(rx);
            std::thread::spawn(move || {
                let value = repair_core::runtime::gpu_status(&process::cancel_token())
                    .unwrap_or_else(|e| format!("GPU status unavailable: {e}"));
                let _ = tx.send(value);
            });
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(150));
        egui::TopBottomPanel::top("top").show(ctx,|ui|{ui.horizontal(|ui|{ui.heading("Rust Repair");ui.separator();for(label,page)in [("Home",Page::Home),("Task",Page::Task),("Review",Page::Review),("History",Page::History),("Model & settings",Page::Settings),("MCP",Page::Mcp)]{ui.selectable_value(&mut self.page,page,label);}if self.pending.is_some(){ui.spinner();if ui.button("Stop").clicked(){self.cancel.store(true,Ordering::SeqCst);if let Ok(mut r)=self.engine.runtime.lock(){r.unload();}self.message="Stop requested. External model servers may finish already submitted inference.".into();}}});});
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.add(egui::Label::new(&self.message).selectable(true));
            if self.pending.is_some() {
                if let Ok(activity) = self.engine.activity.lock() {
                    if !activity.is_empty() {
                        ui.collapsing("Streaming model output (untrusted)", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(120.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&*activity).monospace(),
                                        )
                                        .selectable(true),
                                    );
                                });
                        });
                    }
                }
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_enabled_ui(self.pending.is_none(), |ui| match self.page {
                    Page::Home => self.home_page(ui),
                    Page::Task => self.task_page(ui),
                    Page::Review => self.review_page(ui),
                    Page::Settings => self.settings_page(ui),
                    Page::Mcp => self.mcp_page(ui),
                    Page::History => self.history_page(ui),
                });
            });
        });
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Ok(mut r) = self.engine.runtime.lock() {
            r.unload();
        }
        self.cancel.store(true, Ordering::SeqCst);
        if let Some(rx) = self.pending.take() {
            let _ = rx.recv_timeout(std::time::Duration::from_secs(45));
        }
    }
}
fn main() -> eframe::Result<()> {
    let store = Store::open(store::default_root()).unwrap_or_else(|e| {
        eprintln!("Cannot open local store: {e}");
        std::process::exit(1)
    });
    let engine = Engine::new(store);
    eframe::run_native(
        "Rust Repair",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 800.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::light());
            Ok(Box::new(App::new(engine)))
        }),
    )
}
