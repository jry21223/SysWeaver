use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;

use super::Tool;
use crate::types::tool::ToolResult;

/// 服务管理工具 — 查看/启动/停止/重启系统服务
pub struct ServiceTool;

impl ServiceTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ServiceTool {
    fn name(&self) -> &str {
        "service.manage"
    }

    fn description(&self) -> &str {
        "管理系统服务（systemctl/launchctl）：查看状态、启动、停止、重启服务。\
         停止/重启操作属于中等风险，停止 SSH 等关键服务属于高风险，会经过风险审查。"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "start", "stop", "restart", "enable", "disable", "list"],
                    "description": "操作类型：status=查看状态, start=启动, stop=停止, restart=重启, enable=开机自启, disable=禁用自启, list=列出所有服务"
                },
                "service": {
                    "type": "string",
                    "description": "服务名称（如 nginx、sshd、mysql）"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("缺少 action 参数"))?;

        if action == "list" {
            if dry_run {
                return Ok(ToolResult::dry_run_preview(self.name(), "将列出所有运行中的服务"));
            }
            let output = list_services().await;
            return Ok(ToolResult::success(self.name(), &output, 0));
        }

        let service = args["service"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("此操作需要提供 service 参数"))?;

        validate_service_name(service)?;

        if dry_run {
            return Ok(ToolResult::dry_run_preview(
                self.name(),
                &format!("将对服务 '{}' 执行 {} 操作", service, action),
            ));
        }

        match action {
            "status" => {
                let output = get_service_status(service).await;
                Ok(ToolResult::success(self.name(), &output, 0))
            }
            "start" | "stop" | "restart" | "enable" | "disable" => {
                run_service_command(self.name(), service, action).await
            }
            _ => Err(anyhow::anyhow!("不支持的操作: {}", action)),
        }
    }
}

fn validate_service_name(name: &str) -> Result<()> {
    if name.chars().all(|c| c.is_alphanumeric() || "-_.@".contains(c)) {
        Ok(())
    } else {
        Err(anyhow::anyhow!("服务名包含非法字符"))
    }
}

async fn list_services() -> String {
    if cfg!(target_os = "macos") {
        Command::new("sh")
            .arg("-c")
            .arg("launchctl list 2>/dev/null | awk 'NF>=3 && $1~/^[0-9]+$/{print $1\"\\t\"$3}' | head -20")
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_else(|e| format!("ERROR: {}", e))
    } else {
        Command::new("systemctl")
            .args(["list-units", "--type=service", "--state=running", "--no-pager", "--plain"])
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_else(|e| format!("ERROR: {}", e))
    }
}

async fn get_service_status(service: &str) -> String {
    if cfg!(target_os = "macos") {
        let out = Command::new("launchctl")
            .args(["list"])
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        let matches: Vec<&str> = out.lines().filter(|l| l.contains(service)).collect();
        if matches.is_empty() {
            format!("服务 '{}' 未找到或未运行", service)
        } else {
            matches.join("\n")
        }
    } else {
        Command::new("systemctl")
            .args(["status", service, "--no-pager", "-l"])
            .output()
            .await
            .map(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                if stdout.is_empty() { stderr } else { stdout }
            })
            .unwrap_or_else(|e| format!("ERROR: {}", e))
    }
}

async fn run_service_command(tool_name: &str, service: &str, action: &str) -> Result<ToolResult> {
    let result = if cfg!(target_os = "macos") {
        // macOS: launchctl load/unload for start/stop
        let subcommand = match action {
            "start" => "start",
            "stop" => "stop",
            "restart" => "kickstart",
            _ => return Err(anyhow::anyhow!("macOS 不支持 {}", action)),
        };
        Command::new("launchctl")
            .args([subcommand, service])
            .output()
            .await
    } else {
        Command::new("systemctl")
            .args([action, service])
            .output()
            .await
    };

    match result {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            if o.status.success() {
                let status_out = get_service_status(service).await;
                let summary = format!(
                    "服务 '{}' 已成功执行 {} 操作\n\n当前状态：\n{}",
                    service, action, status_out
                );
                Ok(ToolResult::success(tool_name, &summary, 0))
            } else {
                let msg = if stderr.is_empty() { stdout } else { stderr };
                Ok(ToolResult::failure(tool_name, &msg, o.status.code().unwrap_or(-1)))
            }
        }
        Err(e) => Ok(ToolResult::failure(tool_name, &e.to_string(), -1)),
    }
}
