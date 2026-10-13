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
}
impl Engine {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            runtime: Default::default(),
            activity: Default::default(),
        }
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
        let _lock = self.lock(r)?;
        ensure!(
            matches!(
                r.state,
                State::Pinned | State::AwaitPlan | State::NoFix | State::Cancelled
            ),
            "Analyze only an unpatched run"
        );
        ensure!(r.attempts < 3, "Proposal budget exhausted");
        ensure!(r.prompts.len() < 3, "Prompt budget exhausted");
        self.transition(r, State::Analyzing)?;
        let activity = self.activity.clone();
        if let Ok(mut a) = activity.lock() {
            a.clear();
        }
        match model::propose_stream(r, s, cancel, &mut |chunk| {
            if let Ok(mut a) = activity.lock() {
                a.push_str(&process::scrub(chunk));
            }
        }) {
            Ok(p) => {
                r.evidence.level = if p.no_fix { 0 } else { 1 };
                r.evidence.reason = if p.no_fix {
                    "Model reported no supported fix"
                } else {
                    "App verified source citations; model claim untested"
                }
                .into();
                let no_fix = p.no_fix;
                r.proposal = Some(p);
                self.transition(
                    r,
                    if no_fix {
                        State::NoFix
                    } else {
                        State::AwaitPlan
                    },
                )
            }
            Err(e) => {
                r.notes.push(e.to_string());
                self.transition(r, State::Cancelled)?;
                Err(e)
            }
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
        r.proposal = Some(p);
        r.evidence = Evidence {
            level: 1,
            reason: "User-edited proposal; source citations verified, tests must run again".into(),
            ..Default::default()
        };
        r.notes
            .push("User edited proposal; prior evidence invalidated".into());
        self.transition(r, State::AwaitPlan)
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
        ensure!(!history.iter().any(|old|old.pr_url.is_some()&&old.owner==r.owner&&old.repo==r.repo),"A previously published app PR exists for this repository; close/review it before another (0.1.0 conservatively blocks repeats)");
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
