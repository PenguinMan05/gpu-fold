use anyhow::{bail,ensure,Context,Result};
use crate::{process,validation};
use serde::{Deserialize,Serialize};
use serde_json::{Map,Value};
use std::{collections::BTreeMap,path::Path,time::Duration};
use rmcp::{ServiceExt,transport::TokioChildProcess,model::CallToolRequestParam};

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {pub command:Option<String>,#[serde(default)] pub args:Vec<String>,#[serde(default)] pub env:BTreeMap<String,String>,pub url:Option<String>,#[serde(default)] pub cwd:Option<String>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {#[serde(rename="mcpServers")]pub servers:BTreeMap<String,Server>}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct ToolPin {pub name:String,pub hash:String,pub definition:Value}
pub fn import(path:&Path)->Result<Config>{ensure!(std::fs::metadata(path)?.len()<=256000,"MCP config exceeds size cap");let c:Config=serde_json::from_slice(&std::fs::read(path)?).context("Expected mcpServers JSON. Unknown keys are rejected; original file remains untouched.")?;for s in c.servers.values(){ensure!(s.command.is_some()!=s.url.is_some(),"Define exactly one of command or URL");ensure!(s.env.len()<=30&&s.args.len()<=50,"MCP configuration over cap");}Ok(c)}
fn command(s:&Server,workspace:&Path)->Result<tokio::process::Command>{if s.url.is_some(){bail!("Remote MCP is disabled in 0.1.0; only explicitly approved local stdio servers are supported")}let exe=Path::new(s.command.as_deref().context("Missing command")?);ensure!(exe.is_absolute()&&exe.is_file(),"MCP command must be an absolute executable path");let mut c=tokio::process::Command::new(exe);c.args(&s.args).env_clear().env("PATH","/usr/local/bin:/usr/bin:/bin").env("HOME",workspace).current_dir(workspace).kill_on_drop(true);for(k,v)in &s.env{ensure!(!k.is_empty()&&!k.contains('='),"Invalid environment variable name");c.env(k,v);}Ok(c)}
pub fn inspect(s:&Server,workspace:&Path,consent:bool)->Result<Vec<ToolPin>>{ensure!(consent,"Server launch consent required");let rt=tokio::runtime::Runtime::new()?;rt.block_on(async{let transport=TokioChildProcess::new(command(s,workspace)?)?;let service=tokio::time::timeout(Duration::from_secs(15),().serve(transport)).await??;let tools=tokio::time::timeout(Duration::from_secs(15),service.list_all_tools()).await??;ensure!(tools.len()<=100,"MCP tool count over cap");let mut out=vec![];for t in tools{let definition=serde_json::to_value(&t)?;let bytes=serde_json::to_vec(&definition)?;ensure!(bytes.len()<=64000,"Tool definition too large");out.push(ToolPin{name:t.name.to_string(),hash:validation::digest(&bytes),definition});}service.cancel().await?;Ok(out)})}
fn local_refs_only(value:&Value)->Result<()>{match value{Value::Object(m)=>{if let Some(Value::String(r))=m.get("$ref"){ensure!(r.starts_with('#'),"External schema references are disabled")}for v in m.values(){local_refs_only(v)?}},Value::Array(a)=>{for v in a{local_refs_only(v)?}},_=>{}}Ok(())}
pub fn call(s:&Server,workspace:&Path,pin:&ToolPin,args:Map<String,Value>,consent:bool)->Result<String>{
    ensure!(consent,"Per-call tool consent required");local_refs_only(&pin.definition)?;let schema=jsonschema::JSONSchema::compile(&pin.definition["inputSchema"]).map_err(|e|anyhow::anyhow!("Unsupported tool schema: {e}"))?;let payload=Value::Object(args.clone());if let Err(errors)=schema.validate(&payload){let messages=errors.take(8).map(|e|e.to_string()).collect::<Vec<_>>().join("; ");bail!("Tool arguments do not match approved schema: {messages}")}ensure!(serde_json::to_vec(&args)?.len()<=32000,"MCP payload over cap");let rt=tokio::runtime::Runtime::new()?;rt.block_on(async{
        let service=tokio::time::timeout(Duration::from_secs(15),().serve(TokioChildProcess::new(command(s,workspace)?)?)).await??;let tools=tokio::time::timeout(Duration::from_secs(15),service.list_all_tools()).await??;let t=tools.iter().find(|t|t.name.as_ref()==pin.name).context("Approved tool disappeared")?;ensure!(validation::digest(&serde_json::to_vec(&serde_json::to_value(t)?)?)==pin.hash,"Tool definition changed; reapprove before calling");
        let result=tokio::time::timeout(Duration::from_secs(30),service.call_tool(CallToolRequestParam{name:pin.name.clone().into(),arguments:Some(args)})).await??;let text=serde_json::to_string_pretty(&result)?;ensure!(text.len()<=64000,"Tool result exceeds cap");service.cancel().await?;Ok(process::scrub(&text))
    })
}
