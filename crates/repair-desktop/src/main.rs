use anyhow::Context;
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
    Queue,
    Review,
    Settings,
    Mcp,
    History,
}
enum JobResult {
    Run(Run),
    Message(String),
    Tickets {
        owner: String,
        repo: String,
        tickets: Vec<github::Ticket>,
    },
    Queue(Vec<QueueItemResult>),
    Tools(Vec<mcp::ToolPin>),
    Device(github::DeviceCode),
    SignedIn(bool),
    ModelStatus(Settings, String),
}
#[derive(Clone)]
struct QueueItemResult {
    ticket: github::Ticket,
    run: Option<Run>,
    error: Option<String>,
}
struct App {
    engine: Engine,
    page: Page,
    settings: Settings,
    url: String,
    task: String,
    issue: String,
    sweep: bool,
    ticket_limit: usize,
    tickets: Vec<github::Ticket>,
    tickets_repository: Option<(String, String)>,
    selected_tickets: BTreeSet<u64>,
    skip_seen_tickets: bool,
    queue_results: Vec<QueueItemResult>,
    run: Option<Run>,
    proposal_json: String,
    edit_proposal: bool,
    message: String,
    pending: Option<mpsc::Receiver<Result<JobResult, String>>>,
    progress: Option<mpsc::Receiver<String>>,
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
    runtime_approval: Option<String>,
    sandbox_consent: bool,
    context_preview: String,
    gpu_live: bool,
    gpu_status: String,
    gpu_rx: Option<mpsc::Receiver<String>>,
    gpu_poll: std::time::Instant,
    mcp_path: String,
    mcp_config: Option<mcp::Config>,
    mcp_config_path: Option<PathBuf>,
    mcp_config_hash: Option<String>,
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
        Self {
            engine,
            page: Page::Home,
            settings,
            url: String::new(),
            task: String::new(),
            issue: String::new(),
            sweep: false,
            ticket_limit: 100,
            tickets: vec![],
            tickets_repository: None,
            selected_tickets: BTreeSet::new(),
            skip_seen_tickets: true,
            queue_results: vec![],
            run: None,
            proposal_json: String::new(),
            edit_proposal: false,
            message: "Choose a repository to begin. All changes remain local until you approve publishing.".into(),
            pending: None,
            progress: None,
            cancel: process::cancel_token(),
            approve_exec: false,
            reviewed: BTreeSet::new(),
            rules: false,
            duplicates: false,
            issue_rule: false,
            responsibility: false,
            title: String::new(),
            body: String::new(),
            signoff: false,
            confirm_publish: false,
            token: String::new(),
            device: None,
            last_poll: 0,
            runtime_approval: None,
            sandbox_consent: false,
            context_preview: String::new(),
            gpu_live: false,
            gpu_status: String::new(),
            gpu_rx: None,
            gpu_poll: std::time::Instant::now(),
            mcp_path: String::new(),
            mcp_config: None,
            mcp_config_path: None,
            mcp_config_hash: None,
            mcp_name: String::new(),
            mcp_consent: false,
            tools: vec![],
            tool_index: 0,
            tool_args: "{}".into(),
            tool_consent: false,
            tool_pinned: false,
            tool_readonly: true,
        }
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
    fn progress_job<F>(&mut self, f: F)
    where
        F: FnOnce(Engine, process::Cancel, mpsc::Sender<String>) -> anyhow::Result<JobResult>
            + Send
            + 'static,
    {
        if self.pending.is_some() {
            return;
        }
        let engine = self.engine.clone();
        let cancel = process::cancel_token();
        self.cancel = cancel.clone();
        let (result_tx, result_rx) = mpsc::channel();
        let (progress_tx, progress_rx) = mpsc::channel();
        self.pending = Some(result_rx);
        self.progress = Some(progress_rx);
        self.message = "Starting GPUFold queue…".into();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                f(engine, cancel, progress_tx)
            }))
            .map_err(|_| {
                "Queue worker stopped unexpectedly; saved runs can be inspected in History"
                    .to_owned()
            })
            .and_then(|r| r.map_err(|e| format!("{e:#}")));
            let _ = result_tx.send(result);
        });
    }
    fn observe(&mut self) {
        if let Some(progress) = &self.progress {
            while let Ok(status) = progress.try_recv() {
                self.message = status;
            }
        }
        let result = self.pending.as_ref().and_then(|r| r.try_recv().ok());
        if let Some(result) = result {
            self.pending = None;
            self.progress = None;
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
                Ok(JobResult::Tickets {
                    owner,
                    repo,
                    tickets,
                }) => {
                    self.selected_tickets.clear();
                    self.tickets = tickets;
                    self.tickets_repository = Some((owner, repo));
                    self.message = format!(
                        "Found {} open unassigned issue(s). Select the tickets to process.",
                        self.tickets.len()
                    );
                }
                Ok(JobResult::Queue(results)) => {
                    let proposed = results
                        .iter()
                        .filter(|item| item.run.as_ref().is_some_and(|r| r.proposal.is_some()))
                        .count();
                    let failed = results.iter().filter(|item| item.error.is_some()).count();
                    self.queue_results = results;
                    self.page = Page::Queue;
                    self.message = format!(
                        "Queue finished: {proposed} proposal(s), {failed} item(s) need attention. Review each proposal before applying it."
                    );
                }
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
    fn build_sandbox_button(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(
            &mut self.sandbox_consent,
            "I approve downloading the official Rust base image and building the local test sandbox",
        );
        if ui
            .add_enabled(
                self.sandbox_consent,
                egui::Button::new("Prepare test sandbox"),
            )
            .clicked()
        {
            self.sandbox_consent = false;
            self.job(|_, cancel| {
                let script = sandbox_script().context(
                    "Sandbox setup script is missing. Reinstall GPUFold or build from the source checkout.",
                )?;
                let mut command = std::process::Command::new("bash");
                command
                    .arg(&script)
                    .arg("--approve-image-download");
                let output = process::run(
                    command,
                    vec!["bash".into(), script.display().to_string()],
                    std::time::Duration::from_secs(15 * 60),
                    &cancel,
                )?;
                process::success(output)?;
                Ok(JobResult::Message(
                    "SUCCESS: rootless test sandbox is ready. Repository tests stay offline inside it.".into(),
                ))
            });
        }
    }
    fn home_page(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(18.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("Work through unattended GitHub issues on your GPU");
                        ui.label("Point GPUFold at a public repository, choose your local model, and review repair proposals one ticket at a time.");
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("LOCAL FIRST").strong().color(egui::Color32::from_rgb(36, 104, 82)));
                    });
                });
            });

        ui.add_space(12.0);
        ui.heading("Get started");
            ui.label("The demo uses a supplied fix and never publishes. Queue processing uses your selected local model; test execution stays behind a separate approval.");
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_sized([220.0, 38.0], egui::Button::new("Try the known-bug demo"))
                .clicked()
            {
                self.job(|e, _| {
                    let mut r = e.fixture()?;
                    let p: Proposal =
                        serde_json::from_str(include_str!("../../../fixtures/fix-add.json"))?;
                    e.replace_proposal(&mut r, p)?;
                    Ok(JobResult::Run(r))
                });
            }
            if ui
                .add_sized([220.0, 38.0], egui::Button::new("Scan repository tickets"))
                .clicked()
            {
                self.page = Page::Task;
            }
            if ui
                .add_sized([190.0, 38.0], egui::Button::new("Configure local model"))
                .clicked()
            {
                self.page = Page::Settings;
            }
        });

        ui.add_space(14.0);
        ui.columns(3, |columns| {
            for (column, (number, title, detail)) in columns.iter_mut().zip([
                (
                    "01",
                    "Choose your model",
                    "Select local model files and a GPU-capable runtime or loopback endpoint.",
                ),
                (
                    "02",
                    "Queue unattended issues",
                    "GPUFold finds open, unassigned issues and processes them in order.",
                ),
                (
                    "03",
                    "Review every repair",
                    "Proposals are not applied or published until you review them.",
                ),
            ]) {
                egui::Frame::group(column.style())
                    .inner_margin(egui::Margin::same(12.0))
                    .show(column, |ui| {
                        ui.label(
                            egui::RichText::new(number)
                                .strong()
                                .color(egui::Color32::from_rgb(76, 103, 164)),
                        );
                        ui.label(
                            egui::RichText::new(title)
                                .strong()
                                .color(egui::Color32::from_rgb(35, 48, 74)),
                        );
                        ui.label(detail);
                    });
            }
        });

        ui.add_space(14.0);
        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Test sandbox");
                    ui.label("Rootless Podman · offline repository tests");
                });
                self.build_sandbox_button(ui);
                ui.small("Setup downloads the official Rust base image only after you approve it. Later test runs do not pull images.");
            });

        ui.add_space(14.0);
        let running = self
            .engine
            .runtime
            .lock()
            .map(|mut r| r.running())
            .unwrap_or(false);
        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Local model");
                    ui.label(if running { "Managed runtime running" } else { "Managed runtime stopped" });
                });
                ui.label(format!("Endpoint: {}", self.settings.endpoint));
                if self.settings.runtime_path.is_empty() || self.settings.model_path.is_empty() {
                    ui.small("No managed runtime is configured. You can also use an existing local endpoint.");
                }
                if self.gpu_live && !self.gpu_status.is_empty() {
                    ui.label(&self.gpu_status);
                }
            });

        ui.add_space(14.0);
        ui.heading("Recent runs");
        match self.engine.store.list() {
            Ok(runs) if runs.is_empty() => {
                ui.label("Your saved runs will appear here.");
            }
            Ok(runs) => {
                for r in runs.into_iter().take(5) {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}/{}", r.owner, r.repo));
                        ui.label(format!("{:?}", r.state));
                        ui.label(format!("Evidence {}", r.evidence.level));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Open review").clicked() {
                                self.run = Some(r.clone());
                                self.reviewed.clear();
                                self.confirm_publish = false;
                                self.page = Page::Review;
                            }
                        });
                    });
                    ui.separator();
                }
            }
            Err(error) => {
                ui.colored_label(
                    egui::Color32::RED,
                    format!("Could not read local history: {error}"),
                );
            }
        }
    }
    fn queue_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("GPUFold queue results");
        ui.label("Each ticket has its own saved run. Review the model proposal, approve its plan, apply it locally, and verify it in the sandbox.");
        if ui.button("Scan another repository or batch").clicked() {
            self.page = Page::Task;
        }
        if self.queue_results.is_empty() {
            ui.separator();
            ui.label("No queue results yet. Scan a repository and select open, unassigned tickets.");
            return;
        }
        let snapshot = self.queue_results.clone();
        ui.separator();
        for item in snapshot {
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::same(12.0))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.hyperlink_to(
                            format!("#{} · {}", item.ticket.number, item.ticket.title),
                            &item.ticket.url,
                        );
                        if let Some(run) = &item.run {
                            ui.label(format!("{:?}", run.state));
            ui.small(format!(
                "source {}",
                run.commit.get(..7).unwrap_or(&run.commit)
            ));
                            if run.proposal.is_some() {
                                ui.label("proposal ready");
                            }
                            let label = if run.proposal.is_some() {
                                "Review proposal"
                            } else {
                                "Open saved run"
                            };
                            if ui.button(label).clicked() {
                                self.run = Some(run.clone());
                                self.reviewed.clear();
                                self.confirm_publish = false;
                                self.page = Page::Review;
                            }
                        } else {
                            ui.weak("No run created");
                        }
                    });
                    if let Some(error) = &item.error {
                        ui.colored_label(egui::Color32::YELLOW, error);
                    }
                });
            ui.add_space(6.0);
        }
    }
    fn task_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("GPUFold · Repository ticket queue");
        ui.label("Scan public GitHub issues, then use your selected local model to prepare repair proposals in order.");
        ui.label("Repository URL (https://github.com/owner/repository)");
        ui.add(egui::TextEdit::singleline(&mut self.url).desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            ui.label("Maximum issues to scan");
            ui.add(egui::DragValue::new(&mut self.ticket_limit).clamp_range(1..=500));
            ui.label("Unassigned open issues only");
        });
        let repository_is_valid = github::parse_repo(&self.url).is_ok();
        if !self.url.trim().is_empty() && !repository_is_valid {
            ui.colored_label(
                egui::Color32::RED,
                "Enter a URL in the form https://github.com/owner/repository.",
            );
        }
        if ui
            .add_enabled(
                repository_is_valid,
                egui::Button::new("Scan for unattended tickets"),
            )
            .clicked()
        {
            let url = self.url.clone();
            let limit = self.ticket_limit;
            self.tickets.clear();
            self.tickets_repository = None;
            self.selected_tickets.clear();
            self.job(move |_, cancel| {
                let (owner, repo) = github::parse_repo(&url)?;
                let tickets = github::tickets(&owner, &repo, limit, true, &cancel)?;
                Ok(JobResult::Tickets {
                    owner,
                    repo,
                    tickets,
                })
            });
        }
        let tickets_match_repository =
            tickets_match_repository(&self.url, self.tickets_repository.as_ref());
        if !self.tickets.is_empty() && !tickets_match_repository {
            ui.colored_label(
                egui::Color32::YELLOW,
                "The repository URL changed. Scan it again before selecting or processing tickets.",
            );
        }
        if !self.tickets.is_empty() && tickets_match_repository {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{} open, unassigned issue(s) found. Pull requests are excluded.",
                    self.tickets.len()
                ));
                ui.checkbox(&mut self.skip_seen_tickets, "Skip issues already in GPUFold history");
            });
            let seen = self
                .engine
                .store
                .list()
                .unwrap_or_default()
                .into_iter()
                .filter(|r| {
                    github::parse_repo(&self.url).is_ok_and(|(owner, repo)| {
                        r.owner.eq_ignore_ascii_case(&owner)
                            && r.repo.eq_ignore_ascii_case(&repo)
                    })
                })
                .filter_map(|r| r.issue_number)
                .collect::<BTreeSet<_>>();
            let selectable = self
                .tickets
                .iter()
                .filter(|ticket| !self.skip_seen_tickets || !seen.contains(&ticket.number))
                .map(|ticket| ticket.number)
                .collect::<Vec<_>>();
            let selected_count = selectable
                .iter()
                .filter(|number| self.selected_tickets.contains(*number))
                .count();
            ui.horizontal(|ui| {
                if ui.button("Select first 10 new tickets").clicked() {
                    self.selected_tickets = selectable.iter().take(10).copied().collect();
                }
                if ui.button("Clear selection").clicked() {
                    self.selected_tickets.clear();
                }
                ui.label(format!("{selected_count} selected"));
            });
            for ticket in &self.tickets {
                ui.horizontal_wrapped(|ui| {
                    let was_seen = seen.contains(&ticket.number);
                    let can_select = !self.skip_seen_tickets || !was_seen;
                    let mut selected = self.selected_tickets.contains(&ticket.number);
                    if ui
                        .add_enabled(can_select, egui::Checkbox::new(&mut selected, ""))
                        .changed()
                    {
                        if selected {
                            self.selected_tickets.insert(ticket.number);
                        } else {
                            self.selected_tickets.remove(&ticket.number);
                        }
                    }
                    ui.hyperlink_to(
                        format!("#{} · {}", ticket.number, ticket.title),
                        &ticket.url,
                    );
                    ui.small(format!(
                        "{} comment(s) · updated {}",
                        ticket.comments, ticket.updated_at
                    ));
                    if was_seen {
                        ui.weak("already in GPUFold history");
                    }
                    if !ticket.labels.is_empty() {
                        ui.weak(ticket.labels.join(" · "));
                    }
                });
            }
            if selected_count > 10 {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "Process up to 10 tickets per batch to keep local storage manageable.",
                );
            }
            let queue_is_ready = selected_count > 0
                && selected_count <= 10
                && !self.settings.endpoint.trim().is_empty();
            if ui
                .add_enabled(
                    queue_is_ready,
                    egui::Button::new(format!(
                        "Generate proposals for {} selected ticket(s)",
                        selected_count
                    )),
                )
                .clicked()
            {
                let url = self.url.clone();
                let settings = self.settings.clone();
                let selected = self
                    .tickets
                    .iter()
                    .filter(|ticket| {
                        self.selected_tickets.contains(&ticket.number)
                            && selectable.contains(&ticket.number)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                self.queue_results.clear();
                self.progress_job(move |engine, cancel, progress| {
                    let _ = progress.send("Checking the configured local model".into());
                    repair_core::model::health(&settings)
                        .context("Connect and check your selected local model before processing tickets")?;
                    let _ = progress.send("Pinning the repository snapshot for this queue".into());
                    let (owner, repo) = github::parse_repo(&url)?;
                    let metadata = github::public_repo(&owner, &repo)?;
                    let base = metadata["default_branch"]
                        .as_str()
                        .context("Repository has no default branch")?;
                    let pinned_commit = github::pinned(&owner, &repo, base)?;
                    let mut results = Vec::with_capacity(selected.len());
                    for (index, ticket) in selected.iter().enumerate() {
                        if let Err(error) = process::check(&cancel) {
                            results.push(QueueItemResult {
                                ticket: ticket.clone(),
                                run: None,
                                error: Some(error.to_string()),
                            });
                            break;
                        }
                        let _ = progress.send(format!(
                            "Ticket {}/{} · preparing #{} {}",
                            index + 1,
                            selected.len(),
                            ticket.number,
                            ticket.title
                        ));
                        match engine.create_queued_issue(
                            &url,
                            ticket.number,
                            &pinned_commit,
                            &cancel,
                        ) {
                            Err(error) => results.push(QueueItemResult {
                                ticket: ticket.clone(),
                                run: None,
                                error: Some(format!("Could not prepare ticket: {error:#}")),
                            }),
                            Ok(mut run) => {
                                if let Err(error) = github::require_unassigned(
                                    &run.owner,
                                    &run.repo,
                                    ticket.number,
                                ) {
                                    run.notes.push(format!("Queue skipped ticket: {error}"));
                                    engine.transition(&mut run, State::Cancelled)?;
                                    results.push(QueueItemResult {
                                        ticket: ticket.clone(),
                                        run: Some(run),
                                        error: Some(error.to_string()),
                                    });
                                    continue;
                                }
                                let _ = progress.send(format!(
                                    "Ticket {}/{} · running the local model for #{}",
                                    index + 1,
                                    selected.len(),
                                    ticket.number
                                ));
                                match engine.analyze(&mut run, &settings, &cancel) {
                                    Ok(()) => results.push(QueueItemResult {
                                        ticket: ticket.clone(),
                                        run: Some(run),
                                        error: None,
                                    }),
                                    Err(error) => {
                                        results.push(QueueItemResult {
                                            ticket: ticket.clone(),
                                            run: Some(run),
                                            error: Some(format!("Model analysis stopped: {error:#}")),
                                        });
                                        for remaining in selected.iter().skip(index + 1) {
                                            results.push(QueueItemResult {
                                                ticket: remaining.clone(),
                                                run: None,
                                                error: Some("Not started because the model analysis stopped.".into()),
                                            });
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Ok(JobResult::Queue(results))
                });
            }
            ui.small("Tickets are analyzed sequentially. GPUFold does not run repository scripts, apply patches, or publish pull requests during queue processing; review and verify each proposal first.");
        }

        ui.separator();
        ui.heading("One-off task or bug sweep");
        ui.label("The repository commit is pinned before source is read.");
        ui.checkbox(&mut self.sweep, "Bug sweep (findings only; no patches)");
        if !self.sweep {
            ui.horizontal(|ui| {
                ui.label("Open issue number (optional)");
                ui.add(egui::TextEdit::singleline(&mut self.issue).desired_width(120.0));
            });
        }
        ui.label("Problem or scan focus");
        ui.add(
            egui::TextEdit::multiline(&mut self.task)
                .desired_rows(5)
                .desired_width(f32::INFINITY),
        );
        let issue_is_valid = self.sweep
            || self.issue.trim().is_empty()
            || self
                .issue
                .trim()
                .parse::<u64>()
                .is_ok_and(|number| number > 0);
        let task_is_valid = !self.task.trim().is_empty();
        if !self.sweep && !issue_is_valid {
            ui.colored_label(
                egui::Color32::RED,
                "Issue number must be a positive whole number.",
            );
        }
        if !task_is_valid {
            ui.small("Describe the bug, or give the sweep a specific focus.");
        }
        if ui
            .add_enabled(
                repository_is_valid && issue_is_valid && task_is_valid,
                egui::Button::new("Pin repository and inspect"),
            )
            .clicked()
        {
            let url = self.url.clone();
            let task = self.task.clone();
            let mode = if self.sweep { Mode::Sweep } else { Mode::Issue };
            match if self.issue.trim().is_empty() || self.sweep {
                Ok(None)
            } else {
                self.issue
                    .trim()
                    .parse::<u64>()
                    .ok()
                    .filter(|number| *number > 0)
                    .map(Some)
                    .ok_or(())
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
        if r.state == State::Review && r.evidence.level == 3 && r.owner != "LOCAL_FIXTURE" {
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
        let approved_command = runtime_approval_key(&self.settings);
        let mut approved = self.runtime_approval.as_deref() == Some(&approved_command);
        if ui
            .checkbox(
                &mut approved,
                "I approve this exact local runtime command and selected model",
            )
            .changed()
        {
            self.runtime_approval = approved.then(|| approved_command.clone());
        }
        ui.collapsing("Review launch command", |ui| {
            ui.monospace(format!(
                "{:?} serve --host 127.0.0.1 --port <port from endpoint> -m {:?} --max-seq-len {}{}",
                self.settings.runtime_path,
                self.settings.model_path,
                self.settings.context_tokens + self.settings.max_output_tokens + 512,
                if self.settings.cpu { " --cpu" } else { "" }
            ));
            ui.small(format!("The endpoint currently configured is {}", self.settings.endpoint));
        });
        if ui
            .add_enabled(approved, egui::Button::new("Load local model"))
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
            let path = PathBuf::from(&self.mcp_path);
            match mcp::import(&path) {
                Ok(config) => match (path.canonicalize(), std::fs::read(&path)) {
                    (Ok(canonical), Ok(contents)) => {
                        self.mcp_name = config.servers.keys().next().cloned().unwrap_or_default();
                        self.mcp_config = Some(config);
                        self.mcp_config_path = Some(canonical);
                        self.mcp_config_hash = Some(repair_core::validation::digest(&contents));
                        self.tools.clear();
                        self.mcp_consent = false;
                        self.tool_pinned = false;
                        self.message = "Configuration imported; original file not changed".into();
                    }
                    (Err(error), _) | (_, Err(error)) => self.message = error.to_string(),
                },
                Err(e) => self.message = e.to_string(),
            }
        }
        if let (Some(imported_path), Some(imported_hash)) =
            (&self.mcp_config_path, &self.mcp_config_hash)
        {
            if !mcp_config_is_current(
                PathBuf::from(&self.mcp_path).as_path(),
                imported_path,
                imported_hash,
            ) {
                ui.colored_label(
                    egui::Color32::from_rgb(173, 92, 36),
                    "This file changed after import. Reimport it before reviewing or launching a server.",
                );
                return;
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
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("GPUFold");
                ui.separator();
                for (label, page) in [
                    ("Home", Page::Home),
                    ("Repository", Page::Task),
                    ("Queue", Page::Queue),
                    ("Review", Page::Review),
                    ("History", Page::History),
                    ("Model & settings", Page::Settings),
                    ("MCP", Page::Mcp),
                ] {
                    ui.selectable_value(&mut self.page, page, label);
                }
                if self.pending.is_some() {
                    ui.spinner();
                    if ui.button("Stop").clicked() {
                        self.cancel.store(true, Ordering::SeqCst);
                        if let Ok(mut runtime) = self.engine.runtime.lock() {
                            runtime.unload();
                        }
                        self.message = "Stop requested. An already submitted local inference may finish shortly.".into();
                    }
                }
            });
        });
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
                    Page::Queue => self.queue_page(ui),
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
fn runtime_approval_key(settings: &Settings) -> String {
    let command = serde_json::to_vec(&(
        &settings.runtime_path,
        &settings.model_path,
        &settings.endpoint,
        settings.cpu,
        settings.context_tokens,
        settings.max_output_tokens,
    ))
    .unwrap_or_default();
    repair_core::validation::digest(&command)
}

fn tickets_match_repository(url: &str, scanned: Option<&(String, String)>) -> bool {
    let Some((owner, repo)) = github::parse_repo(url).ok() else {
        return false;
    };
    scanned.is_some_and(|(ticket_owner, ticket_repo)| {
        owner.eq_ignore_ascii_case(ticket_owner) && repo.eq_ignore_ascii_case(ticket_repo)
    })
}

fn sandbox_script() -> Option<PathBuf> {
    let installed = PathBuf::from("/usr/share/rust-repair/scripts/build-sandbox.sh");
    if installed.is_file() {
        return Some(installed);
    }
    let development =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/build-sandbox.sh");
    development.is_file().then_some(development)
}

fn mcp_config_is_current(
    path: &std::path::Path,
    imported_path: &std::path::Path,
    imported_hash: &str,
) -> bool {
    if path.canonicalize().ok().as_deref() != Some(imported_path) {
        return false;
    }
    std::fs::read(path)
        .ok()
        .is_some_and(|contents| repair_core::validation::digest(&contents) == imported_hash)
}

fn main() -> eframe::Result<()> {
    let store = Store::open(store::default_root()).unwrap_or_else(|e| {
        eprintln!("Cannot open local store: {e}");
        std::process::exit(1)
    });
    let engine = Engine::new(store);
    eframe::run_native(
        "GPUFold",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1100.0, 800.0])
                .with_min_inner_size([760.0, 560.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut style = (*cc.egui_ctx.style()).clone();
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 7.0);
            style.spacing.interact_size.y = 30.0;
            let mut visuals = egui::Visuals::light();
            visuals.panel_fill = egui::Color32::from_rgb(250, 251, 253);
            visuals.window_fill = egui::Color32::from_rgb(250, 251, 253);
            visuals.faint_bg_color = egui::Color32::from_rgb(243, 245, 249);
            visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(250, 251, 253);
            visuals.widgets.noninteractive.bg_stroke.color = egui::Color32::from_rgb(222, 227, 236);
            visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(239, 243, 250);
            visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(239, 243, 250);
            visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);
            visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(222, 232, 249);
            visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(222, 232, 249);
            visuals.widgets.active.bg_fill = egui::Color32::from_rgb(57, 82, 137);
            visuals.widgets.active.weak_bg_fill = egui::Color32::from_rgb(57, 82, 137);
            visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;
            visuals.selection.bg_fill = egui::Color32::from_rgb(57, 82, 137);
            visuals.selection.stroke.color = egui::Color32::WHITE;
            visuals.hyperlink_color = egui::Color32::from_rgb(47, 86, 151);
            visuals.window_rounding = egui::Rounding::same(10.0);
            style.visuals = visuals;
            cc.egui_ctx.set_style(style);
            Ok(Box::new(App::new(engine)))
        }),
    )
}

#[cfg(test)]
mod ui_tests {
    use super::{
        mcp_config_is_current, runtime_approval_key, sandbox_script,
        tickets_match_repository,
    };
    use repair_core::types::Settings;

    #[test]
    fn changing_a_managed_launch_argument_requires_fresh_approval() {
        let original = Settings::default();
        let approved = runtime_approval_key(&original);

        let mut changed = original.clone();
        changed.runtime_path = "/opt/mistralrs".into();
        assert_ne!(runtime_approval_key(&changed), approved);

        let mut changed = original.clone();
        changed.model_path = "/models/coder".into();
        assert_ne!(runtime_approval_key(&changed), approved);

        let mut changed = original.clone();
        changed.endpoint = "http://127.0.0.1:1235/v1/chat/completions".into();
        assert_ne!(runtime_approval_key(&changed), approved);

        let mut changed = original.clone();
        changed.cpu = true;
        assert_ne!(runtime_approval_key(&changed), approved);

        let mut changed = original.clone();
        changed.context_tokens += 512;
        assert_ne!(runtime_approval_key(&changed), approved);

        let mut changed = original;
        changed.max_output_tokens += 512;
        assert_ne!(runtime_approval_key(&changed), approved);
    }

    #[test]
    fn sandbox_setup_script_is_available_for_a_source_checkout() {
        assert!(sandbox_script().is_some());
    }

    #[test]
    fn tickets_are_bound_to_the_repository_that_was_scanned() {
        let scanned = ("example".to_owned(), "project".to_owned());
        assert!(tickets_match_repository(
            "https://github.com/Example/Project",
            Some(&scanned)
        ));
        assert!(!tickets_match_repository(
            "https://github.com/other/project",
            Some(&scanned)
        ));
        assert!(!tickets_match_repository("invalid", Some(&scanned)));
    }

    #[test]
    fn changed_mcp_file_must_be_reimported_before_server_launch() {
        let root = std::env::temp_dir().join(format!(
            "rust-repair-mcp-ui-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("mcp.json");
        std::fs::write(&path, br#"{"mcpServers":{}}"#).unwrap();
        let canonical = path.canonicalize().unwrap();
        let approved_hash = repair_core::validation::digest(&std::fs::read(&path).unwrap());
        assert!(mcp_config_is_current(&path, &canonical, &approved_hash));

        std::fs::write(&path, br#"{"mcpServers":{"changed":{}}}"#).unwrap();
        assert!(!mcp_config_is_current(&path, &canonical, &approved_hash));
        let _ = std::fs::remove_dir_all(root);
    }
}
