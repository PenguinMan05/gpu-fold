use crate::{types::*,process,validation};
use anyhow::{bail,ensure,Context,Result};
use reqwest::blocking::{Client,Response};
use serde_json::{json,Value};
use std::io::Read;
use base64::Engine as _;
use std::time::Duration;

pub fn client()->Result<Client>{Ok(Client::builder().user_agent("Local-Rust-Repair/0.1.0").redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(30)).build()?)}
pub fn response(mut r:Response)->Result<Value>{let status=r.status();let mut data=Vec::new();r.by_ref().take(2_000_001).read_to_end(&mut data)?;ensure!(data.len()<=2_000_000,"Remote response too large");if !status.is_success(){bail!("GitHub HTTP {} (check access, OAuth restrictions, or rate limits)",status.as_u16())}Ok(serde_json::from_slice(&data)?)}
pub fn parse_repo(url:&str)->Result<(String,String)>{let s=url.trim().strip_prefix("https://github.com/").context("Use https://github.com/OWNER/REPOSITORY")?.trim_end_matches('/').trim_end_matches(".git");let parts=s.split('/').collect::<Vec<_>>();ensure!(parts.len()==2,"Repository URL must contain owner and name only");for p in &parts{ensure!(!p.is_empty()&&p.len()<100&&*p!="."&&*p!=".."&&p.chars().all(|c|c.is_ascii_alphanumeric()||matches!(c,'_'|'-'|'.')),"Invalid repository name")}Ok((parts[0].into(),parts[1].into()))}
pub fn public_repo(owner:&str,repo:&str)->Result<Value>{let r=response(client()?.get(format!("https://api.github.com/repos/{owner}/{repo}")).send()?)?;ensure!(r["private"].as_bool()==Some(false),"Version 1 supports public repositories only");ensure!(!r["archived"].as_bool().unwrap_or(false),"Archived repository cannot accept fixes");Ok(r)}
pub fn pinned(owner:&str,repo:&str,base:&str)->Result<String>{let v=response(client()?.get(format!("https://api.github.com/repos/{owner}/{repo}/commits/{base}")).send()?)?;let sha=v["sha"].as_str().context("Missing commit SHA")?;ensure!(sha.len()==40&&sha.chars().all(|c|c.is_ascii_hexdigit()),"Invalid SHA");Ok(sha.into())}
pub fn related(owner:&str,repo:&str)->Result<Vec<(String,String)>>{let v=response(client()?.get(format!("https://api.github.com/repos/{owner}/{repo}/issues")).query(&[("state","all"),("per_page","100"),("sort","updated")]).send()?)?;Ok(v.as_array().context("Invalid issue response")?.iter().filter_map(|i|Some((i["title"].as_str()?.to_owned(),i["html_url"].as_str()?.to_owned()))).collect())}
pub fn issue(owner:&str,repo:&str,number:u64)->Result<String>{let v=response(client()?.get(format!("https://api.github.com/repos/{owner}/{repo}/issues/{number}")).send()?)?;ensure!(v.get("pull_request").is_none(),"Choose an issue, not an existing pull request");ensure!(v["state"].as_str()==Some("open"),"Choose an open issue");let t=format!("Issue #{number}: {}\n{}",v["title"].as_str().unwrap_or(""),v["body"].as_str().unwrap_or(""));ensure!(t.len()<=20000,"Issue too long");Ok(process::scrub(&t))}
pub fn save_token(token:&str)->Result<()> {ensure!(!token.trim().is_empty(),"Empty token");keyring::Entry::new("local-rust-repair","github")?.set_password(token.trim())?;Ok(())}
pub fn token()->Result<String>{Ok(keyring::Entry::new("local-rust-repair","github")?.get_password().context("Sign in before publishing")?)}
pub fn revoke_local()->Result<()>{keyring::Entry::new("local-rust-repair","github")?.delete_credential()?;Ok(())}
#[derive(Clone,Debug)]
pub struct DeviceCode {pub code:String,pub user_code:String,pub verification_uri:String,pub interval:u64,pub expires:u64}
pub fn device_start(client_id:&str)->Result<DeviceCode>{ensure!(!client_id.is_empty(),"Set an OAuth app client ID with device flow enabled");let v=response(client()?.post("https://github.com/login/device/code").header("Accept","application/json").form(&[("client_id",client_id),("scope","public_repo")]).send()?)?;Ok(DeviceCode{code:v["device_code"].as_str().context("Device flow rejected")?.into(),user_code:v["user_code"].as_str().context("No user code")?.into(),verification_uri:v["verification_uri"].as_str().unwrap_or("https://github.com/login/device").into(),interval:v["interval"].as_u64().unwrap_or(5),expires:now()+v["expires_in"].as_u64().unwrap_or(900)})}
pub fn device_poll(client_id:&str,d:&DeviceCode)->Result<bool>{ensure!(now()<d.expires,"Device code expired");let v=response(client()?.post("https://github.com/login/oauth/access_token").header("Accept","application/json").form(&[("client_id",client_id),("device_code",&d.code),("grant_type","urn:ietf:params:oauth:grant-type:device_code")]).send()?)?;if let Some(t)=v["access_token"].as_str(){save_token(t)?;return Ok(true)}match v["error"].as_str(){Some("authorization_pending")|Some("slow_down")=>Ok(false),_=>bail!("Device sign-in failed or was denied")}}
pub fn draft_body(run:&Run)->String{format!("## Problem\n{}\n\n## Change\n{}\n\n## AI assistance\nDrafted with local model `{}`. Human reviewed every changed file.\n\n## Evidence\nLevel {}: {}\nPinned base: `{}`\n\n## Tests\n{}\n",run.task,run.proposal.as_ref().map(|p|p.explanation.as_str()).unwrap_or(""),run.model_id,run.evidence.level,run.evidence.reason,run.commit,run.evidence.baseline.iter().chain(&run.evidence.after).map(|t|format!("- `{}`: exit {:?}, {} ms",t.command.join(" "),t.exit_code,t.elapsed_ms)).collect::<Vec<_>>().join("\n"))}
pub fn changed_files(run:&Run)->Vec<String>{let mut files=run.proposal.as_ref().map(|p|p.edits.iter().map(|e|e.path.clone()).collect::<Vec<_>>()).unwrap_or_default();if let Some(r)=run.proposal.as_ref().and_then(|p|p.reproduction.as_ref()){files.push(r.path.clone())}files}
pub fn policy_blocks_ai(run:&Run)->bool{run.preflight.policies.values().any(|s|{let s=s.to_ascii_lowercase();["do not accept ai-generated","do not accept ai generated","ai-generated contributions are not accepted","ai-generated contributions are prohibited","no ai-generated contributions","no ai generated contributions"].iter().any(|p|s.contains(p))})}
pub fn check_publish(run:&Run,a:&PublishApproval)->Result<()> {
    parse_repo(&run.source)?;ensure!(!policy_blocks_ai(run),"Repository policy appears to prohibit AI-generated contributions; publishing is blocked");
    ensure!(run.state==State::Review&&run.evidence.level==3,"Verified level-3 evidence required");
    ensure!(a.rules_allow_ai&&a.duplicates_checked&&a.issue_requirement_met&&a.human_responsibility,"Complete contribution and duplicate checks");
    ensure!(!a.title.trim().is_empty()&&a.title.len()<=200,"PR title required (maximum 200 characters)");
    let files=changed_files(run);ensure!(files.iter().all(|p|a.reviewed_files.contains(p)),"Review every changed file");
    ensure!(run.mode==Mode::Issue||run.evidence.level==3,"A sweep must reproduce a bug before publishing");
    validation::verify_work(&run.root.join("work"),&run.root.join("pristine"),run.proposal.as_ref().context("Missing proposal")?)?;Ok(())
}
pub fn publish(run:&Run,a:&PublishApproval,settings:&Settings,cancel:&process::Cancel)->Result<String>{
    check_publish(run,a)?;let token=token()?;let c=client()?;
    let user=response(c.get("https://api.github.com/user").bearer_auth(&token).send()?)?;let login=user["login"].as_str().context("No authenticated login")?;
    ensure!(pinned(&run.owner,&run.repo,&run.base)?==run.commit,"Upstream moved. Start a fresh run and re-test before publishing.");
    ensure!(!login.eq_ignore_ascii_case(&run.owner),"Version 1 targets contributions to other owners; it never pushes upstream");
    process::check(cancel)?;
    let fork_url=format!("https://api.github.com/repos/{login}/{}",run.repo);
    let existing=c.get(&fork_url).bearer_auth(&token).send()?;
    let _created=if existing.status().as_u16()==404{response(c.post(format!("https://api.github.com/repos/{}/{}/forks",run.owner,run.repo)).bearer_auth(&token).json(&json!({"default_branch_only":true})).send()?)?}else{response(existing)?};
    // GitHub fork creation is asynchronous. Check availability with a bounded poll.
    let mut ready=None;for _ in 0..20 {process::check(cancel)?;let response=c.get(&fork_url).bearer_auth(&token).send()?;if response.status().is_success(){ready=Some(crate::github::response(response)?);break}std::thread::sleep(Duration::from_secs(1));}
    let fork=ready.context("Fork not ready; inspect GitHub before retrying")?;ensure!(fork["fork"].as_bool()==Some(true)&&fork["parent"]["full_name"].as_str().is_some_and(|n|n.eq_ignore_ascii_case(run.source.trim_start_matches("https://github.com/"))),"Destination is not your fork of this repository");
    let dir=run.root.join("publish");ensure!(!dir.exists(),"Publish workspace exists; inspect the prior attempt before retrying");
    let mut cmd=process::git();cmd.args(["-c","protocol.file.allow=always","clone","--no-checkout"]).arg(run.root.join("cache.git")).arg(&dir);process::success(process::run(cmd,vec!["git clone app-cache".into()],Duration::from_secs(60),cancel)?)?;
    let branch=format!("repair/{}",run.id);
    fn git_at(dir:&std::path::Path,args:&[&str],cancel:&process::Cancel)->Result<()> {let mut g=process::git();g.current_dir(dir).args(args);process::success(process::run(g,args.iter().map(|s|s.to_string()).collect(),Duration::from_secs(60),cancel)?)?;Ok(())}
    git_at(&dir,&["update-ref",&format!("refs/heads/{branch}"),&run.commit],cancel)?;
    git_at(&dir,&["symbolic-ref","HEAD",&format!("refs/heads/{branch}")],cancel)?;
    git_at(&dir,&["read-tree",&run.commit],cancel)?;
    crate::repository::copy_tree(&run.root.join("work"),&dir)?;
    git_at(&dir,&["add","--all"],cancel)?;
    let mut add=process::git();add.current_dir(&dir).args(["add","--force","--"]).args(changed_files(run));process::success(process::run(add,vec!["git add reviewed files".into()],Duration::from_secs(30),cancel)?)?;
    let name=if settings.author_name.is_empty(){login.to_owned()}else{settings.author_name.clone()};
    let email=if settings.author_email.is_empty(){format!("{}+{}@users.noreply.github.com",user["id"].as_u64().unwrap_or(0),login)}else{settings.author_email.clone()};
    let mut cmd=process::git();cmd.current_dir(&dir).args(["-c",&format!("user.name={name}"),"-c",&format!("user.email={email}"),"commit","-m",&a.title]);if a.signoff{cmd.arg("--signoff");}
    process::success(process::run(cmd,vec!["git commit reviewed patch".into()],Duration::from_secs(60),cancel)?)?;
    let mut scope=process::git();scope.current_dir(&dir).args(["diff","--name-only",&run.commit,"HEAD"]);let observed=process::success(process::run(scope,vec!["git verify committed scope".into()],Duration::from_secs(30),cancel)?)?;
    let mut actual=observed.output.lines().filter(|s|!s.is_empty()).map(str::to_owned).collect::<Vec<_>>();actual.sort();let mut expected=changed_files(run);expected.sort();ensure!(actual==expected,"Committed files differ from reviewed scope; publication stopped");
    for path in &expected{let mut read=process::git();read.current_dir(&dir).args(["show",&format!("HEAD:{path}")]);let content=process::success(process::run(read,vec!["git verify committed content".into()],Duration::from_secs(30),cancel)?)?;let expected=std::fs::read_to_string(run.root.join("work").join(path))?;ensure!(content.output.trim_end()==process::scrub(&expected).trim_end(),"Committed content differs from reviewed patch");}
    let destination=format!("https://github.com/{login}/{}.git",run.repo);
    let mut push=process::git();push.current_dir(&dir).env("GIT_CONFIG_COUNT","1").env("GIT_CONFIG_KEY_0",format!("http.{destination}.extraHeader")).env("GIT_CONFIG_VALUE_0",format!("Authorization: Basic {}",base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}")))).args(["push",&destination,&format!("HEAD:refs/heads/{branch}")]);
    let result=process::run(push,vec!["git push personal fork branch".into()],Duration::from_secs(120),cancel)?;
    ensure!(result.exit_code==Some(0),"Push failed; check GitHub permissions. No force-push was attempted.");
    let body=format!("{}\n\n{}",a.body,draft_body(run));
    let pr=response(c.post(format!("https://api.github.com/repos/{}/{}/pulls",run.owner,run.repo)).bearer_auth(&token).json(&json!({"title":a.title,"body":body,"head":format!("{login}:{branch}"),"base":run.base,"draft":true,"maintainer_can_modify":false})).send()?)?;
    Ok(pr["html_url"].as_str().context("PR created but URL missing; check GitHub before retrying")?.into())
}
