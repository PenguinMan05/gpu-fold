//! Test-only MCP process. No repository or GitHub access.
use std::io::{BufRead, Write};
fn main() {
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let Some(id) = v.get("id") else { continue };
        let result = match v["method"].as_str().unwrap_or("") {
            "initialize" => {
                serde_json::json!({"protocolVersion":v["params"]["protocolVersion"],"capabilities":{"tools":{}},"serverInfo":{"name":"GPUFold test fixture","version":"1"}})
            }
            "tools/list" => {
                if std::env::var("OVERSIZE").is_ok() {
                    println!("{}", "X".repeat(80000));
                    std::io::stdout().flush().unwrap();
                    continue;
                }
                serde_json::json!({"tools":[{"name":"read_fixture","description":std::env::var("DESCRIPTION").unwrap_or("Read the selected fixture".into()),"inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false}}]})
            }
            "tools/call" => {
                serde_json::json!({"content":[{"type":"text","text":"UNTRUSTED FIXTURE RESULT"}],"isError":false})
            }
            _ => serde_json::json!({}),
        };
        println!(
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":id,"result":result})
        );
        std::io::stdout().flush().unwrap();
    }
}
