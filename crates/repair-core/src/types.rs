use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Mode { Issue, Sweep }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum State { Pinned, Analyzing, AwaitPlan, AwaitPatch, Patched, Verifying, Review, NoFix, Cancelled, Publishing, Published }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Citation { pub path: String, pub start_line: usize, pub snippet: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edit { pub path: String, pub old: String, pub new: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reproduction { pub path: String, pub content: String, pub test_name: String, pub failure_marker: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proposal {
    pub explanation: String,
    pub citations: Vec<Citation>,
    pub planned_files: Vec<String>,
    pub edits: Vec<Edit>,
    pub reproduction: Option<Reproduction>,
    pub no_fix: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Observation { pub command: Vec<String>, pub exit_code: Option<i32>, pub output: String, pub elapsed_ms: u128, pub truncated: bool }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evidence { pub level: u8, pub reason: String, pub baseline: Vec<Observation>, pub after: Vec<Observation> }
impl Default for Evidence { fn default() -> Self { Self { level: 0, reason: "No model claim or test evidence yet".into(), baseline: vec![], after: vec![] } } }
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Preflight { pub flags: Vec<String>, pub policies: BTreeMap<String,String>, pub related: Vec<(String,String)>, pub license: String, pub file_count: usize, pub bytes: u64 }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub id: String, pub created: u64, pub updated: u64,
    pub owner: String, pub repo: String, pub source: String, pub commit: String, pub base: String,
    pub mode: Mode, pub task: String, pub issue_number: Option<u64>, pub state: State,
    pub root: PathBuf, pub preflight: Preflight, pub proposal: Option<Proposal>, pub evidence: Evidence,
    pub model_id: String, pub model_hash: String, pub prompts: Vec<String>, pub model_outputs: Vec<String>,
    pub attempts: u8, pub context_paths: Vec<String>, pub notes: Vec<String>, pub pr_url: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub endpoint: String, pub model_id: String, pub weights_path: String,
    pub context_chars: usize, pub max_output_tokens: usize, pub image: String, pub author_name: String,
    pub author_email: String, pub oauth_client_id: String,
}
impl Default for Settings { fn default()->Self { Self {
    endpoint:"http://127.0.0.1:1234/v1/chat/completions".into(), model_id:"local".into(), weights_path:String::new(),
    context_chars:24000, max_output_tokens:4096, image:"localhost/rust-repair-sandbox:1".into(),
    author_name:String::new(), author_email:String::new(), oauth_client_id:String::new(),
} } }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishApproval { pub reviewed_files: Vec<String>, pub rules_allow_ai: bool, pub duplicates_checked: bool, pub issue_requirement_met: bool, pub human_responsibility: bool, pub title: String, pub body: String, pub signoff: bool }
