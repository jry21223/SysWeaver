use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;

use super::Tool;
use crate::types::tool::ToolResult;

/// 进程管理工具 — 列出/查找/终止进程
pub struct ProcessTool;

impl ProcessTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ProcessTool {
    fn name(&self) -> &str {
        "process.manage"
    }

    fn description(&self) -> &str {
        "管理系统进程：列出进程、查找进程、终止进程。\
         终止进程（kill）属于高风险操作，会经过风险审查。"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "find", "kill", "info"],
                    "description": "操作类型：list=列出所有进程, find=按名称/PID查找, kill=终止进程, info=查看进程详情"
                },
                "filter": {
                    "type": "string",
                    "description": "过滤关键词（进程名或用户名），用于 list/find 操作"
                },
                "pid": {
                    "type": "integer",
                    "description": "进程 PID，用于 kill/info 操作"
                },
                "signal": {
                    "type": "string",
                    "enum": ["TERM", "KILL", "HUP", "INT"],
                    "description": "终止信号，默认 TERM（优雅终止）"
                },
                "sort_by": {
                    "type": "string",
                    "enum": ["cpu", "memory", "pid"],
                    "description": "排序方式，默认按内存降序"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("缺少 action 参数"))?;

        match action {
            "list" => {
                if dry_run {
                    return Ok(ToolResult::dry_run_preview(self.name(), "将列出所有系统进程（按内存降序 top-20）"));
                }
                let sort_by = args["sort_by"].as_str().unwrap_or("memory");
                let output = list_processes(sort_by, args["filter"].as_str()).await;
                Ok(ToolResult::success(self.name(), &output, 0))
            }
            "find" => {
                let filter = args["filter"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("find 操作需要提供 filter 参数"))?;
                validate_filter(filter)?;

                if dry_run {
                    return Ok(ToolResult::dry_run_preview(self.name(), &format!("将查找包含 '{}' 的进程", filter)));
                }
                let output = find_process(filter).await;
                Ok(ToolResult::success(self.name(), &output, 0))
            }
            "kill" => {
                let pid = args["pid"]
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("kill 操作需要提供 pid 参数"))?;
                let signal = args["signal"].as_str().unwrap_or("TERM");

                if dry_run {
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        &format!("将向 PID {} 发送 {} 信号", pid, signal),
                    ));
                }

                let output = Command::new("kill")
                    .arg(format!("-{}", signal))
                    .arg(pid.to_string())
                    .output()
                    .await;

                match output {
                    Ok(o) if o.status.success() => Ok(ToolResult::success(
                        self.name(),
                        &format!("已向 PID {} 发送 {} 信号", pid, signal),
                        0,
                    )),
                    Ok(o) => {
                        let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                        Ok(ToolResult::failure(self.name(), &stderr, o.status.code().unwrap_or(-1)))
                    }
                    Err(e) => Ok(ToolResult::failure(self.name(), &e.to_string(), -1)),
                }
            }
            "info" => {
                let pid = args["pid"]
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("info 操作需要提供 pid 参数"))?;

                if dry_run {
                    return Ok(ToolResult::dry_run_preview(self.name(), &format!("将查看 PID {} 的详细信息", pid)));
                }

                let out = Command::new("ps")
                    .args(["-p", &pid.to_string(), "-o", "pid,ppid,user,stat,%cpu,%mem,command"])
                    .output()
                    .await
                    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                    .unwrap_or_else(|e| format!("ERROR: {}", e));

                Ok(ToolResult::success(self.name(), &out, 0))
            }
            _ => Err(anyhow::anyhow!("不支持的操作: {}", action)),
        }
    }
}

fn validate_filter(filter: &str) -> Result<()> {
    if filter.chars().all(|c| c.is_alphanumeric() || "-_.@:/".contains(c)) {
        Ok(())
    } else {
        Err(anyhow::anyhow!("filter 包含非法字符，只允许字母、数字和 -_.@:/"))
    }
}

async fn list_processes(sort_by: &str, filter: Option<&str>) -> String {
    let raw = if cfg!(target_os = "macos") {
        let sort_flag = match sort_by {
            "cpu" => "-r -k 3",
            "pid" => "-k 1",
            _ => "-r -k 4", // memory
        };
        Command::new("sh")
            .arg("-c")
            .arg(format!("ps aux | sort -n{} | head -20", sort_flag))
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default()
    } else {
        let sort_flag = match sort_by {
            "cpu" => "--%cpu",
            "pid" => "pid",
            _ => "-%mem",
        };
        Command::new("ps")
            .args(["aux", &format!("--sort={}", sort_flag)])
            .output()
            .await
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .take(21)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    };

    match filter {
        Some(f) => raw
            .lines()
            .filter(|l| l.to_lowercase().contains(&f.to_lowercase()))
            .collect::<Vec<_>>()
            .join("\n"),
        None => raw,
    }
}

async fn find_process(name: &str) -> String {
    let all = Command::new("ps")
        .args(["aux"])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let matches: Vec<&str> = all
        .lines()
        .filter(|l| l.to_lowercase().contains(&name.to_lowercase()))
        .collect();

    if matches.is_empty() {
        format!("未找到包含 '{}' 的进程", name)
    } else {
        let header = all.lines().next().unwrap_or("USER PID %CPU %MEM COMMAND");
        format!("{}\n{}", header, matches.join("\n"))
    }
}
