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
    name.replacen('_', ".", 1)
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
        // SSH 远程模式：将所有访问操作系统状态/副作用的工具路由到远端
        // 只有不会触碰系统资源的纯逻辑工具（当前没有）才允许 fall-through 到本地
        if let Some(ssh) = &self.ssh {
            match call.tool.as_str() {
                "shell.exec"     => return dispatch_ssh_shell(ssh, call).await,
                "system.info"    => return dispatch_ssh_system(ssh, call).await,
                "process.manage" => return dispatch_ssh_process(ssh, call).await,
                "service.manage" => return dispatch_ssh_service(ssh, call).await,
                "log.tail"       => return dispatch_ssh_log(ssh, call).await,
                "net.check"      => return dispatch_ssh_net(ssh, call).await,
                "file.read"      => return dispatch_ssh_file_read(ssh, call).await,
                "file.write"     => return dispatch_ssh_file_write(ssh, call).await,
                "file.search"    => return dispatch_ssh_file_search(ssh, call).await,
                "user.manage"    => return dispatch_ssh_user(ssh, call).await,
                "package.manage" => return dispatch_ssh_package(ssh, call).await,
                "cron.manage"    => return dispatch_ssh_cron(ssh, call).await,
                "health.check"   => return dispatch_ssh_health(ssh, call).await,
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

/// Maps the raw `ssh.exec` result to a `ToolResult`.
/// `lenient=true` treats non-empty stdout as success even when exit_code != 0,
/// which is needed for commands that mix useful output with non-zero exit (e.g. grep, ps).
fn ssh_exec_to_result(
    tool: &str,
    raw: anyhow::Result<(String, String, i32, u64)>,
    lenient: bool,
) -> Result<ToolResult> {
    match raw {
        Ok((stdout, stderr, exit_code, duration_ms)) => {
            let success = exit_code == 0 || (lenient && !stdout.is_empty());
            Ok(ToolResult {
                success,
                tool: tool.to_string(),
                stdout: if stdout.is_empty() { stderr.clone() } else { stdout },
                stderr,
                exit_code,
                duration_ms,
                dry_run_preview: None,
            })
        }
        Err(e) => Ok(ToolResult::failure(tool, &e.to_string(), -1)),
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

    ssh_exec_to_result("system.info", ssh.exec(cmd).await, true)
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

    ssh_exec_to_result("process.manage", ssh.exec(&cmd).await, true)
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

    ssh_exec_to_result("service.manage", ssh.exec(&cmd).await, true)
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

    ssh_exec_to_result("log.tail", ssh.exec(&cmd).await, false)
}

/// 受保护路径前缀（与 tools/file.rs 保持一致；远程模式不能依赖本地 canonicalize）
const REMOTE_PROTECTED_PREFIXES: &[&str] = &[
    "/etc/passwd", "/etc/shadow", "/etc/sudoers", "/etc/ssh/",
    "/boot/", "/dev/", "/proc/", "/sys/",
];

fn is_remote_protected_path(path: &str) -> bool {
    REMOTE_PROTECTED_PREFIXES.iter().any(|p| path.starts_with(p))
}

/// 把任意字符串包装成 POSIX 单引号字面量，转义内部单引号 → '\''
fn shell_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' { out.push_str("'\\''"); } else { out.push(ch); }
    }
    out.push('\'');
    out
}

/// SSH 模式下执行 file.read（远端读取文件，head/tail）
async fn dispatch_ssh_file_read(ssh: &SshConfig, call: &ToolCall) -> Result<ToolResult> {
    let raw_path = call.args["path"].as_str()
        .ok_or_else(|| anyhow::anyhow!("缺少 path 参数"))?;
    let max_lines = call.args["max_lines"].as_u64().unwrap_or(200).min(10_000);
    let tail = call.args["tail"].as_bool().unwrap_or(false);

    if call.dry_run {
        return Ok(ToolResult::dry_run_preview(
            "file.read",
            &format!("[SSH:{}] 将读取远端文件: {}", ssh.display(), raw_path),
        ));
    }

    if is_remote_protected_path(raw_path) {
        return Ok(ToolResult::failure(
            "file.read",
            &format!("拒绝读取受保护路径: {}", raw_path), -1,
        ));
    }

    let p = shell_single_quote(raw_path);
    let cmd = if tail {
        format!("tail -n {} {} 2>&1", max_lines, p)
    } else {
        format!("head -n {} {} 2>&1", max_lines, p)
    };

    match ssh.exec(&cmd).await {
        Ok((stdout, stderr, exit_code, duration_ms)) => Ok(ToolResult {
            success: exit_code == 0,
            tool: "file.read".to_string(),
            stdout: if stdout.is_empty() { stderr.clone() } else { stdout },
            stderr,
            exit_code,
            duration_ms,
            dry_run_preview: None,
        }),
        Err(e) => Ok(ToolResult::failure("file.read", &e.to_string(), -1)),
    }
}

/// SSH 模式下执行 file.write（base64 编码内容后远端解码写入）
async fn dispatch_ssh_file_write(ssh: &SshConfig, call: &ToolCall) -> Result<ToolResult> {
    let raw_path = call.args["path"].as_str()
        .ok_or_else(|| anyhow::anyhow!("缺少 path 参数"))?;
    let content = call.args["content"].as_str()
        .ok_or_else(|| anyhow::anyhow!("缺少 content 参数"))?;
    let mode = call.args["mode"].as_str().unwrap_or("overwrite");

    if is_remote_protected_path(raw_path) {
        return Ok(ToolResult::failure(
            "file.write",
            &format!("拒绝写入受保护路径: {}。请通过 shell.exec 并经安全确认后操作。", raw_path),
            -1,
        ));
    }

    if call.dry_run {
        let preview_len = content.chars().count().min(100);
        let preview: String = content.chars().take(preview_len).collect();
        return Ok(ToolResult::dry_run_preview(
            "file.write",
            &format!(
                "[SSH:{}] 将{}写入远端文件: {}\n内容预览（前100字）: {}",
                ssh.display(),
                if mode == "append" { "追加" } else { "覆盖" },
                raw_path, preview
            ),
        ));
    }

    use base64::{engine::general_purpose::STANDARD, Engine};
    let encoded = STANDARD.encode(content.as_bytes());
    let p = shell_single_quote(raw_path);
    let redirect = if mode == "append" { ">>" } else { ">" };

    // 覆盖写时：先 cp 备份到 /tmp/.sysweaver-bak-<ts>，给 Undo 留路径
    let cmd = if mode != "append" {
        format!(
            "set -e; mkdir -p \"$(dirname {0})\"; \
             BAK=\"\"; if [ -f {0} ]; then \
               BAK=/tmp/.sysweaver-bak-$$-$(date +%s); cp {0} \"$BAK\" 2>/dev/null || BAK=\"\"; \
             fi; \
             printf %s {1} | base64 -d {2} {0} && \
             if [ -n \"$BAK\" ]; then echo \"[BACKUP:$BAK]\"; fi",
            p, shell_single_quote(&encoded), redirect
        )
    } else {
        format!(
            "set -e; mkdir -p \"$(dirname {0})\"; \
             printf %s {1} | base64 -d {2} {0}",
            p, shell_single_quote(&encoded), redirect
        )
    };

    match ssh.exec(&cmd).await {
        Ok((stdout, stderr, exit_code, duration_ms)) => {
            if exit_code == 0 {
                let summary = format!(
                    "已{}写入 {} 字节到远端 {} {}",
                    if mode == "append" { "追加" } else { "覆盖" },
                    content.len(),
                    raw_path,
                    stdout.trim(),
                );
                Ok(ToolResult {
                    success: true,
                    tool: "file.write".to_string(),
                    stdout: summary,
                    stderr,
                    exit_code,
                    duration_ms,
                    dry_run_preview: None,
                })
            } else {
                Ok(ToolResult {
                    success: false,
                    tool: "file.write".to_string(),
                    stdout,
                    stderr,
                    exit_code,
                    duration_ms,
                    dry_run_preview: None,
                })
            }
        }
        Err(e) => Ok(ToolResult::failure("file.write", &e.to_string(), -1)),
    }
}

/// SSH 模式下执行 file.search（远端 grep / find）
async fn dispatch_ssh_file_search(ssh: &SshConfig, call: &ToolCall) -> Result<ToolResult> {
    let pattern = call.args["pattern"].as_str()
        .ok_or_else(|| anyhow::anyhow!("缺少 pattern 参数"))?;
    let path = call.args["path"].as_str().unwrap_or(".");
    let mode = call.args["mode"].as_str().unwrap_or("content");
    let max_results = call.args["max_results"].as_u64().unwrap_or(50).min(1000);

    if call.dry_run {
        return Ok(ToolResult::dry_run_preview(
            "file.search",
            &format!(
                "[SSH:{}] 将在 {} 中{}搜索: {}",
                ssh.display(), path,
                if mode == "filename" { "按文件名" } else { "按内容" },
                pattern
            ),
        ));
    }

    let p = shell_single_quote(path);
    let pat = shell_single_quote(pattern);

    let cmd = if mode == "filename" {
        format!("find {} -name {} 2>/dev/null | head -n {}", p, pat, max_results)
    } else {
        format!(
            "grep -rn --include='*.txt' --include='*.log' --include='*.conf' \
             --include='*.cfg' --include='*.json' --include='*.yaml' --include='*.yml' \
             --include='*.sh' --include='*.py' --include='*.js' --include='*.ts' \
             -m {} {} {} 2>/dev/null | head -n {}",
            max_results, pat, p, max_results
        )
    };

    match ssh.exec(&cmd).await {
        Ok((stdout, stderr, _exit_code, duration_ms)) => {
            let result = if stdout.trim().is_empty() {
                format!("未找到匹配 '{}' 的结果", pattern)
            } else {
                stdout
            };
            Ok(ToolResult {
                success: true,
                tool: "file.search".to_string(),
                stdout: result,
                stderr,
                exit_code: 0,
                duration_ms,
                dry_run_preview: None,
            })
        }
        Err(e) => Ok(ToolResult::failure("file.search", &e.to_string(), -1)),
    }
}

/// 用户名校验（与 tools/user.rs::validate_username 保持一致）
fn validate_username_remote(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 32 {
        return Err(anyhow::anyhow!("用户名长度非法（1-32 字符）"));
    }
    if name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
        Ok(())
    } else {
        Err(anyhow::anyhow!("用户名包含非法字符"))
    }
}

/// SSH 模式下执行 user.manage
async fn dispatch_ssh_user(ssh: &SshConfig, call: &ToolCall) -> Result<ToolResult> {
    let action = call.args["action"].as_str()
        .ok_or_else(|| anyhow::anyhow!("缺少 action 参数"))?;

    if call.dry_run {
        return Ok(ToolResult::dry_run_preview(
            "user.manage",
            &format!("[SSH:{}] 将执行 user.manage action={}", ssh.display(), action),
        ));
    }

    let cmd = match action {
        "list" => {
            "awk -F: '$3>=1000 && $1!=\"nobody\" {print $1\"\\t\"$3\"\\t\"$6\"\\t\"$7}' /etc/passwd 2>/dev/null \
             || dscl . list /Users 2>/dev/null | grep -v '^_'".to_string()
        }
        "info" => {
            let username = call.args["username"].as_str()
                .ok_or_else(|| anyhow::anyhow!("info 操作需要 username"))?;
            validate_username_remote(username)?;
            let u = shell_single_quote(username);
            format!(
                "id {0} 2>&1; echo '---'; \
                 grep \"^{1}:\" /etc/passwd 2>/dev/null; echo '---'; \
                 last -n 3 {0} 2>/dev/null",
                u, username.replace('"', "")
            )
        }
        "create" => {
            let username = call.args["username"].as_str()
                .ok_or_else(|| anyhow::anyhow!("create 操作需要 username"))?;
            validate_username_remote(username)?;
            let shell = call.args["shell"].as_str().unwrap_or("/bin/bash");
            let comment = call.args["comment"].as_str().unwrap_or(username);
            let groups: Vec<String> = call.args["groups"].as_array()
                .map(|g| g.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            for g in &groups {
                if !g.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) {
                    return Err(anyhow::anyhow!("用户组名包含非法字符: {}", g));
                }
            }
            let mut parts = vec![
                "useradd".to_string(),
                "-m".to_string(),
                "-s".to_string(), shell_single_quote(shell),
                "-c".to_string(), shell_single_quote(comment),
            ];
            if !groups.is_empty() {
                parts.push("-G".to_string());
                parts.push(shell_single_quote(&groups.join(",")));
            }
            parts.push(shell_single_quote(username));
            format!("{} 2>&1", parts.join(" "))
        }
        "delete" => {
            let username = call.args["username"].as_str()
                .ok_or_else(|| anyhow::anyhow!("delete 操作需要 username"))?;
            validate_username_remote(username)?;
            format!("userdel -r {} 2>&1", shell_single_quote(username))
        }
        "passwd" => {
            return Ok(ToolResult::failure(
                "user.manage",
                "SSH 模式下不支持 passwd（需要交互式输入），请改用 shell.exec 配合 chpasswd。",
                -1,
            ));
        }
        _ => return Ok(ToolResult::failure("user.manage", &format!("不支持的操作: {}", action), -1)),
    };

    match ssh.exec(&cmd).await {
        Ok((stdout, stderr, exit_code, duration_ms)) => Ok(ToolResult {
            success: exit_code == 0,
            tool: "user.manage".to_string(),
            stdout: if stdout.is_empty() { stderr.clone() } else { stdout },
            stderr,
            exit_code,
            duration_ms,
            dry_run_preview: None,
        }),
        Err(e) => Ok(ToolResult::failure("user.manage", &e.to_string(), -1)),
    }
}

/// 包名校验（与 tools/package.rs::validate_package_name 保持一致）
fn validate_package_name_remote(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow::anyhow!("包名不能为空"));
    }
    if name.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '.' | '+' | '_' | ':' | '=')) {
        Ok(())
    } else {
        Err(anyhow::anyhow!("包名包含非法字符: {}", name))
    }
}

/// SSH 模式下执行 package.manage（远端包管理器）
async fn dispatch_ssh_package(ssh: &SshConfig, call: &ToolCall) -> Result<ToolResult> {
    let action = call.args["action"].as_str()
        .ok_or_else(|| anyhow::anyhow!("缺少 action 参数"))?;
    let manager_hint = call.args["manager"].as_str().unwrap_or("auto");

    if call.dry_run {
        let pkg = call.args["package"].as_str().unwrap_or("");
        return Ok(ToolResult::dry_run_preview(
            "package.manage",
            &format!("[SSH:{}] 将执行 package.manage action={} package={}", ssh.display(), action, pkg),
        ));
    }

    // 远程包管理器自动检测：通过 SSH 一次性探测可用的包管理器
    let manager = if manager_hint == "auto" {
        let probe = "for c in apt-get dnf yum pacman brew apk; do \
                     command -v $c >/dev/null 2>&1 && { echo $c; break; }; done";
        match ssh.exec(probe).await {
            Ok((stdout, _, _, _)) => match stdout.trim() {
                "apt-get" => "apt".to_string(),
                "dnf"     => "dnf".to_string(),
                "yum"     => "yum".to_string(),
                "pacman"  => "pacman".to_string(),
                "brew"    => "brew".to_string(),
                "apk"     => "apk".to_string(),
                _         => "unknown".to_string(),
            },
            Err(_) => "unknown".to_string(),
        }
    } else {
        manager_hint.to_string()
    };

    if manager == "unknown" {
        return Ok(ToolResult::failure(
            "package.manage",
            "远端未检测到支持的包管理器（apt/yum/dnf/pacman/brew/apk）",
            1,
        ));
    }

    let needs_pkg = matches!(action, "install" | "remove" | "search" | "info");
    let pkg = if needs_pkg {
        let p = call.args["package"].as_str()
            .ok_or_else(|| anyhow::anyhow!("{} 操作需要 package 参数", action))?;
        validate_package_name_remote(p)?;
        Some(p)
    } else {
        None
    };

    let cmd: String = match (action, pkg) {
        ("install", Some(p)) => match manager.as_str() {
            "apt"    => format!("apt-get -y install {} 2>&1", shell_single_quote(p)),
            "dnf"    => format!("dnf -y install {} 2>&1", shell_single_quote(p)),
            "yum"    => format!("yum -y install {} 2>&1", shell_single_quote(p)),
            "pacman" => format!("pacman --noconfirm -S {} 2>&1", shell_single_quote(p)),
            "brew"   => format!("brew install {} 2>&1", shell_single_quote(p)),
            "apk"    => format!("apk add {} 2>&1", shell_single_quote(p)),
            _ => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
        },
        ("remove", Some(p)) => match manager.as_str() {
            "apt"    => format!("apt-get -y remove {} 2>&1", shell_single_quote(p)),
            "dnf"    => format!("dnf -y remove {} 2>&1", shell_single_quote(p)),
            "yum"    => format!("yum -y remove {} 2>&1", shell_single_quote(p)),
            "pacman" => format!("pacman --noconfirm -R {} 2>&1", shell_single_quote(p)),
            "brew"   => format!("brew uninstall {} 2>&1", shell_single_quote(p)),
            "apk"    => format!("apk del {} 2>&1", shell_single_quote(p)),
            _ => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
        },
        ("search", Some(p)) => match manager.as_str() {
            "apt"    => format!("apt-cache search {} 2>&1", shell_single_quote(p)),
            "dnf"    => format!("dnf search {} 2>&1", shell_single_quote(p)),
            "yum"    => format!("yum search {} 2>&1", shell_single_quote(p)),
            "pacman" => format!("pacman -Ss {} 2>&1", shell_single_quote(p)),
            "brew"   => format!("brew search {} 2>&1", shell_single_quote(p)),
            "apk"    => format!("apk search {} 2>&1", shell_single_quote(p)),
            _ => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
        },
        ("info", Some(p)) => match manager.as_str() {
            "apt"    => format!("apt-cache show {} 2>&1", shell_single_quote(p)),
            "dnf"    => format!("dnf info {} 2>&1", shell_single_quote(p)),
            "yum"    => format!("yum info {} 2>&1", shell_single_quote(p)),
            "pacman" => format!("pacman -Si {} 2>&1", shell_single_quote(p)),
            "brew"   => format!("brew info {} 2>&1", shell_single_quote(p)),
            "apk"    => format!("apk info -a {} 2>&1", shell_single_quote(p)),
            _ => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
        },
        ("list-installed", _) => match manager.as_str() {
            "apt"    => "dpkg --get-selections 2>&1".to_string(),
            "dnf"    => "dnf list installed 2>&1".to_string(),
            "yum"    => "yum list installed 2>&1".to_string(),
            "pacman" => "pacman -Q 2>&1".to_string(),
            "brew"   => "brew list 2>&1".to_string(),
            "apk"    => "apk list --installed 2>&1".to_string(),
            _ => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
        },
        ("update-cache", _) => match manager.as_str() {
            "apt"    => "apt-get update 2>&1".to_string(),
            "dnf"    => "dnf check-update 2>&1".to_string(),
            "yum"    => "yum check-update 2>&1".to_string(),
            "pacman" => "pacman -Sy 2>&1".to_string(),
            "brew"   => "brew update 2>&1".to_string(),
            "apk"    => "apk update 2>&1".to_string(),
            _ => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
        },
        ("upgrade-all", _) => match manager.as_str() {
            "apt"    => "apt-get -y upgrade 2>&1".to_string(),
            "dnf"    => "dnf -y upgrade 2>&1".to_string(),
            "yum"    => "yum -y update 2>&1".to_string(),
            "pacman" => "pacman --noconfirm -Su 2>&1".to_string(),
            "brew"   => "brew upgrade 2>&1".to_string(),
            "apk"    => "apk upgrade 2>&1".to_string(),
            _ => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
        },
        _ => return Ok(ToolResult::failure("package.manage", &format!("不支持的操作: {}", action), 1)),
    };

    match ssh.exec(&cmd).await {
        Ok((stdout, stderr, exit_code, duration_ms)) => Ok(ToolResult {
            success: exit_code == 0,
            tool: "package.manage".to_string(),
            stdout: if stdout.is_empty() { stderr.clone() } else { stdout },
            stderr,
            exit_code,
            duration_ms,
            dry_run_preview: None,
        }),
        Err(e) => Ok(ToolResult::failure("package.manage", &e.to_string(), -1)),
    }
}

/// 校验 cron 表达式（与 tools/cron.rs 保持一致）
fn validate_cron_schedule_remote(schedule: &str) -> Result<()> {
    let parts: Vec<&str> = schedule.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(anyhow::anyhow!("cron 表达式必须有 5 个字段"));
    }
    for part in &parts {
        if !part.chars().all(|c| c.is_ascii_digit() || matches!(c, '*' | '/' | '-' | ',')) {
            return Err(anyhow::anyhow!("cron 字段包含非法字符: '{}'", part));
        }
    }
    Ok(())
}

/// SSH 模式下执行 cron.manage（远端 crontab 操作）
async fn dispatch_ssh_cron(ssh: &SshConfig, call: &ToolCall) -> Result<ToolResult> {
    let action = call.args["action"].as_str()
        .ok_or_else(|| anyhow::anyhow!("缺少 action 参数"))?;
    let user = call.args["user"].as_str();

    if call.dry_run {
        return Ok(ToolResult::dry_run_preview(
            "cron.manage",
            &format!("[SSH:{}] 将执行 cron.manage action={}", ssh.display(), action),
        ));
    }

    // 用户名校验
    if let Some(u) = user {
        if !u.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
            return Err(anyhow::anyhow!("user 参数包含非法字符"));
        }
    }
    let user_flag = user.map(|u| format!("-u {}", shell_single_quote(u))).unwrap_or_default();

    let cmd: String = match action {
        "list" => format!("crontab {} -l 2>&1 || echo '（当前无定时任务）'", user_flag),
        "show-system" => {
            "echo '=== /etc/crontab ==='; cat /etc/crontab 2>/dev/null; \
             echo '=== /etc/cron.d/ ==='; ls -la /etc/cron.d/ 2>/dev/null; \
             for d in cron.daily cron.weekly cron.monthly cron.hourly; do \
               echo \"=== /etc/$d ===\"; ls -la /etc/$d 2>/dev/null; \
             done".to_string()
        }
        "add" => {
            let schedule = call.args["schedule"].as_str()
                .ok_or_else(|| anyhow::anyhow!("add 操作需要 schedule 参数"))?;
            let command = call.args["command"].as_str()
                .ok_or_else(|| anyhow::anyhow!("add 操作需要 command 参数"))?;
            validate_cron_schedule_remote(schedule)?;
            if command.contains(":(){") {
                return Err(anyhow::anyhow!("检测到潜在危险命令模式"));
            }
            let entry = format!("{} {}", schedule, command);
            use base64::{engine::general_purpose::STANDARD, Engine};
            let entry_b64 = STANDARD.encode(entry.as_bytes());
            // 读取现有 → 追加新条目 → 写回（避免重复）
            format!(
                "EX=$(crontab {0} -l 2>/dev/null || true); \
                 NEW=$(printf %s {1} | base64 -d); \
                 if echo \"$EX\" | grep -F -q \"$NEW\"; then \
                   echo '已存在，无需重复添加'; \
                 else \
                   {{ echo \"$EX\"; echo \"$NEW\"; }} | grep -v '^$' | crontab {0} - && echo '已成功添加'; \
                 fi",
                user_flag, shell_single_quote(&entry_b64)
            )
        }
        "remove" => {
            let keyword = call.args["keyword"].as_str()
                .ok_or_else(|| anyhow::anyhow!("remove 操作需要 keyword 参数"))?;
            if keyword.trim().is_empty() {
                return Err(anyhow::anyhow!("关键词不能为空"));
            }
            let kw = shell_single_quote(keyword);
            format!(
                "EX=$(crontab {0} -l 2>/dev/null || true); \
                 N=$(echo \"$EX\" | grep -F {1} | grep -v '^[[:space:]]*#' | wc -l | tr -d '[:space:]'); \
                 if [ \"$N\" = \"0\" ]; then \
                   echo '未找到匹配的定时任务'; \
                 else \
                   echo \"$EX\" | grep -F -v {1} | crontab {0} - && echo \"已删除 $N 条匹配项\"; \
                 fi",
                user_flag, kw
            )
        }
        _ => return Ok(ToolResult::failure("cron.manage", &format!("不支持的操作: {}", action), -1)),
    };

    match ssh.exec(&cmd).await {
        Ok((stdout, stderr, exit_code, duration_ms)) => Ok(ToolResult {
            success: exit_code == 0,
            tool: "cron.manage".to_string(),
            stdout: if stdout.is_empty() { stderr.clone() } else { stdout },
            stderr,
            exit_code,
            duration_ms,
            dry_run_preview: None,
        }),
        Err(e) => Ok(ToolResult::failure("cron.manage", &e.to_string(), -1)),
    }
}

/// SSH 模式下执行 health.check（远端综合诊断）
async fn dispatch_ssh_health(ssh: &SshConfig, call: &ToolCall) -> Result<ToolResult> {
    if call.dry_run {
        return Ok(ToolResult::dry_run_preview(
            "health.check",
            &format!("[SSH:{}] 将运行远端综合健康诊断", ssh.display()),
        ));
    }
    crate::tools::health::execute_remote(&call.args, ssh).await
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

    ssh_exec_to_result("net.check", ssh.exec(&cmd).await, true)
}
