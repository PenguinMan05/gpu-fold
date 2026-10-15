use crate::types::*;
use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Component, Path};

pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn safe_path(p: &str) -> Result<()> {
    ensure!(
        !p.is_empty() && p.len() < 240 && !p.contains('\\') && !p.chars().any(char::is_control),
        "Invalid file path"
    );
    ensure!(
        p.split('/').all(|s| !s.is_empty() && s != "." && s != ".."),
        "Path contains empty or relative segments"
    );
    for c in Path::new(p).components() {
        match c {
            Component::Normal(s) => {
                let s = s.to_string_lossy();
                ensure!(
                    s != ".git" && !s.contains(':') && !s.ends_with('.') && !s.ends_with(' '),
                    "Unsafe path component"
                )
            }
            _ => bail!("Only relative normal file paths are allowed"),
        }
    }
    Ok(())
}
pub fn text(root: &Path, p: &str) -> Result<String> {
    safe_path(p)?;
    let path = root.join(p);
    let canonical = path.canonicalize()?;
    ensure!(
        canonical.starts_with(root.canonicalize()?),
        "File escapes workspace"
    );
    ensure!(
        !std::fs::symlink_metadata(&path)?.file_type().is_symlink(),
        "Symlink forbidden"
    );
    ensure!(
        std::fs::metadata(&path)?.len() <= 256000,
        "File exceeds context cap"
    );
    Ok(std::fs::read_to_string(path)?)
}
fn sensitive(p: &str) -> bool {
    let l = p.to_ascii_lowercase();
    l == "cargo.lock"
        || l.starts_with(".github/")
        || l.starts_with(".cargo/")
        || l.contains("license")
        || l.contains("rust-toolchain")
        || l.ends_with(".exe")
        || l.ends_with(".so")
        || l.ends_with(".bin")
        || l.ends_with("build.rs")
}
pub fn validate_proposal(root: &Path, p: &Proposal) -> Result<()> {
    ensure!(!p.explanation.trim().is_empty() && p.explanation.len() <= 20000, "A nonempty explanation is required (maximum 20,000 bytes)");
    ensure!(p.tool_requests.len() <= 10, "Tool-call budget exceeded");
    if p.no_fix {
        ensure!(
            p.edits.is_empty() && p.reproduction.is_none(),
            "No-fix proposal cannot contain edits"
        )
    }
    ensure!(
        (p.no_fix || !p.tool_requests.is_empty() || !p.citations.is_empty())
            && p.citations.len() <= 20,
        "Need 1-20 source citations for a bug claim"
    );
    for c in &p.citations {
        let src = text(root, &c.path)?;
        ensure!(
            c.start_line > 0 && !c.snippet.is_empty(),
            "Invalid citation"
        );
        let tail = src
            .lines()
            .skip(c.start_line - 1)
            .collect::<Vec<_>>()
            .join("\n");
        ensure!(
            tail.starts_with(c.snippet.trim_end_matches('\n')),
            "Citation does not match pinned source: {}:{}",
            c.path,
            c.start_line
        );
    }
    if p.no_fix {
        ensure!(
            p.tool_requests.is_empty(),
            "No-fix output cannot request tools"
        );
        return Ok(());
    }
    ensure!(
        p.planned_files.len() <= 5 && p.edits.len() + usize::from(p.reproduction.is_some()) <= 5,
        "Maximum five changed files including the reproduction test"
    );
    let planned = p.planned_files.iter().collect::<BTreeSet<_>>();
    ensure!(
        planned.len() == p.planned_files.len(),
        "Duplicate planned file"
    );
    let mut touched = BTreeSet::new();
    let mut changed = 0;
    for e in &p.edits {
        safe_path(&e.path)?;
        ensure!(planned.contains(&e.path), "Edit outside planned files");
        ensure!(
            touched.insert(e.path.clone()),
            "One edit per file; combine replacements"
        );
        ensure!(
            e.path.ends_with(".rs") && !sensitive(&e.path),
            "Only Rust source edits; protected path rejected"
        );
        ensure!(
            !e.path.starts_with("tests/")
                && !e.path.contains("/tests/")
                && !e.old.contains("#[test]")
                && !e.old.contains("#[cfg(test)]")
                && !e.new.contains("#[ignore]"),
            "Existing tests cannot be changed"
        );
        let src = text(root, &e.path)?;
        ensure!(
            !e.old.is_empty() && src.matches(&e.old).count() == 1,
            "Old text must match once"
        );
        let edit_at = src.find(&e.old).context("Replacement missing")?;
        for marker in ["#[cfg(test)]", "#[test]"] {
            if let Some(test_at) = src.find(marker) {
                ensure!(
                    edit_at + e.old.len() <= test_at,
                    "Existing test section cannot be edited"
                )
            }
        }
        ensure!(e.old != e.new, "Empty change");
        ensure!(!e.new.contains('\0'), "Binary output rejected");
        for s in [
            "-----BEGIN PRIVATE KEY",
            "ghp_",
            "github_pat_",
            "AKIA",
            "sk-proj-",
        ] {
            ensure!(!e.new.contains(s), "Possible secret rejected")
        }
        changed += e.old.lines().count() + e.new.lines().count();
        let patched = src.replacen(&e.old, &e.new, 1);
        ensure!(
            crate::code::tests(&src)? == crate::code::tests(&patched)?,
            "Existing test AST items must remain unchanged"
        );
        ensure!(
            !crate::process::has_secret(&e.new),
            "Possible secret in patch"
        );
    }
    ensure!(
        p.tool_requests.is_empty() || (p.edits.is_empty() && p.reproduction.is_none()),
        "Tool requests must precede a final patch"
    );
    if let Some(r) = &p.reproduction {
        safe_path(&r.path)?;
        ensure!(
            r.path.starts_with("tests/repair_") && r.path.ends_with(".rs"),
            "Reproduction must be tests/repair_*.rs"
        );
        ensure!(
            !root.join(&r.path).exists(),
            "Cannot overwrite existing test"
        );
        ensure!(
            !r.content.is_empty() && r.content.len() <= 16000,
            "Invalid reproduction size"
        );
        ensure!(
            r.content.contains("#[test]") && !r.content.contains("#[ignore]"),
            "Reproduction must contain an enabled test"
        );
        ensure!(
            valid_name(&r.test_name) && r.content.contains(&format!("fn {}", r.test_name)),
            "Invalid reproduction test name"
        );
        ensure!(
            r.failure_marker.len() >= 8
                && r.failure_marker.len() < 200
                && r.content.contains(&r.failure_marker),
            "Test must contain a distinct failure marker"
        );
        changed += r.content.lines().count();
    }
    if let Some(r) = &p.reproduction {
        ensure!(
            !crate::process::has_secret(&r.content),
            "Possible secret in reproduction"
        );
        crate::code::tests(&r.content)?;
    }
    ensure!(changed <= 150, "Patch exceeds 150 changed lines");
    Ok(())
}
pub fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() < 100 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
pub fn apply(root: &Path, pristine: &Path, p: &Proposal) -> Result<()> {
    validate_proposal(pristine, p)?;
    for e in &p.edits {
        let src = text(pristine, &e.path)?;
        ensure!(
            text(root, &e.path)? == src,
            "Working file changed since plan approval"
        );
        std::fs::write(root.join(&e.path), src.replacen(&e.old, &e.new, 1))?;
    }
    if let Some(r) = &p.reproduction {
        std::fs::create_dir_all(root.join("tests"))?;
        ensure!(
            !root.join(&r.path).exists(),
            "Reproduction path already exists"
        );
        std::fs::write(root.join(&r.path), &r.content)?;
    }
    Ok(())
}
pub fn verify_work(root: &Path, pristine: &Path, p: &Proposal) -> Result<()> {
    for e in &p.edits {
        let expected = text(pristine, &e.path)?.replacen(&e.old, &e.new, 1);
        ensure!(
            text(root, &e.path)? == expected,
            "Changed file differs from reviewed proposal"
        )
    }
    if let Some(r) = &p.reproduction {
        ensure!(text(root, &r.path)? == r.content, "Reproduction changed")
    }
    for ent in walkdir::WalkDir::new(root) {
        let ent = ent?;
        ensure!(!ent.file_type().is_symlink(), "Symlink in worktree");
        if ent.file_type().is_file() {
            let rel = ent
                .path()
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            let allowed = p.edits.iter().any(|e| e.path == rel)
                || p.reproduction.as_ref().is_some_and(|r| r.path == rel);
            if !allowed {
                ensure!(
                    std::fs::read(ent.path())?
                        == std::fs::read(pristine.join(&rel)).context("Unexpected new file")?,
                    "Unreviewed file changed: {rel}"
                )
            }
        }
    }
    for ent in walkdir::WalkDir::new(pristine) {
        let ent = ent?;
        if ent.file_type().is_file() {
            let rel = ent.path().strip_prefix(pristine)?;
            ensure!(root.join(rel).is_file(), "File missing from worktree")
        }
    }
    Ok(())
}
