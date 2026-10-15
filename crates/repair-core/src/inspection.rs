//! App-owned, read-only investigation of the pinned repository. No shell or network.
use crate::{model::clip_text, process, types::*, validation};
use anyhow::{ensure, Result};
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

pub const SERVER: &str = "gpufold-source";
pub fn builtin(request: &ToolRequest) -> bool { request.server == SERVER && matches!(request.name.as_str(), "read_file" | "search_code") }
pub fn definitions() -> serde_json::Value {
    json!([
        {"server":SERVER,"name":"read_file","arguments":{"path":"repository-relative file","start_line":"1-based integer","line_count":"1-120"}},
        {"server":SERVER,"name":"search_code","arguments":{"query":"literal symbol or text","path_prefix":"optional relative directory"}}
    ])
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs { path: String, #[serde(default="first")] start_line: usize, #[serde(default="count")] line_count: usize }
fn first() -> usize { 1 } fn count() -> usize { 80 }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs { query: String, #[serde(default)] path_prefix: String }

fn readable(root: &Path, path: &str) -> Result<String> {
    let text = validation::text(root,path)?;
    ensure!(!process::has_secret(&text), "Source inspection skipped a file containing possible credentials");
    Ok(text)
}
fn read_args(request: &ToolRequest) -> Result<ReadArgs> {
    let args: ReadArgs = serde_json::from_value(json!(request.arguments))?;
    validation::safe_path(&args.path)?;
    ensure!(args.start_line>0 && (1..=120).contains(&args.line_count), "read_file requires start_line >= 1 and line_count 1-120");
    Ok(args)
}
fn search_args(request: &ToolRequest) -> Result<SearchArgs> {
    let args: SearchArgs = serde_json::from_value(json!(request.arguments))?;
    ensure!(!args.query.trim().is_empty() && args.query.len()<=160 && !args.query.chars().any(char::is_control), "search_code needs a short literal query");
    if !args.path_prefix.is_empty() { validation::safe_path(args.path_prefix.trim_end_matches('/'))?; }
    Ok(args)
}
pub fn validate(request: &ToolRequest) -> Result<()> {
    ensure!(builtin(request), "Unknown built-in inspection tool");
    if request.name == "read_file" { read_args(request)?; } else { search_args(request)?; }
    Ok(())
}
fn focus(run: &mut Run, path: &str, line: usize, line_count: usize) {
    run.inspections.retain(|old| old.path != path);
    run.inspections.insert(0, SourceInspection { path:path.into(), start_line:line.max(1), line_count:line_count.min(120) });
    run.inspections.truncate(6);
}
pub fn execute(run: &mut Run, request: &ToolRequest, cancel: &process::Cancel) -> Result<String> {
    validate(request)?;
    let root=run.root.join("pristine");
    if request.name == "read_file" {
        let args=read_args(request)?;
        let text=readable(&root,&args.path)?;
        ensure!(args.start_line <= text.lines().count(), "{} has only {} lines", args.path,text.lines().count());
        focus(run,&args.path,args.start_line,args.line_count);
        return Ok(format!("Opened {}:{}-{} from the pinned source",args.path,args.start_line,args.start_line.saturating_add(args.line_count-1)));
    }
    let args=search_args(request)?;
    let mut hits=Vec::new();
    let query=args.query.to_lowercase();
    for entry in walkdir::WalkDir::new(&root).sort_by_file_name() {
        process::check(cancel)?;
        let entry=entry?;
        if !entry.file_type().is_file() || entry.metadata()?.len()>256000 { continue; }
        let path=entry.path().strip_prefix(&root)?.to_string_lossy().replace('\\',"/");
        if !args.path_prefix.is_empty() && !(path==args.path_prefix || path.starts_with(&format!("{}/",args.path_prefix.trim_end_matches('/')))) { continue; }
        if !path.ends_with(".rs") && !path.ends_with(".toml") && !path.ends_with(".md") { continue; }
        let Ok(text)=readable(&root,&path) else { continue; };
        for (i,line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&query) {
                hits.push((path.clone(),i+1,clip_text(line,160).to_owned()));
                break;
            }
        }
        if hits.len()>=8 { break; }
    }
    for (path,line,_) in hits.iter().rev() { focus(run,path,line.saturating_sub(12).max(1),80); }
    Ok(if hits.is_empty() { format!("No literal matches for {:?}; try another symbol or wording",args.query) } else {
        format!("Found {:?}:\n{}",args.query,hits.iter().map(|(p,l,s)|format!("{p}:{l} {s}")).collect::<Vec<_>>().join("\n"))
    })
}
/// Target the implicated source after rejection instead of repeating the same blind pass.
pub fn focus_rejected(run: &mut Run) {
    let Some(raw)=run.model_outputs.last() else { return; };
    let Ok(proposal)=crate::model::parse_proposal(raw) else { return; };
    let root=run.root.join("pristine");
    for citation in proposal.citations.iter().take(3) {
        if readable(&root,&citation.path).is_ok() { focus(run,&citation.path,citation.start_line.saturating_sub(12).max(1),80); }
    }
    for edit in proposal.edits.iter().take(3) {
        if let Ok(source)=readable(&root,&edit.path) {
            if let Some(at)=source.find(&edit.old).filter(|_| !edit.old.is_empty()) { focus(run,&edit.path,source[..at].lines().count().saturating_sub(12).max(1),100); }
        }
    }
}
pub fn context(run: &Run, budget: usize) -> Result<(String,Vec<String>)> {
    let root=run.root.join("pristine");
    let mut text=String::new(); let mut paths=Vec::new();
    for request in &run.inspections {
        let source=readable(&root,&request.path)?;
        let header=format!("\nBEGIN UNTRUSTED SOURCE {} (requested inspection)\n",request.path);
        let footer="END UNTRUSTED SOURCE\n";
        let allowance=(budget/2).max(128);
        if text.len()+header.len()+footer.len()+32>budget { break; }
        let mut block=header.clone();
        for (i,line) in source.lines().enumerate().skip(request.start_line.saturating_sub(1)).take(request.line_count) {
            let numbered=format!("{}: {line}\n",i+1);
            if text.len()+block.len()+numbered.len()+footer.len()>budget || block.len()+numbered.len()>allowance { break; }
            block.push_str(&numbered);
        }
        if block==header { continue; }
        text.push_str(&block); text.push_str(footer); paths.push(request.path.clone());
    }
    Ok((text,paths))
}
/// Correct only demonstrable line-number/line-prefix errors, never fuzzy source claims.
pub fn anchor_citations(root: &Path, proposal: &mut Proposal, allowed: &[String]) -> Result<Vec<String>> {
    let prefix=regex::Regex::new(r"^\s*\d+: ?")?;
    let mut notes=Vec::new();
    for citation in &mut proposal.citations {
        if !allowed.contains(&citation.path) { continue; }
        let source=validation::text(root,&citation.path)?;
        let original=citation.snippet.trim_end_matches('\n').to_owned();
        if original.is_empty() { continue; }
        let stripped=original.lines().map(|line|prefix.replace(line,"").into_owned()).collect::<Vec<_>>().join("\n");
        for candidate in [original.as_str(),stripped.as_str()] {
            if candidate.is_empty() { continue; }
            let matches=source.match_indices(candidate).filter(|(at,_)|*at==0 || source.as_bytes()[at-1]==b'\n').map(|(at,_)|at).collect::<Vec<_>>();
            // A citation already correct at the specified line needs no disambiguation.
            let correct=source.lines().skip(citation.start_line.saturating_sub(1)).collect::<Vec<_>>().join("\n").starts_with(candidate) && citation.start_line>0;
            if correct || matches.len()==1 {
                let line=if correct {citation.start_line} else { source[..matches[0]].bytes().filter(|b|*b==b'\n').count()+1 };
                if line!=citation.start_line || candidate!=citation.snippet { notes.push(format!("Verified exact source in {} and anchored citation at line {line}",citation.path)); }
                citation.start_line=line; citation.snippet=candidate.to_owned(); break;
            }
        }
    }
    Ok(notes)
}
