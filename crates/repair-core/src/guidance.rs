//! Read project conventions as scoped guidance, never as permission to execute code.
use crate::{process, validation};
use anyhow::Result;
use std::path::Path;

pub fn is_instruction(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    matches!(name, "agents.md" | "agents.override.md" | "skills.md" | "skill.md" | "claude.md" | "gemini.md" | "instructions.md" | "copilot-instructions.md" | ".cursorrules")
        || lower.starts_with(".github/instructions/") && lower.ends_with(".instructions.md")
        || lower.starts_with(".cursor/rules/") && lower.ends_with(".mdc")
        || lower.starts_with(".windsurf/rules/") && lower.ends_with(".md")
}

fn applies(path: &str, text: &str, source: &[String]) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower.starts_with(".github/instructions/") {
        if let Some(patterns) = text.lines().find_map(|line| line.trim().strip_prefix("applyTo:")) {
            let patterns = patterns.trim().trim_matches(['\'', '"']);
            return patterns.split(',').any(|p| {
                let escaped = regex::escape(p.trim()).replace(r"\*\*/", "(?:.*/)?").replace(r"\*\*", ".*").replace(r"\*", "[^/]*").replace(r"\?", "[^/]");
                regex::Regex::new(&format!("^{escaped}$")).is_ok_and(|re| source.iter().any(|s| re.is_match(s)))
            });
        }
    }
    if lower.starts_with(".github/instructions/") || lower == ".github/copilot-instructions.md" || lower.starts_with(".cursor/rules/") || lower.starts_with(".windsurf/rules/") || lower.ends_with("/skill.md") || !path.contains('/') {
        return true;
    }
    let parent = path.rsplit_once('/').map(|p| format!("{}/", p.0)).unwrap_or_default();
    source.iter().any(|s| s.starts_with(&parent))
}

pub fn context(root: &Path, source: &[String], budget: usize) -> Result<(String, Vec<String>)> {
    let mut candidates = Vec::new();
    for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
        let entry = entry?;
        if !entry.file_type().is_file() { continue; }
        let path = entry.path().strip_prefix(root)?.to_string_lossy().replace('\\', "/");
        if is_instruction(&path) && entry.metadata()?.len() <= 64000 { candidates.push(path); }
    }
    // Find governing AGENTS files before optional skills, even in large skill trees.
    candidates.sort_by_key(|path| {
        let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
        let priority = if name.starts_with("agents.") { 0 } else if name == "skill.md" { 2 } else { 1 };
        (priority, path.matches('/').count(), path.clone())
    });
    let mut files = Vec::new();
    let mut reserved = 0usize;
    for path in candidates.into_iter().take(256) {
        let text = validation::text(root, &path)?;
        if !applies(&path, &text, source) { continue; }
        let lower = path.to_ascii_lowercase();
        let global = lower.starts_with(".github/") || lower.starts_with(".cursor/") || lower.starts_with(".windsurf/") || !path.contains('/') || lower.ends_with("/skill.md");
        let scope = if global { "repository conventions; skills are optional" } else { path.rsplit_once('/').map_or("repository", |(parent,_)| parent) };
        let header = format!("\nPROJECT GUIDANCE {path} (scope: {scope})\n");
        let overhead = header.len() + "\nEND GUIDANCE\n".len();
        if reserved + overhead + 32 > budget { continue; }
        reserved += overhead + 32;
        files.push((path,header,process::scrub(&text)));
        if files.len() >= 32 { break; }
    }
    // Broad conventions first, followed by rules for more specific directories.
    files.sort_by_key(|(path,_,_)| (path.matches('/').count(), path.clone()));
    let extra = budget.saturating_sub(reserved) / files.len().max(1);
    let mut out = String::new();
    let mut included = Vec::new();
    for (path,header,text) in files {
        out.push_str(&header);
        out.push_str(crate::model::clip_text(&text, 32 + extra));
        out.push_str("\nEND GUIDANCE\n");
        included.push(path);
    }
    Ok((out, included))
}
