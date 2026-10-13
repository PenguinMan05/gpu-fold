use crate::{github, process, types::*, validation};
use anyhow::{ensure, Context, Result};
use std::{collections::BTreeSet, fs, io::Read, path::Path, time::Duration};

pub fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for ent in walkdir::WalkDir::new(from) {
        let ent = ent?;
        ensure!(!ent.file_type().is_symlink(), "Symlink forbidden");
        let relative = ent.path().strip_prefix(from)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let dest = to.join(relative);
        if ent.file_type().is_dir() {
            fs::create_dir_all(dest)?;
        } else if ent.file_type().is_file() {
            fs::copy(ent.path(), dest)?;
        }
    }
    Ok(())
}
pub fn unpack<R: Read>(
    reader: R,
    dest: &Path,
    strip_prefix: bool,
    byte_limit: u64,
    file_limit: usize,
) -> Result<()> {
    let mut tar = tar::Archive::new(reader);
    let mut total = 0u64;
    let mut count = 0;
    let mut paths = BTreeSet::new();
    fs::create_dir_all(dest)?;
    for entry in tar.entries()? {
        let mut entry = entry?;
        let p = entry.path()?.to_path_buf();
        let p = if strip_prefix {
            p.components().skip(1).collect::<std::path::PathBuf>()
        } else {
            p
        };
        if p.as_os_str().is_empty() {
            continue;
        }
        let name = p.to_str().context("Non-UTF8 paths unsupported")?.to_owned();
        validation::safe_path(&name)?;
        let kind = entry.header().entry_type();
        ensure!(
            kind.is_file() || kind.is_dir(),
            "Archive symlinks, hardlinks and special files forbidden"
        );
        ensure!(
            paths.insert(name.to_lowercase()),
            "Duplicate or case-colliding archive path"
        );
        let path = dest.join(&p);
        if kind.is_dir() {
            fs::create_dir_all(&path)?;
            continue;
        }
        count += 1;
        ensure!(count <= file_limit, "Too many files");
        let size = entry.size();
        total = total.checked_add(size).context("Size overflow")?;
        ensure!(total <= byte_limit, "Archive exceeds size cap");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}
pub fn create(
    root: &Path,
    url: &str,
    task: &str,
    mode: Mode,
    issue: Option<u64>,
    cancel: &process::Cancel,
) -> Result<Run> {
    ensure!(task.len() <= 20000, "Task too long");
    let (owner, repo) = github::parse_repo(url)?;
    let meta = github::public_repo(&owner, &repo)?;
    ensure!(
        meta["size"].as_u64().unwrap_or(u64::MAX) <= 500_000,
        "Repository exceeds 500 MB cap"
    );
    let base = meta["default_branch"]
        .as_str()
        .context("No default branch")?
        .to_owned();
    let commit = github::pinned(&owner, &repo, &base)?;
    let id = format!(
        "r{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    let runroot = root.join("runs").join(&id);
    fs::create_dir_all(&runroot)?;
    let mut git = process::git();
    git.args([
        "clone",
        "--bare",
        "--depth",
        "1",
        "--single-branch",
        "--branch",
        &base,
        "--",
        url,
    ])
    .arg(runroot.join("cache.git"));
    process::success(process::run(
        git,
        vec!["git clone public repository".into()],
        Duration::from_secs(180),
        cancel,
    )?)?;
    let mut pin = process::git();
    pin.arg("--git-dir")
        .arg(runroot.join("cache.git"))
        .args(["fetch", "--depth", "1", "origin", &commit]);
    process::success(process::run(
        pin,
        vec!["git fetch pinned commit".into()],
        Duration::from_secs(120),
        cancel,
    )?)?;
    let archive = runroot.join("source.tar");
    let mut cmd = process::git();
    cmd.arg("--git-dir")
        .arg(runroot.join("cache.git"))
        .args(["archive", "--format=tar"])
        .arg(format!("--output={}", archive.display()))
        .arg(&commit);
    process::success(process::run(
        cmd,
        vec!["git archive pinned snapshot".into()],
        Duration::from_secs(90),
        cancel,
    )?)?;
    ensure!(
        fs::metadata(&archive)?.len() <= 550_000_000,
        "Archive too large"
    );
    unpack(
        fs::File::open(&archive)?,
        &runroot.join("pristine"),
        false,
        500_000_000,
        50_000,
    )?;
    fs::remove_file(archive)?;
    ensure!(
        runroot.join("pristine/Cargo.toml").is_file(),
        "Version 1 supports Rust Cargo repositories only"
    );
    copy_tree(&runroot.join("pristine"), &runroot.join("work"))?;
    let mut preflight = scan(&runroot.join("pristine"))?;
    preflight.license = meta["license"]["spdx_id"]
        .as_str()
        .unwrap_or("No declared license")
        .into();
    match github::related(&owner, &repo) {
        Ok(r) => preflight.related = r,
        Err(e) => preflight
            .flags
            .push(format!("Duplicate API check unavailable: {e}")),
    }
    let task = if let Some(n) = issue {
        github::issue(&owner, &repo, n)?
    } else {
        process::scrub(task)
    };
    Ok(Run {
        id,
        created: now(),
        updated: now(),
        owner,
        repo,
        source: url.trim_end_matches('/').trim_end_matches(".git").into(),
        commit,
        base,
        mode,
        task,
        issue_number: issue,
        state: State::Pinned,
        root: runroot,
        preflight,
        proposal: None,
        evidence: Evidence::default(),
        model_id: String::new(),
        model_hash: String::new(),
        prompts: vec![],
        model_outputs: vec![],
        attempts: 0,
        context_paths: vec![],
        notes: vec![],
        pr_url: None,
        mcp_approvals: vec![],
        tool_results: vec![],
    })
}
pub fn scan(root: &Path) -> Result<Preflight> {
    let mut p = Preflight::default();
    for ent in walkdir::WalkDir::new(root) {
        let ent = ent?;
        if !ent.file_type().is_file() {
            continue;
        }
        p.file_count += 1;
        p.bytes += ent.metadata()?.len();
        let name = ent
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        let lower = name.to_lowercase();
        if name.ends_with("build.rs")
            || name.starts_with(".cargo/")
            || name.contains("rust-toolchain")
            || name == ".gitmodules"
            || name == ".gitattributes"
            || name.ends_with(".exe")
            || name.ends_with(".so")
        {
            p.flags.push(name.clone());
        }
        if name.ends_with("Cargo.toml") {
            let src = fs::read_to_string(ent.path())?;
            if src.contains("proc-macro") || src.contains("git =") || src.contains("git=") {
                p.flags
                    .push(format!("{name}: procedural macro or git dependency"));
            }
        }
        if lower.contains("contributing")
            || lower.contains("code_of_conduct")
            || lower.contains("pull_request_template")
            || lower.contains("ai_policy")
            || lower == "agents.md"
            || lower == "readme.md"
        {
            if ent.metadata()?.len() <= 64000
                && p.policies.len() < 20
                && p.policies.values().map(String::len).sum::<usize>() < 128000
            {
                p.policies
                    .insert(name, process::scrub(&fs::read_to_string(ent.path())?));
            }
        }
    }
    Ok(p)
}
pub fn context(root: &Path, task: &str, max_chars: usize) -> Result<(String, Vec<String>)> {
    ensure!(
        (2000..=64000).contains(&max_chars),
        "Context budget must be 2,000-64,000 characters"
    );
    let terms = task
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| s.len() > 3)
        .map(str::to_lowercase)
        .collect::<BTreeSet<_>>();
    let mut files = vec![];
    let mut map = String::new();
    let map_cap = (max_chars / 4).min(6000);
    for ent in walkdir::WalkDir::new(root).sort_by_file_name() {
        let ent = ent?;
        if !ent.file_type().is_file() {
            continue;
        }
        let path = ent
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if map.len() < map_cap {
            map.push_str(&format!("{path}\n"));
        }
        if path.ends_with(".rs") && ent.metadata()?.len() <= 128000 && !path.starts_with("tests/") {
            let text = fs::read_to_string(ent.path())?;
            if map.len() < map_cap {
                for symbol in crate::code::outline(&text)? {
                    if map.len() + symbol.len() > map_cap {
                        break;
                    }
                    map.push_str(&format!("  {symbol}\n"));
                }
            }
            if process::has_secret(&text) {
                continue;
            }
            let lower = text.to_lowercase();
            let score = terms
                .iter()
                .map(|t| {
                    (if path.to_lowercase().contains(t) {
                        10
                    } else {
                        0
                    }) + (if lower.contains(t) { 1 } else { 0 })
                })
                .sum::<usize>();
            files.push((score, path, text));
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut out = format!("REPOSITORY FILE MAP (untrusted data)\n{map}\n");
    let mut selected = vec![];
    for (_, path, src) in files {
        if selected.len() >= 20 {
            break;
        }
        let lines = src.lines().collect::<Vec<_>>();
        let start = if src.len() > max_chars / 2 {
            let anchor = lines
                .iter()
                .position(|l| terms.iter().any(|t| l.to_lowercase().contains(t)))
                .unwrap_or(0);
            anchor.saturating_sub(15)
        } else {
            0
        };
        let count = if src.len() > max_chars / 2 {
            100
        } else {
            lines.len()
        };
        let numbered = lines
            .iter()
            .enumerate()
            .skip(start)
            .take(count)
            .map(|(i, line)| format!("{}: {}\n", i + 1, line))
            .collect::<String>();
        let block = format!("\nBEGIN UNTRUSTED SOURCE {path}\n{numbered}END UNTRUSTED SOURCE\n");
        if out.len() + block.len() > max_chars {
            continue;
        }
        out.push_str(&block);
        selected.push(path);
    }
    ensure!(
        !selected.is_empty(),
        "No Rust source fits the context budget. Increase context or select a smaller repository."
    );
    Ok((out, selected))
}
