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
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
        let entry = entry?;
        if !entry.file_type().is_file() { continue; }
        let path = entry.path().strip_prefix(root)?.to_string_lossy().replace('\\', "/");
        if !is_instruction(&path) || entry.metadata()?.len() > 64000 { continue; }
        let text = validation::text(root, &path)?;
        if applies(&path, &text, source) { files.push((path, process::scrub(&text))); }
        if files.len() >= 32 { break; }
    }
    // Broad conventions first; a more specific directory's guidance takes precedence.
    files.sort_by_key(|(path, _)| (path.matches('/').count(), path.clone()));
    let mut out = String::new();
    let mut included = Vec::new();
    let per_file = budget.saturating_sub(files.len() * 90) / files.len().max(1);
    for (path, text) in files {
        if per_file < 24 { break; }
        let text = crate::model::clip_text(&text, per_file);
        let scope = if path.starts_with('.') || !path.contains('/') || path.to_ascii_lowercase().ends_with("/skill.md") { "repository conventions; skills are optional" } else { path.rsplit_once('/').map_or("repository", |(parent,_)| parent) };
        let block = format!("\nPROJECT GUIDANCE {path} (scope: {scope})\n{text}\nEND GUIDANCE\n");
        if out.len() + block.len() > budget { break; }
        out.push_str(&block);
        included.push(path);
    }
    Ok((out, included))
}
