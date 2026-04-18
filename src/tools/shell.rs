use async_trait::async_trait;
use anyhow::Result;
use serde_json::json;
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use super::Tool;
use crate::types::tool::ToolResult;

pub struct ShellTool {
    default_timeout_secs: u64,
}

impl ShellTool {
    pub fn new() -> Self {
        Self { default_timeout_secs: 30 }
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str { "shell.exec" }

    fn description(&self) -> &str {
        "在服务器上执行 shell 命令。仅用于无法通过专用工具完成的操作。\
         危险命令会被安全层拦截。命令执行有超时限制。"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "要执行的 shell 命令"
                },
                "working_dir": {
                    "type": "string",
                    "description": "工作目录，默认为 /"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "超时秒数，默认 30",
                    "default": 30
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult> {
        let command = args["command"].as_str()
            .ok_or_else(|| anyhow::anyhow!("缺少 command 参数"))?;
        let working_dir = args["working_dir"].as_str().unwrap_or("/");
        let timeout_secs = args["timeout_secs"].as_u64().unwrap_or(self.default_timeout_secs);

        if dry_run {
            return Ok(ToolResult::dry_run_preview(
                self.name(),
                &format!("将执行命令: `{}`\n工作目录: {}", command, working_dir),
            ));
        }

        let start = Instant::now();
        let result = timeout(
            Duration::from_secs(timeout_secs),
            Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(working_dir)
                .output(),
        ).await;

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                if output.status.success() {
                    Ok(ToolResult { success: true, tool: self.name().to_string(),
                        stdout, stderr, exit_code, duration_ms, dry_run_preview: None })
                } else {
                    Ok(ToolResult { success: false, tool: self.name().to_string(),
                        stdout, stderr, exit_code, duration_ms, dry_run_preview: None })
                }
            }
            Ok(Err(e)) => Ok(ToolResult::failure(self.name(), &e.to_string(), -1)),
            Err(_) => Ok(ToolResult::failure(
                self.name(),
                &format!("命令执行超时（{}秒）", timeout_secs),
                -1,
            )),
        }
    }
}
