pub mod file;
pub mod process;
pub mod service;
pub mod shell;
pub mod system;
pub mod user;

use crate::executor::ssh::SshConfig;
use crate::types::tool::{ToolCall, ToolResult};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

pub fn to_openai_tool_name(name: &str) -> String {
    name.replace('.', "_")
}

pub fn from_openai_tool_name(name: &str) -> String {
    if name.contains('_') {
        let parts: Vec<&str> = name.splitn(2, '_').collect();
        if parts.len() == 2 {
            format!("{}.{}", parts[0], parts[1])
        } else {
            name.to_string()
        }
    } else {
        name.to_string()
    }
}

pub fn is_valid_openai_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// 所有工具必须实现的统一接口
#[async_trait]
pub trait Tool: Send + Sync {
    /// 工具名称，格式: "namespace.action"
    fn name(&self) -> &str;
    /// 工具功能描述（发送给 LLM）
    fn description(&self) -> &str;
    /// 工具参数的 JSON Schema（发送给 LLM）
    fn schema(&self) -> serde_json::Value;
    /// 执行工具
    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult>;
}

/// 工具分发器
pub struct ToolManager {
    tools: Vec<Box<dyn Tool>>,
    /// SSH 远程执行配置（Some = 远程模式，None = 本地模式）
    ssh: Option<Arc<SshConfig>>,
}

impl ToolManager {
    pub fn new() -> Self {
        Self {
            tools: vec![
                Box::new(shell::ShellTool::new()),
                Box::new(file::FileReadTool::new()),
                Box::new(file::FileWriteTool::new()),
                Box::new(file::FileSearchTool::new()),
                Box::new(system::SystemTool::new()),
                Box::new(process::ProcessTool::new()),
                Box::new(service::ServiceTool::new()),
                Box::new(user::UserTool::new()),
            ],
            ssh: None,
        }
    }

    /// 创建 SSH 远程模式工具管理器
    pub fn with_ssh(ssh: SshConfig) -> Self {
        let mut mgr = Self::new();
        mgr.ssh = Some(Arc::new(ssh));
        mgr
    }

    /// 获取 SSH 配置（供外部查询）
    #[allow(dead_code)]
    pub fn ssh_config(&self) -> Option<&SshConfig> {
        self.ssh.as_deref()
    }

    /// 根据 ToolCall 分发到对应工具执行
    pub async fn dispatch(&self, call: &ToolCall) -> Result<ToolResult> {
        // SSH 远程模式：shell.exec 和 system.info 路由到远程
        if let Some(ssh) = &self.ssh {
            if call.tool == "shell.exec" {
                return dispatch_ssh_shell(ssh, call).await;
            }
            if call.tool == "system.info" {
                return dispatch_ssh_system(ssh, call).await;
            }
        }

        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == call.tool)
            .ok_or_else(|| anyhow::anyhow!("未知工具: {}", call.tool))?;

        tool.execute(&call.args, call.dry_run).await
    }

    /// 生成所有工具的 Schema 列表（用于 LLM tool_use）
    pub fn all_schemas(&self) -> Vec<serde_json::Value> {
        self.tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "input_schema": t.schema(),
                })
            })
            .collect()
    }
}

impl Default for ToolManager {
    fn default() -> Self {
        Self::new()
    }
}

/// SSH 模式下执行 shell.exec
async fn dispatch_ssh_shell(ssh: &SshConfig, call: &ToolCall) -> Result<ToolResult> {
    let command = call.args["command"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("缺少 command 参数"))?;

    if call.dry_run {
        return Ok(ToolResult::dry_run_preview(
            "shell.exec",
            &format!("[SSH:{}] 将执行: `{}`", ssh.display(), command),
        ));
    }

    match ssh.exec(command).await {
        Ok((stdout, stderr, exit_code, duration_ms)) => Ok(ToolResult {
            success: exit_code == 0,
            tool: "shell.exec".to_string(),
            stdout,
            stderr,
            exit_code,
            duration_ms,
            dry_run_preview: None,
        }),
        Err(e) => Ok(ToolResult::failure("shell.exec", &e.to_string(), -1)),
    }
}

/// SSH 模式下执行 system.info（将查询命令路由到远程）
async fn dispatch_ssh_system(ssh: &SshConfig, call: &ToolCall) -> Result<ToolResult> {
    if call.dry_run {
        return Ok(ToolResult::dry_run_preview(
            "system.info",
            &format!("[SSH:{}] 将查询系统信息", ssh.display()),
        ));
    }

    let query = call.args["query"].as_str().unwrap_or("os");
    let cmd = match query {
        "disk"    => "df -h 2>&1",
        "memory"  => "free -h 2>&1 || vm_stat 2>&1",
        "cpu"     => "nproc 2>/dev/null; grep 'model name' /proc/cpuinfo 2>/dev/null | head -1 || sysctl -n machdep.cpu.brand_string 2>/dev/null",
        "process" => "ps aux --sort=-%mem 2>/dev/null | head -15 || ps aux 2>&1 | head -15",
        "user"    => "cut -d: -f1,3,7 /etc/passwd 2>/dev/null | grep -v nologin | head -20 || dscl . -list /Users 2>/dev/null | head -20",
        "network" => "ss -tlnp 2>/dev/null || netstat -tlnp 2>/dev/null || lsof -iTCP -sTCP:LISTEN 2>/dev/null",
        "service" => "systemctl list-units --type=service --state=running --no-pager --no-legend 2>/dev/null | head -15 || launchctl list 2>/dev/null | head -15",
        "os"      => "uname -a 2>&1; cat /etc/os-release 2>/dev/null || sw_vers 2>/dev/null",
        _         => "uname -a",
    };

    match ssh.exec(cmd).await {
        Ok((stdout, stderr, exit_code, duration_ms)) => Ok(ToolResult {
            success: exit_code == 0 || !stdout.is_empty(),
            tool: "system.info".to_string(),
            stdout: if stdout.is_empty() { stderr.clone() } else { stdout },
            stderr,
            exit_code,
            duration_ms,
            dry_run_preview: None,
        }),
        Err(e) => Ok(ToolResult::failure("system.info", &e.to_string(), -1)),
    }
}
