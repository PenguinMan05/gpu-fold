use crate::{
    github, model, process, repository,
    sandbox::{self, SandboxRunner},
    store::Store,
    types::*,
    validation,
};
use anyhow::{ensure, Context, Result};
use std::path::Path;

struct RunLock(std::fs::File);
impl Drop for RunLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

#[derive(Clone)]
pub struct Engine {
    pub store: Store,
    pub runtime: std::sync::Arc<std::sync::Mutex<crate::runtime::ManagedRuntime>>,
    pub activity: std::sync::Arc<std::sync::Mutex<String>>,
    pub model_output: std::sync::Arc<std::sync::Mutex<String>>,
}
impl Engine {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            runtime: Default::default(),
            activity: Default::default(),
            model_output: Default::default(),
        }
    }
    pub fn report_activity(&self, message: &str) {
        if let Ok(mut log) = self.activity.lock() {
            log.push_str(&process::scrub(message));
            log.push('\n');
            if log.len() > 64000 {
                let mut start = log.len() - 64000;
                while !log.is_char_boundary(start) { start += 1; }
                log.drain(..start);
            }
        }
    }
    pub fn clear_activity(&self) {
        if let Ok(mut log) = self.activity.lock() { log.clear(); }
        if let Ok(mut output) = self.model_output.lock() { output.clear(); }
    }
    fn lock(&self, r: &Run) -> Result<RunLock> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(r.root.join("run.lock"))?;
        fs2::FileExt::try_lock_exclusive(&file)
            .context("This run is already active in another process")?;
        let current = self.store.get(&r.id)?;
        ensure!(
            serde_json::to_string(&current)? == serde_json::to_string(r)?,
            "Run changed in another process; reload it from History"
        );
        Ok(RunLock(file))
    }
    pub fn create(
        &self,
        url: &str,
        task: &str,
        mode: Mode,
        issue: Option<u64>,
        cancel: &process::Cancel,
    ) -> Result<Run> {
        let r = repository::create(&self.store.root, url, task, mode, issue, cancel)?;
        self.store.save(&r)?;
        Ok(r)
    }
    pub fn create_queued_issue(
        &self,
        url: &str,
        issue: u64,
        pinned_commit: &str,
        cancel: &process::Cancel,
    ) -> Result<Run> {
        let r =
            repository::create_queued_issue(&self.store.root, url, issue, pinned_commit, cancel)?;
        self.store.save(&r)?;
        Ok(r)
    }
    pub fn fixture(&self) -> Result<Run> {
        let id = format!(
            "fixture{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let root = self.store.root.join("runs").join(&id);
        std::fs::create_dir_all(root.join("pristine/src"))?;
        std::fs::write(
            root.join("pristine/Cargo.toml"),
            include_str!("../../../fixtures/fixture-add/Cargo.toml"),
        )?;
        std::fs::write(
            root.join("pristine/Cargo.lock"),
            include_str!("../../../fixtures/fixture-add/Cargo.lock"),
        )?;
        std::fs::write(
            root.join("pristine/src/lib.rs"),
            include_str!("../../../fixtures/fixture-add/src/lib.rs"),
        )?;
        repository::copy_tree(&root.join("pristine"), &root.join("work"))?;
        let preflight = repository::scan(&root.join("pristine"))?;
        let r = Run {
            id,
            created: now(),
            updated: now(),
            owner: "LOCAL_FIXTURE".into(),
            repo: "fixture-add".into(),
            source: "fixture://fixture-add (publishing disabled)".into(),
            commit: validation::digest(
                include_str!("../../../fixtures/fixture-add/src/lib.rs").as_bytes(),
            ),
            base: "fixture".into(),
            mode: Mode::Issue,
            task: "add(5, 2) incorrectly returns 3 rather than 7".into(),
            state: State::Pinned,
            issue_number: None,
            root,
            preflight,
            proposal: None,
            evidence: Evidence::default(),
            model_id: String::new(),
            model_hash: String::new(),
            prompts: vec![],
            model_outputs: vec![],
            attempts: 0,
            context_paths: vec![],
            notes: vec![
                "Trusted synthetic benchmark; not a real-world success-rate evaluation".into(),
            ],
            pr_url: None,
            mcp_approvals: vec![],
            tool_results: vec![],
        };
        self.store.save(&r)?;
        Ok(r)
    }
    pub fn transition(&self, r: &mut Run, state: State) -> Result<()> {
        r.state = state;
        r.updated = now();
        self.store.save(r)
    }
    pub fn analyze(&self, r: &mut Run, s: &Settings, cancel: &process::Cancel) -> Result<()> {
        self.analyze_inner(r, s, cancel, true)
    }
    /// A queue turn: one proposal attempt, so a difficult ticket cannot starve others.
    pub fn analyze_once(&self, r: &mut Run, s: &Settings, cancel: &process::Cancel) -> Result<()> {
        self.analyze_inner(r, s, cancel, false)
    }
    fn analyze_inner(&self, r: &mut Run, s: &Settings, cancel: &process::Cancel, keep_retrying: bool) -> Result<()> {
        let _lock = self.lock(r)?;
        ensure!(
            matches!(
                r.state,
                State::Pinned
                    | State::AwaitPlan
                    | State::AwaitTools
                    | State::NoFix
                    | State::Cancelled
                    | State::Review
            ),
            "Analyze only an unpatched run"
        );
        ensure!(
            r.state != State::Review || r.evidence.level < 3,
            "A verified patch must be reviewed, not overwritten"
        );
        if r.proposal.is_some() {
            let work = r.root.join("work");
            if work.exists() {
                std::fs::remove_dir_all(&work)?;
            }
            repository::copy_tree(&r.root.join("pristine"), &work)?;
        }
        self.transition(r, State::Analyzing)?;
        self.report_activity(&format!("Working on {}/{}{}", r.owner, r.repo, r.issue_number.map(|n| format!(" #{n}")).unwrap_or_default()));
        let mut settings = s.clone();
        let mut feedback = r.notes.iter().rev().find(|note| note.contains("needs revision:")).cloned().unwrap_or_default();
        if feedback.contains("truncat") || feedback.contains("length") {
            let total = s.context_tokens.saturating_add(s.max_output_tokens);
            settings.max_output_tokens = settings.max_output_tokens.saturating_mul(2).min(8192).min(total.saturating_sub(1024)).max(s.max_output_tokens);
            settings.context_tokens = total.saturating_sub(settings.max_output_tokens);
        }
        let mut failures = 0usize;
        let outcome = loop {
            if let Err(error) = process::check(cancel) { break Err(error); }
            if let Ok(mut output) = self.model_output.lock() { output.clear(); }
            let attempt = model::propose_attempt(r, &settings, cancel, &mut |chunk| {
                if let Ok(mut output) = self.model_output.lock() {
                    if output.len() + chunk.len() <= 64000 { output.push_str(chunk); }
                }
            }, &mut |status| self.report_activity(status), &feedback);
            let attempt = match attempt {
                Ok(proposal) if proposal.no_fix && r.mode == Mode::Issue => {
                    self.report_activity(&format!("Model summary: {}", model::clip_text(&process::scrub(&proposal.explanation), 1000)));
                    Err(anyhow::Error::new(model::RetryableProposalError("No supported repair yet. Inspect another source window, reconsider the issue, and either propose a supported repair or accurately explain the remaining obstacle. Never invent a fix.".into())))
                }
                other => other,
            };
            match attempt {
                Ok(proposal) => break Ok(proposal),
                Err(error) => {
                    if process::check(cancel).is_err() { break Err(anyhow::anyhow!("Analysis stopped by you; saved attempts are in History")); }
                    if error.downcast_ref::<model::RetryableProposalError>().is_none() { break Err(error); }
                    failures = failures.saturating_add(1);
                    feedback = format!("The last attempt failed validation: {error:#}. Correct this specific problem. Include every required field, especially explanation. Use exact source text and keep the reply concise.");
                    r.notes.push(format!("Attempt {} needs revision: {error:#}", r.attempts));
                    if r.notes.len() > 64 { r.notes.drain(..r.notes.len()-64); }
                    r.updated = now();
                    self.store.save(r)?;
                    let message = format!("Attempt {} needs revision: {error:#}", r.attempts);
                    self.report_activity(&message);
                    let lower = message.to_ascii_lowercase();
                    if lower.contains("truncat") || lower.contains("length") {
                        let total = s.context_tokens.saturating_add(s.max_output_tokens);
                        settings.max_output_tokens = (settings.max_output_tokens.saturating_mul(2)).min(8192).min(total.saturating_sub(1024)).max(s.max_output_tokens);
                        settings.context_tokens = total.saturating_sub(settings.max_output_tokens);
                        self.report_activity(&format!("Rebalanced the existing model window: {} input / {} output tokens", settings.context_tokens, settings.max_output_tokens));
                    } else if lower.contains("context") || lower.contains("too long") {
                        settings.context_chars = (settings.context_chars * 2 / 3).max(256);
                        self.report_activity("Reducing source context automatically before retrying");
                    }
                    if !keep_retrying { break Err(error); }
                    let delay = failures.min(8);
                    self.report_activity(&format!("Retrying automatically in {delay}s. Stop current task is always available."));
                    for _ in 0..delay * 10 {
                        if process::check(cancel).is_err() { break; }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
        };
        match outcome {
            Ok(p) => {
                self.report_activity(&format!("Model summary: {}", model::clip_text(&process::scrub(&p.explanation), 1500)));
                self.report_activity("Proposal passed source and scope checks. Ready for your review; tests have not run yet.");
                r.evidence = Evidence::default();
                r.evidence.level = if p.no_fix || !p.tool_requests.is_empty() {
                    0
                } else {
                    1
                };
                r.evidence.reason = if p.no_fix {
                    "Model reported no supported fix"
                } else {
                    "App verified source citations; model claim untested"
                }
                .into();
                let tools_pending = !p.tool_requests.is_empty();
                let no_fix = p.no_fix;
                r.proposal = Some(p);
                self.transition(
                    r,
                    if no_fix {
                        State::NoFix
                    } else if tools_pending {
                        State::AwaitTools
                    } else {
                        State::AwaitPlan
                    },
                )
            }
            Err(e) => {
                if e.downcast_ref::<model::RetryableProposalError>().is_some() {
                    self.report_activity("Attempt saved. This ticket will get another turn after the other queued tickets.");
                } else { self.report_activity(&format!("Analysis paused: {e:#}")); }
                r.notes.push(e.to_string());
                self.transition(r, State::Cancelled)?;
                Err(e)
            }
        }
    }
    pub fn approve_mcp(
        &self,
        r: &mut Run,
        path: &Path,
        server: &str,
        pin: &crate::mcp::ToolPin,
        read_only: bool,
    ) -> Result<()> {
        let _lock = self.lock(r)?;
        ensure!(
            !matches!(
                r.state,
                State::Publishing | State::Published | State::Patched | State::Verifying
            ),
            "Configure tools on an unpatched run"
        );
        let config = crate::mcp::import(path)?;
        let srv = config
            .servers
            .get(server)
            .context("MCP server disappeared")?;
        ensure!(
            srv.url.is_none(),
            "Remote MCP requires a separate egress adapter"
        );
        let approval = McpApproval {
            config_path: path.canonicalize()?,
            server: server.into(),
            server_hash: validation::digest(&serde_json::to_vec(srv)?),
            pin: pin.clone(),
            read_only,
            enabled: true,
        };
        r.mcp_approvals
            .retain(|a| a.server != server || a.pin.name != pin.name);
        r.mcp_approvals.push(approval);
        self.store.save(r)
    }
    pub fn requested_tool(
        &self,
        r: &mut Run,
        index: usize,
        approve_call: bool,
        cancel: &process::Cancel,
    ) -> Result<()> {
        let _lock = self.lock(r)?;
        ensure!(
            r.state == State::AwaitTools,
            "No model tool request awaiting review"
        );
        ensure!(r.tool_results.len() < 10, "Run tool-call budget exhausted");
        let request = r
            .proposal
            .as_ref()
            .context("No model proposal")?
            .tool_requests
            .get(index)
            .context("Tool request missing")?
            .clone();
        let approval = r
            .mcp_approvals
            .iter()
            .find(|a| a.enabled && a.server == request.server && a.pin.name == request.name)
            .context("Tool is not approved")?
            .clone();
        ensure!(
            approval.read_only || approve_call,
            "Approve the exact tool payload and its possible effects"
        );
        let config = crate::mcp::import(&approval.config_path)?;
        let server = config
            .servers
            .get(&approval.server)
            .context("Server disappeared")?;
        ensure!(
            validation::digest(&serde_json::to_vec(server)?) == approval.server_hash,
            "Server configuration changed; approve it again"
        );
        crate::mcp::workspace_args(
            &serde_json::Value::Object(request.arguments.clone()),
            &r.root.join("work"),
        )?;
        let output = crate::mcp::call_cancel(
            server,
            &r.root.join("work"),
            &approval.pin,
            request.arguments.clone(),
            true,
            cancel,
        )?;
        r.tool_results.push(format!(
            "BEGIN UNTRUSTED MCP RESULT {} / {}\nArguments: {}\n{}\nEND UNTRUSTED MCP RESULT",
            request.server,
            request.name,
            process::scrub(&serde_json::to_string(&request.arguments)?),
            output
        ));
        if let Some(p) = &mut r.proposal {
            p.tool_requests.remove(index);
        }
        if r.proposal
            .as_ref()
            .is_some_and(|p| p.tool_requests.is_empty())
        {
            r.proposal = None;
            self.transition(r, State::Pinned)
        } else {
            self.store.save(r)
        }
    }
    pub fn reject(&self, r: &mut Run) -> Result<()> {
        let _lock = self.lock(r)?;
        ensure!(
            !matches!(r.state, State::Publishing | State::Published),
            "Cannot reject a published or publishing run"
        );
        r.notes
            .push("User rejected the proposal; worktree reset to pinned source".into());
        let work = r.root.join("work");
        if work.exists() {
            std::fs::remove_dir_all(&work)?;
        }
        repository::copy_tree(&r.root.join("pristine"), &work)?;
        r.proposal = None;
        r.evidence = Evidence::default();
        self.transition(r, State::NoFix)
    }
    pub fn replace_proposal(&self, r: &mut Run, p: Proposal) -> Result<()> {
        let _lock = self.lock(r)?;
        ensure!(
            !matches!(r.state, State::Publishing | State::Published),
            "Cannot edit published proposal"
        );
        validation::validate_proposal(&r.root.join("pristine"), &p)?;
        ensure!(
            r.mode == Mode::Issue || (p.edits.is_empty() && p.reproduction.is_none()),
            "Sweep findings only"
        );
        let work = r.root.join("work");
        if work.exists() {
            std::fs::remove_dir_all(&work)?;
        }
        repository::copy_tree(&r.root.join("pristine"), &work)?;
        let no_fix = p.no_fix;
        let tools_pending = !p.tool_requests.is_empty();
        r.proposal = Some(p);
        r.evidence = Evidence {
            level: if no_fix || tools_pending { 0 } else { 1 },
            reason: if no_fix {
                "No fix proposed"
            } else {
                "User-edited proposal; previous test evidence invalidated"
            }
            .into(),
            ..Default::default()
        };
        r.notes
            .push("User edited proposal; prior evidence invalidated".into());
        self.transition(
            r,
            if no_fix {
                State::NoFix
            } else if tools_pending {
                State::AwaitTools
            } else {
                State::AwaitPlan
            },
        )
    }
    pub fn approve_plan(&self, r: &mut Run) -> Result<()> {
        let _lock = self.lock(r)?;
        ensure!(r.state == State::AwaitPlan, "No plan awaiting approval");
        if r.mode == Mode::Sweep {
            self.transition(r, State::Review)
        } else {
            self.transition(r, State::AwaitPatch)
        }
    }
    pub fn apply_patch(&self, r: &mut Run) -> Result<()> {
        let _lock = self.lock(r)?;
        self.recover_inner(r)?;
        ensure!(
            r.state == State::AwaitPatch && r.mode == Mode::Issue,
            "Approve a specific issue plan before applying a patch"
        );
        let p = r.proposal.as_ref().context("No proposal")?;
        ensure!(!p.edits.is_empty(), "No production fix was proposed");
        let mut untouched = p.clone();
        untouched.edits.clear();
        untouched.reproduction = None;
        validation::verify_work(&r.root.join("work"), &r.root.join("pristine"), &untouched)?;
        let stage = r.root.join("patch-stage");
        let previous = r.root.join("work-previous");
        ensure!(
            !stage.exists() && !previous.exists(),
            "Interrupted patch transaction found; inspect saved workspace before proceeding"
        );
        repository::copy_tree(&r.root.join("pristine"), &stage)?;
        validation::apply(&stage, &r.root.join("pristine"), p)?;
        std::fs::rename(r.root.join("work"), &previous)?;
        if let Err(error) = std::fs::rename(&stage, r.root.join("work")) {
            let _ = std::fs::rename(&previous, r.root.join("work"));
            return Err(error.into());
        }
        std::fs::remove_dir_all(previous)?;
        self.transition(r, State::Patched)
    }
    pub fn baseline(
        &self,
        r: &mut Run,
        s: &Settings,
        approved: bool,
        cancel: &process::Cancel,
    ) -> Result<()> {
        let _lock = self.lock(r)?;
        ensure!(
            approved,
            "Approve dependency downloads and sandbox execution"
        );
        ensure!(
            r.state == State::AwaitPatch && r.mode == Mode::Issue,
            "Approve the plan before reproducing the bug"
        );
        let p = r.proposal.clone().context("No proposal")?;
        validation::validate_proposal(&r.root.join("pristine"), &p)?;
        let repro = p
            .reproduction
            .context("A new reproduction test is required")?;
        sandbox::check_runtime(cancel)?;
        let image = sandbox::image_id(&s.image, cancel)?;
        let vendor = r.root.join("vendor");
        sandbox::fetch_vendor(&r.root.join("pristine"), &vendor, cancel)?;
        let path = r.root.join("baseline");
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        sandbox::sanitized_copy(&r.root.join("pristine"), &path)?;
        if let Some(parent) = path.join(&repro.path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path.join(&repro.path), &repro.content)?;
        r.evidence = Evidence::default();
        self.store.save(r)?;
        let runner = sandbox::PodmanRunner;
        for _ in 0..2 {
            process::check(cancel)?;
            let observation = runner.test(&path, &vendor, &image, cancel)?;
            r.evidence.baseline.push(observation);
            self.store.save(r)?;
        }
        r.evidence = sandbox::evaluate_baseline(&repro, r.evidence.baseline.clone());
        r.notes.push(format!(
            "Baseline ran before applying the production patch; sandbox {image}"
        ));
        self.store.save(r)
    }
    pub fn verify(
        &self,
        r: &mut Run,
        s: &Settings,
        approve_fetch_and_execution: bool,
        cancel: &process::Cancel,
    ) -> Result<()> {
        let _lock = self.lock(r)?;
        ensure!(
            approve_fetch_and_execution,
            "Explicit download and execution approval required"
        );
        ensure!(
            matches!(r.state, State::Patched | State::Review | State::Cancelled)
                && r.mode == Mode::Issue,
            "No patched issue run"
        );
        let p = r.proposal.clone().context("No patch")?;
        validation::verify_work(&r.root.join("work"), &r.root.join("pristine"), &p)?;
        let repro = p
            .reproduction
            .clone()
            .context("No reproduction test proposed; evidence stays at level 1")?;
        self.transition(r, State::Verifying)?;
        let result = (|| -> Result<()> {
            sandbox::check_runtime(cancel)?;
            let image = sandbox::image_id(&s.image, cancel)?;
            r.notes.push(format!("Sandbox image: {image}. Cargo configs and toolchain selectors excluded from execution copy."));
            let vendor = r.root.join("vendor");
            sandbox::fetch_vendor(&r.root.join("pristine"), &vendor, cancel)?;
            for dir in ["baseline", "after"] {
                let path = r.root.join(dir);
                if path.exists() {
                    std::fs::remove_dir_all(&path)?;
                }
                let source = if dir == "baseline" {
                    r.root.join("pristine")
                } else {
                    r.root.join("work")
                };
                sandbox::sanitized_copy(&source, &path)?;
                if dir == "baseline" {
                    std::fs::create_dir_all(path.join("tests"))?;
                    std::fs::write(path.join(&repro.path), &repro.content)?;
                }
            }
            let runner = sandbox::PodmanRunner;
            let mut baseline = vec![];
            let mut after = vec![];
            for _ in 0..2 {
                process::check(cancel)?;
                baseline.push(runner.test(&r.root.join("baseline"), &vendor, &image, cancel)?);
                r.evidence.baseline = baseline.clone();
                self.store.save(r)?;
            }
            for _ in 0..2 {
                process::check(cancel)?;
                after.push(runner.test(&r.root.join("after"), &vendor, &image, cancel)?);
                r.evidence.after = after.clone();
                self.store.save(r)?;
            }
            r.evidence = sandbox::evaluate(&repro, baseline, after);
            Ok(())
        })();
        match result {
            Ok(()) => self.transition(r, State::Review),
            Err(e) => {
                r.evidence.level = 1;
                r.evidence.reason = format!("Verification interrupted: {e}");
                r.notes.push(e.to_string());
                self.transition(r, State::Cancelled)?;
                Err(e)
            }
        }
    }
    pub fn publish(
        &self,
        r: &mut Run,
        a: &PublishApproval,
        s: &Settings,
        cancel: &process::Cancel,
    ) -> Result<()> {
        let _lock = self.lock(r)?;
        github::check_publish(r, a)?;
        let history = self.store.list()?;
        ensure!(
            history
                .iter()
                .filter(|old| old.pr_url.is_some() && old.updated > now().saturating_sub(86400))
                .count()
                < 3,
            "Daily publication limit reached"
        );
        for old in history
            .iter()
            .filter(|old| old.owner == r.owner && old.repo == r.repo)
        {
            if let Some(url) = &old.pr_url {
                ensure!(
                    !github::recorded_pr_open(url)?,
                    "An open app pull request already exists for this repository"
                );
            }
        }

        // Save intent before remote effects. A failed or interrupted publish is not automatically retried.
        self.transition(r, State::Publishing)?;
        let mut candidate = r.clone();
        candidate.state = State::Review;
        match github::publish(&candidate, a, s, cancel) {
            Ok(url) => {
                r.pr_url = Some(url);
                self.transition(r, State::Published)
            }
            Err(e) => {
                r.notes.push(format!("Publishing stopped: {e}. Inspect GitHub and the publish workspace before another attempt."));
                self.store.save(r)?;
                Err(e)
            }
        }
    }
    pub fn delete_workspace(&self, r: &mut Run) -> Result<()> {
        let _lock = self.lock(r)?;
        ensure!(
            !matches!(
                r.state,
                State::Analyzing | State::Verifying | State::Publishing
            ),
            "Stop active work before deleting its workspace"
        );
        ensure!(
            r.root.canonicalize()?.parent()
                == Some(self.store.root.join("runs").canonicalize()?.as_path()),
            "Workspace outside the run directory"
        );
        std::fs::remove_dir_all(&r.root)?;
        r.notes
            .push("Workspace deleted by user; history and publication metadata retained".into());
        r.updated = now();
        self.store.save(r)
    }
    pub fn export(&self, r: &Run, destination: &Path) -> Result<()> {
        self.store.export(r, destination)
    }
    pub fn recover(&self, r: &mut Run) -> Result<()> {
        let _lock = self.lock(r)?;
        self.recover_inner(r)
    }
    fn recover_inner(&self, r: &mut Run) -> Result<()> {
        let previous = r.root.join("work-previous");
        if previous.exists() {
            let work = r.root.join("work");
            if work.exists() {
                std::fs::rename(
                    &work,
                    r.root.join(format!(
                        "work-interrupted-{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)?
                            .as_nanos()
                    )),
                )?;
            }
            std::fs::rename(previous, work)?;
            r.notes
                .push("Recovered original worktree after interrupted patch transaction".into());
            self.store.save(r)?;
        }
        let stage = r.root.join("patch-stage");
        if stage.exists() {
            std::fs::rename(
                stage,
                r.root.join(format!(
                    "patch-interrupted-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_nanos()
                )),
            )?;
        }
        match r.state {
            State::Analyzing | State::Verifying => {
                r.notes.push(
                    "Interrupted step detected; resume will restart this step from saved inputs"
                        .into(),
                );
                self.transition(r, State::Cancelled)
            }
            State::Publishing => {
                r.notes.push(
                    "Publishing was interrupted; check GitHub manually before proceeding".into(),
                );
                self.store.save(r)
            }
            _ => Ok(()),
        }
    }
}
