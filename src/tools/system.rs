use async_trait::async_trait;
use anyhow::Result;
use serde_json::json;
use tokio::process::Command;

use super::Tool;
use crate::types::tool::ToolResult;

/// 系统信息查询工具（优先使用这个而非 shell.exec）
pub struct SystemTool;

impl SystemTool {
    pub fn new() -> Self { Self }

    async fn run(&self, cmd: &str) -> String {
        let out = Command::new("sh").arg("-c").arg(cmd).output().await;
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(e) => format!("ERROR: {}", e),
        }
    }
}

#[async_trait]
impl Tool for SystemTool {
    fn name(&self) -> &str { "system.info" }

    fn description(&self) -> &str {
        "查询系统信息：磁盘/内存/CPU/进程/用户/网络/服务状态。\
         优先使用此工具而不是 shell.exec 执行查询命令。"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "enum": ["disk", "memory", "cpu", "process", "user", "network", "service", "os"],
                    "description": "查询类型"
                },
                "filter": {
                    "type": "string",
                    "description": "可选过滤关键词，例如进程名、用户名、服务名"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult> {
        let query = args["query"].as_str()
            .ok_or_else(|| anyhow::anyhow!("缺少 query 参数"))?;
        let filter = args["filter"].as_str().unwrap_or("");

        if dry_run {
            return Ok(ToolResult::dry_run_preview(
                self.name(),
                &format!("将查询系统信息: {} (filter: {})", query, filter),
            ));
        }

        let output = match query {
            "disk" => self.run("df -h").await,
            "memory" => self.run("free -h && echo '---' && cat /proc/meminfo | grep -E 'MemTotal|MemFree|MemAvailable|Cached'").await,
            "cpu" => self.run("uptime && echo '---' && nproc && cat /proc/cpuinfo | grep 'model name' | head -1").await,
            "process" => {
                if filter.is_empty() {
                    self.run("ps aux --sort=-%mem | head -20").await
                } else {
                    self.run(&format!("ps aux | grep -v grep | grep '{}'", filter)).await
                }
            }
            "user" => {
                if filter.is_empty() {
                    self.run("cut -d: -f1,3,7 /etc/passwd | grep -v nologin | grep -v false").await
                } else {
                    self.run(&format!("id {} 2>&1 && last {} | head -5", filter, filter)).await
                }
            }
            "network" => self.run("ss -tlnp && echo '---' && ip -br addr").await,
            "service" => {
                if filter.is_empty() {
                    self.run("systemctl list-units --type=service --state=running --no-pager | head -20").await
                } else {
                    self.run(&format!("systemctl status {} --no-pager", filter)).await
                }
            }
            "os" => self.run("uname -a && echo '---' && cat /etc/os-release 2>/dev/null || cat /etc/redhat-release 2>/dev/null").await,
            _ => return Err(anyhow::anyhow!("不支持的查询类型: {}", query)),
        };

        Ok(ToolResult::success(self.name(), &output, 0))
    }
}
