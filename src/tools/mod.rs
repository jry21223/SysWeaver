pub mod cron;
pub mod file;
pub mod health;
pub mod log;
pub mod net;
pub mod package;
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
                Box::new(log::LogTailTool::new()),
                Box::new(net::NetCheckTool::new()),
                Box::new(package::PackageTool::new()),
                Box::new(cron::CronTool::new()),
                Box::new(health::HealthCheckTool::new()),
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
    pub fn ssh_config(&self) -> Option<&SshConfig> {
        self.ssh.as_deref()
    }

    /// 根据 ToolCall 分发到对应工具执行
    pub async fn dispatch(&self, call: &ToolCall) -> Result<ToolResult> {
        // SSH 远程模式：将所有执行 OS 命令的工具路由到远端
        if let Some(ssh) = &self.ssh {
            match call.tool.as_str() {
                "shell.exec"     => return dispatch_ssh_shell(ssh, call).await,
                "system.info"    => return dispatch_ssh_system(ssh, call).await,
                "process.manage" => return dispatch_ssh_process(ssh, call).await,
                "service.manage" => return dispatch_ssh_service(ssh, call).await,
                "log.tail"       => return dispatch_ssh_log(ssh, call).await,
                "net.check"      => return dispatch_ssh_net(ssh, call).await,
                _ => {}
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

/// SSH 模式下执行 process.manage
async fn dispatch_ssh_process(ssh: &SshConfig, call: &ToolCall) -> Result<ToolResult> {
    if call.dry_run {
        let action = call.args["action"].as_str().unwrap_or("?");
        return Ok(ToolResult::dry_run_preview(
            "process.manage",
            &format!("[SSH:{}] 将执行 process.manage action={}", ssh.display(), action),
        ));
    }

    let action = call.args["action"].as_str().unwrap_or("");
    let cmd = match action {
        "list" => {
            let sort_by = call.args["sort_by"].as_str().unwrap_or("memory");
            let sort_flag = match sort_by {
                "cpu"    => "--sort=-%cpu",
                "pid"    => "--sort=pid",
                _        => "--sort=-%mem",
            };
            format!("ps aux {} 2>/dev/null | head -21 || ps aux 2>/dev/null | head -21", sort_flag)
        }
        "find" => {
            let filter = call.args["filter"].as_str().unwrap_or("");
            format!("ps aux 2>/dev/null | grep -i '{}' | grep -v grep", filter)
        }
        "kill" => {
            let pid    = call.args["pid"].as_i64().unwrap_or(0);
            let signal = call.args["signal"].as_str().unwrap_or("TERM");
            format!("kill -{} {} 2>&1", signal, pid)
        }
        "info" => {
            let pid = call.args["pid"].as_i64().unwrap_or(0);
            format!("ps -p {} -o pid,ppid,user,stat,%cpu,%mem,command 2>/dev/null || ps aux 2>/dev/null | awk '$2=={}'", pid, pid)
        }
        _ => return Ok(ToolResult::failure("process.manage", &format!("不支持的操作: {}", action), -1)),
    };

    match ssh.exec(&cmd).await {
        Ok((stdout, stderr, exit_code, duration_ms)) => Ok(ToolResult {
            success: exit_code == 0 || !stdout.is_empty(),
            tool: "process.manage".to_string(),
            stdout: if stdout.is_empty() { stderr.clone() } else { stdout },
            stderr,
            exit_code,
            duration_ms,
            dry_run_preview: None,
        }),
        Err(e) => Ok(ToolResult::failure("process.manage", &e.to_string(), -1)),
    }
}

/// SSH 模式下执行 service.manage
async fn dispatch_ssh_service(ssh: &SshConfig, call: &ToolCall) -> Result<ToolResult> {
    if call.dry_run {
        let action = call.args["action"].as_str().unwrap_or("?");
        return Ok(ToolResult::dry_run_preview(
            "service.manage",
            &format!("[SSH:{}] 将执行 service.manage action={}", ssh.display(), action),
        ));
    }

    let action  = call.args["action"].as_str().unwrap_or("");
    let service = call.args["service"].as_str().unwrap_or("");

    let cmd = match action {
        "list" => "systemctl list-units --type=service --state=running --no-pager --plain 2>/dev/null | head -20 || launchctl list 2>/dev/null | head -20".to_string(),
        "status" => format!(
            "systemctl status {} --no-pager -l 2>/dev/null || launchctl list | grep '{}'",
            service, service
        ),
        "start" | "stop" | "restart" | "enable" | "disable" => format!(
            "systemctl {} {} 2>&1 || launchctl {} {} 2>&1",
            action, service, action, service
        ),
        _ => return Ok(ToolResult::failure("service.manage", &format!("不支持的操作: {}", action), -1)),
    };

    match ssh.exec(&cmd).await {
        Ok((stdout, stderr, exit_code, duration_ms)) => Ok(ToolResult {
            success: exit_code == 0 || !stdout.is_empty(),
            tool: "service.manage".to_string(),
            stdout: if stdout.is_empty() { stderr.clone() } else { stdout },
            stderr,
            exit_code,
            duration_ms,
            dry_run_preview: None,
        }),
        Err(e) => Ok(ToolResult::failure("service.manage", &e.to_string(), -1)),
    }
}

/// SSH 模式下执行 log.tail
async fn dispatch_ssh_log(ssh: &SshConfig, call: &ToolCall) -> Result<ToolResult> {
    if call.dry_run {
        return Ok(ToolResult::dry_run_preview(
            "log.tail",
            &format!("[SSH:{}] 将读取远端日志", ssh.display()),
        ));
    }

    let path  = call.args["path"].as_str().unwrap_or("/var/log/syslog");
    let lines = call.args["lines"].as_i64().unwrap_or(50).min(500);
    let cmd   = format!("tail -n {} {} 2>&1", lines, path);

    match ssh.exec(&cmd).await {
        Ok((stdout, stderr, exit_code, duration_ms)) => Ok(ToolResult {
            success: exit_code == 0,
            tool: "log.tail".to_string(),
            stdout: if stdout.is_empty() { stderr.clone() } else { stdout },
            stderr,
            exit_code,
            duration_ms,
            dry_run_preview: None,
        }),
        Err(e) => Ok(ToolResult::failure("log.tail", &e.to_string(), -1)),
    }
}

/// SSH 模式下执行 net.check
async fn dispatch_ssh_net(ssh: &SshConfig, call: &ToolCall) -> Result<ToolResult> {
    if call.dry_run {
        return Ok(ToolResult::dry_run_preview(
            "net.check",
            &format!("[SSH:{}] 将检查远端网络", ssh.display()),
        ));
    }

    let action = call.args["action"].as_str().unwrap_or("ports");
    let cmd = match action {
        "ports"  => "ss -tlnp 2>/dev/null || netstat -tlnp 2>/dev/null || lsof -iTCP -sTCP:LISTEN 2>/dev/null".to_string(),
        "ping"   => {
            let host = call.args["host"].as_str().unwrap_or("8.8.8.8");
            format!("ping -c 3 {} 2>&1", host)
        }
        "dns"    => {
            let host = call.args["host"].as_str().unwrap_or("google.com");
            format!("nslookup {} 2>&1 || dig {} 2>&1", host, host)
        }
        _        => "ss -tlnp 2>/dev/null || netstat -tlnp 2>/dev/null".to_string(),
    };

    match ssh.exec(&cmd).await {
        Ok((stdout, stderr, exit_code, duration_ms)) => Ok(ToolResult {
            success: exit_code == 0 || !stdout.is_empty(),
            tool: "net.check".to_string(),
            stdout: if stdout.is_empty() { stderr.clone() } else { stdout },
            stderr,
            exit_code,
            duration_ms,
            dry_run_preview: None,
        }),
        Err(e) => Ok(ToolResult::failure("net.check", &e.to_string(), -1)),
    }
}
