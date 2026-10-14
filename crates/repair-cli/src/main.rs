use anyhow::{ensure, Result};
use clap::{Parser, Subcommand};
use repair_core::{github, mcp, process, store, types::*, validation, Engine, Store};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "gpufold",
    version,
    about = "GPUFold local GitHub issue repair tools. Every execution and publication requires explicit consent."
)]
struct Args {
    #[arg(long)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    SandboxProbe,
    List,
    Fixture,
    New {
        url: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        issue: Option<u64>,
        #[arg(long)]
        sweep: bool,
    },
    Show {
        id: String,
    },
    Analyze {
        id: String,
        #[arg(long)]
        consent_local_inference: bool,
    },
    ImportProposal {
        id: String,
        file: PathBuf,
    },
    ApprovePlan {
        id: String,
    },
    Apply {
        id: String,
        #[arg(long)]
        reviewed_patch: bool,
    },
    Baseline {
        id: String,
        #[arg(long)]
        approve_downloads_and_code_execution: bool,
    },
    Test {
        id: String,
        #[arg(long)]
        approve_downloads_and_code_execution: bool,
    },
    Export {
        id: String,
        file: PathBuf,
    },
    Settings {
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        weights: Option<String>,
        #[arg(long)]
        image: Option<String>,
        #[arg(long)]
        oauth_client_id: Option<String>,
    },
    McpInspect {
        file: PathBuf,
        server: String,
        #[arg(long)]
        approve_unrestricted_local_process: bool,
    },
    Publish {
        id: String,
        approval_file: PathBuf,
        #[arg(long)]
        confirm_fork_push_draft_pr: bool,
    },
}
fn main() -> Result<()> {
    let a = Args::parse();
    let s = Store::open(a.data_dir.unwrap_or_else(store::default_root))?;
    let engine = Engine::new(s.clone());
    let cancel = process::cancel_token();
    let stop = cancel.clone();
    ctrlc::set_handler(move || stop.store(true, std::sync::atomic::Ordering::SeqCst))?;
    match a.command {
        Action::SandboxProbe => println!(
            "{}",
            repair_core::probe::sandbox_probe(&s.root, &s.settings()?, &cancel)?
        ),
        Action::Fixture => println!("{}", engine.fixture()?.id),
        Action::List => {
            for r in s.list()? {
                println!(
                    "{}\t{:?}\t{}\tlevel {}",
                    r.id, r.state, r.source, r.evidence.level
                )
            }
        }
        Action::New {
            url,
            task,
            issue,
            sweep,
        } => {
            let r = engine.create(
                &url,
                &task,
                if sweep { Mode::Sweep } else { Mode::Issue },
                issue,
                &cancel,
            )?;
            println!("{}\nPinned {}", r.id, r.commit)
        }
        Action::Show { id } => println!("{}", serde_json::to_string_pretty(&s.get(&id)?)?),
        Action::Analyze {
            id,
            consent_local_inference,
        } => {
            ensure!(
                consent_local_inference,
                "Add --consent-local-inference after reviewing model settings"
            );
            let mut r = s.get(&id)?;
            engine.recover(&mut r)?;
            engine.analyze(&mut r, &s.settings()?, &cancel)?;
            println!("{:?}: {}", r.state, r.evidence.reason)
        }
        Action::ImportProposal { id, file } => {
            let mut r = s.get(&id)?;
            ensure!(
                r.state == State::Pinned,
                "Only a fresh pinned run can import a benchmark proposal"
            );
            ensure!(
                std::fs::metadata(&file)?.len() <= 1_000_000,
                "Proposal too large"
            );
            let p: Proposal = serde_json::from_slice(&std::fs::read(file)?)?;
            validation::validate_proposal(&r.root.join("pristine"), &p)?;
            ensure!(
                r.mode == Mode::Issue || p.edits.is_empty(),
                "Sweep findings only"
            );
            engine.replace_proposal(&mut r, p)?;
            println!("Awaiting plan approval")
        }
        Action::ApprovePlan { id } => {
            let mut r = s.get(&id)?;
            engine.approve_plan(&mut r)?;
            println!("{:?}", r.state)
        }
        Action::Apply { id, reviewed_patch } => {
            ensure!(
                reviewed_patch,
                "Add --reviewed-patch after inspecting each replacement"
            );
            let mut r = s.get(&id)?;
            engine.apply_patch(&mut r)?;
            println!("Patched locally; nothing published")
        }
        Action::Baseline {
            id,
            approve_downloads_and_code_execution,
        } => {
            let mut r = s.get(&id)?;
            engine.baseline(
                &mut r,
                &s.settings()?,
                approve_downloads_and_code_execution,
                &cancel,
            )?;
            println!("Level {}: {}", r.evidence.level, r.evidence.reason)
        }
        Action::Test {
            id,
            approve_downloads_and_code_execution,
        } => {
            let mut r = s.get(&id)?;
            engine.recover(&mut r)?;
            engine.verify(
                &mut r,
                &s.settings()?,
                approve_downloads_and_code_execution,
                &cancel,
            )?;
            println!("Level {}: {}", r.evidence.level, r.evidence.reason)
        }
        Action::Export { id, file } => {
            engine.export(&s.get(&id)?, &file)?;
            println!("Exported local run record")
        }
        Action::Settings {
            endpoint,
            model,
            weights,
            image,
            oauth_client_id,
        } => {
            let mut st = s.settings()?;
            if let Some(v) = endpoint {
                st.endpoint = v
            }
            if let Some(v) = model {
                st.model_id = v
            }
            if let Some(v) = weights {
                st.weights_path = v
            }
            if let Some(v) = image {
                st.image = v
            }
            if let Some(v) = oauth_client_id {
                st.oauth_client_id = v
            }
            s.save_settings(&st)?;
            println!("{}", serde_json::to_string_pretty(&st)?)
        }
        Action::McpInspect {
            file,
            server,
            approve_unrestricted_local_process,
        } => {
            let c = mcp::import(&file)?;
            let srv = c
                .servers
                .get(&server)
                .ok_or_else(|| anyhow::anyhow!("Server not found"))?;
            let workspace = s.root.join("mcp-workspace");
            std::fs::create_dir_all(&workspace)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&mcp::inspect(
                    srv,
                    &workspace,
                    approve_unrestricted_local_process
                )?)?
            )
        }
        Action::Publish {
            id,
            approval_file,
            confirm_fork_push_draft_pr,
        } => {
            ensure!(
                confirm_fork_push_draft_pr,
                "Explicit final publishing consent required"
            );
            let mut r = s.get(&id)?;
            let approval: PublishApproval = serde_json::from_slice(&std::fs::read(approval_file)?)?;
            println!("{}", github::draft_body(&r));
            engine.publish(&mut r, &approval, &s.settings()?, &cancel)?;
            println!("{}", r.pr_url.unwrap_or_default())
        }
    }
    Ok(())
}
