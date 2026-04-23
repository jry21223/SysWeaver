use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;

use super::Tool;
use crate::types::tool::ToolResult;

/// 定时任务管理工具 — 查看、添加、删除系统定时任务
pub struct CronTool;

impl CronTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CronTool {
    fn name(&self) -> &str {
        "cron.manage"
    }

    fn description(&self) -> &str {
        "管理系统定时任务（crontab）：列出、添加、删除定时任务。\
         删除任务属于高风险操作，添加/修改属于中等风险，会经过风险审查。\
         支持查询当前用户和系统级（/etc/cron.d/）定时任务。"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "add", "remove", "show-system"],
                    "description": "操作：list=列出当前用户 crontab, add=添加定时任务, remove=删除包含关键词的任务, show-system=查看系统级定时任务(/etc/cron*)"
                },
                "schedule": {
                    "type": "string",
                    "description": "cron 时间表达式（add 时必填），如 '0 2 * * *' 表示每天凌晨2点"
                },
                "command": {
                    "type": "string",
                    "description": "要执行的命令（add 时必填）"
                },
                "keyword": {
                    "type": "string",
                    "description": "关键词（remove 时必填），删除包含该关键词的任务行"
                },
                "user": {
                    "type": "string",
                    "description": "用户名（可选，默认当前用户）"
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
                let user = args["user"].as_str();
                if dry_run {
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        &format!("将列出{}的 crontab 任务", user.unwrap_or("当前用户")),
                    ));
                }
                list_crontab(user).await
            }

            "add" => {
                let schedule = args["schedule"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("add 操作需要 schedule 参数（cron 表达式）"))?;
                let command = args["command"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("add 操作需要 command 参数"))?;

                validate_cron_schedule(schedule)?;
                validate_cron_command(command)?;

                let new_entry = format!("{} {}", schedule, command);

                if dry_run {
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        &format!("将添加定时任务: {}", new_entry),
                    ));
                }

                let user = args["user"].as_str();
                add_cron_entry(&new_entry, user).await
            }

            "remove" => {
                let keyword = args["keyword"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("remove 操作需要 keyword 参数"))?;
                validate_keyword(keyword)?;

                if dry_run {
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        &format!("将删除包含 '{}' 的定时任务（高风险操作）", keyword),
                    ));
                }

                let user = args["user"].as_str();
                remove_cron_entry(keyword, user).await
            }

            "show-system" => {
                if dry_run {
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        "将查看系统级定时任务（/etc/cron.d/, /etc/cron.daily/ 等）",
                    ));
                }
                show_system_cron().await
            }

            _ => Ok(ToolResult::failure(
                self.name(),
                &format!("不支持的操作: {}", action),
                1,
            )),
        }
    }
}

/// 列出 crontab
async fn list_crontab(user: Option<&str>) -> Result<ToolResult> {
    let start = std::time::Instant::now();
    let mut cmd = Command::new("crontab");
    cmd.arg("-l");
    if let Some(u) = user {
        cmd.args(["-u", u]);
    }

    let output = cmd.output().await.map_err(|e| anyhow::anyhow!("执行 crontab 失败: {}", e))?;
    let duration_ms = start.elapsed().as_millis() as u64;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    // crontab -l 在无任务时返回非零，视为成功（空列表）
    let effective_stdout = if stdout.trim().is_empty() && exit_code != 0 {
        "（当前无定时任务）".to_string()
    } else {
        stdout
    };

    Ok(ToolResult {
        success: true,
        tool: "cron.manage".to_string(),
        stdout: effective_stdout,
        stderr,
        exit_code: 0,
        duration_ms,
        dry_run_preview: None,
    })
}

/// 添加一条 cron 条目
async fn add_cron_entry(entry: &str, user: Option<&str>) -> Result<ToolResult> {
    let start = std::time::Instant::now();

    // 读取现有 crontab
    let mut list_cmd = Command::new("crontab");
    list_cmd.arg("-l");
    if let Some(u) = user {
        list_cmd.args(["-u", u]);
    }
    let existing = list_cmd.output().await.ok();
    let existing_content = existing
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    // 检查是否已存在
    if existing_content.contains(entry) {
        return Ok(ToolResult::success(
            "cron.manage",
            &format!("定时任务已存在，无需重复添加:\n{}", entry),
            0,
        ));
    }

    let new_content = if existing_content.trim().is_empty() {
        format!("{}\n", entry)
    } else {
        let trimmed = existing_content.trim_end();
        format!("{}\n{}\n", trimmed, entry)
    };

    // 通过管道写入新 crontab
    let mut write_cmd = tokio::process::Command::new("crontab");
    write_cmd.stdin(std::process::Stdio::piped());
    if let Some(u) = user {
        write_cmd.args(["-u", u]);
    }
    write_cmd.arg("-");

    let mut child = write_cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("启动 crontab 失败: {}", e))?;

    use tokio::io::AsyncWriteExt;
    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        stdin
            .write_all(new_content.as_bytes())
            .await
            .map_err(|e| anyhow::anyhow!("写入 crontab 失败: {}", e))?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| anyhow::anyhow!("等待 crontab 完成失败: {}", e))?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let exit_code = output.status.code().unwrap_or(-1);

    if output.status.success() {
        Ok(ToolResult {
            success: true,
            tool: "cron.manage".to_string(),
            stdout: format!("已成功添加定时任务:\n{}", entry),
            stderr: String::new(),
            exit_code,
            duration_ms,
            dry_run_preview: None,
        })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Ok(ToolResult::failure("cron.manage", &format!("添加失败: {}", stderr), exit_code))
    }
}

/// 删除包含关键词的 cron 条目
async fn remove_cron_entry(keyword: &str, user: Option<&str>) -> Result<ToolResult> {
    let start = std::time::Instant::now();

    // 读取现有 crontab
    let mut list_cmd = Command::new("crontab");
    list_cmd.arg("-l");
    if let Some(u) = user {
        list_cmd.args(["-u", u]);
    }
    let existing = list_cmd.output().await.map_err(|e| anyhow::anyhow!("读取 crontab 失败: {}", e))?;
    let existing_content = String::from_utf8_lossy(&existing.stdout).to_string();

    // 过滤掉包含 keyword 的行
    let lines: Vec<&str> = existing_content
        .lines()
        .filter(|line| !line.contains(keyword))
        .collect();

    let removed_count = existing_content
        .lines()
        .filter(|line| line.contains(keyword) && !line.trim_start().starts_with('#'))
        .count();

    if removed_count == 0 {
        return Ok(ToolResult::success(
            "cron.manage",
            &format!("未找到包含 '{}' 的定时任务", keyword),
            0,
        ));
    }

    let new_content = lines.join("\n") + "\n";

    // 写回 crontab
    let mut write_cmd = tokio::process::Command::new("crontab");
    write_cmd.stdin(std::process::Stdio::piped());
    if let Some(u) = user {
        write_cmd.args(["-u", u]);
    }
    write_cmd.arg("-");

    let mut child = write_cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("启动 crontab 失败: {}", e))?;

    use tokio::io::AsyncWriteExt;
    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        stdin
            .write_all(new_content.as_bytes())
            .await
            .map_err(|e| anyhow::anyhow!("写入 crontab 失败: {}", e))?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| anyhow::anyhow!("等待 crontab 完成失败: {}", e))?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let exit_code = output.status.code().unwrap_or(-1);

    if output.status.success() {
        Ok(ToolResult {
            success: true,
            tool: "cron.manage".to_string(),
            stdout: format!("已删除 {} 条包含 '{}' 的定时任务", removed_count, keyword),
            stderr: String::new(),
            exit_code,
            duration_ms,
            dry_run_preview: None,
        })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Ok(ToolResult::failure("cron.manage", &format!("删除失败: {}", stderr), exit_code))
    }
}

/// 查看系统级定时任务
async fn show_system_cron() -> Result<ToolResult> {
    let start = std::time::Instant::now();

    let mut output_parts: Vec<String> = vec![];

    // /etc/crontab
    if let Ok(o) = Command::new("cat").arg("/etc/crontab").output().await {
        if o.status.success() && !o.stdout.is_empty() {
            output_parts.push(format!(
                "=== /etc/crontab ===\n{}",
                String::from_utf8_lossy(&o.stdout)
            ));
        }
    }

    // /etc/cron.d/
    if let Ok(o) = Command::new("ls").arg("-la").arg("/etc/cron.d/").output().await {
        if o.status.success() {
            output_parts.push(format!(
                "=== /etc/cron.d/ ===\n{}",
                String::from_utf8_lossy(&o.stdout)
            ));
        }
    }

    // /etc/cron.daily/ etc.
    for dir in &["cron.daily", "cron.weekly", "cron.monthly", "cron.hourly"] {
        let path = format!("/etc/{}/", dir);
        if let Ok(o) = Command::new("ls").arg("-la").arg(&path).output().await {
            if o.status.success() && !o.stdout.is_empty() {
                output_parts.push(format!(
                    "=== {} ===\n{}",
                    path,
                    String::from_utf8_lossy(&o.stdout)
                ));
            }
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    let combined = if output_parts.is_empty() {
        "未发现系统级定时任务配置文件".to_string()
    } else {
        output_parts.join("\n")
    };

    Ok(ToolResult {
        success: true,
        tool: "cron.manage".to_string(),
        stdout: combined,
        stderr: String::new(),
        exit_code: 0,
        duration_ms,
        dry_run_preview: None,
    })
}

/// 校验 cron 时间表达式
fn validate_cron_schedule(schedule: &str) -> Result<()> {
    let parts: Vec<&str> = schedule.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(anyhow::anyhow!(
            "cron 表达式必须有 5 个字段（分 时 日 月 周），当前: '{}'",
            schedule
        ));
    }
    // 检查每个字段只包含合法字符
    for part in &parts {
        let valid = part.chars().all(|c| c.is_ascii_digit() || matches!(c, '*' | '/' | '-' | ','));
        if !valid {
            return Err(anyhow::anyhow!("cron 表达式字段包含非法字符: '{}'", part));
        }
    }
    Ok(())
}

/// 校验 cron 命令（防止明显注入）
fn validate_cron_command(cmd: &str) -> Result<()> {
    if cmd.trim().is_empty() {
        return Err(anyhow::anyhow!("cron 命令不能为空"));
    }
    // 阻止 fork bomb
    if cmd.contains(":(){") || cmd.contains(":(){ ") {
        return Err(anyhow::anyhow!("检测到潜在危险命令模式"));
    }
    Ok(())
}

/// 校验关键词（防止注入）
fn validate_keyword(keyword: &str) -> Result<()> {
    if keyword.trim().is_empty() {
        return Err(anyhow::anyhow!("关键词不能为空"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_cron_schedules() {
        assert!(validate_cron_schedule("* * * * *").is_ok());
        assert!(validate_cron_schedule("0 2 * * *").is_ok());
        assert!(validate_cron_schedule("0 2 1 * *").is_ok());
        assert!(validate_cron_schedule("*/5 * * * *").is_ok());
        assert!(validate_cron_schedule("0 8-17 * * 1-5").is_ok());
        assert!(validate_cron_schedule("0,30 * * * *").is_ok());
    }

    #[test]
    fn rejects_invalid_cron_schedule_wrong_field_count() {
        assert!(validate_cron_schedule("* * *").is_err());
        assert!(validate_cron_schedule("* * * * * *").is_err());
        assert!(validate_cron_schedule("").is_err());
    }

    #[test]
    fn rejects_cron_schedule_with_illegal_chars() {
        assert!(validate_cron_schedule("0 2 * * $(cmd)").is_err());
        assert!(validate_cron_schedule("0 2 * * ; rm").is_err());
    }

    #[test]
    fn accepts_valid_cron_commands() {
        assert!(validate_cron_command("/usr/bin/backup.sh").is_ok());
        assert!(validate_cron_command("find /tmp -mtime +7 -delete").is_ok());
        assert!(validate_cron_command("systemctl reload nginx").is_ok());
    }

    #[test]
    fn rejects_empty_cron_command() {
        assert!(validate_cron_command("").is_err());
        assert!(validate_cron_command("   ").is_err());
    }

    #[test]
    fn rejects_fork_bomb_in_command() {
        assert!(validate_cron_command(":(){:|:&};:").is_err());
    }

    #[test]
    fn rejects_empty_keyword() {
        assert!(validate_keyword("").is_err());
        assert!(validate_keyword("   ").is_err());
    }

    #[test]
    fn cron_tool_has_correct_name() {
        assert_eq!(CronTool::new().name(), "cron.manage");
    }

    #[tokio::test]
    async fn dry_run_list_returns_preview() {
        let tool = CronTool::new();
        let args = serde_json::json!({"action": "list"});
        let result = tool.execute(&args, true).await.unwrap();
        assert!(result.dry_run_preview.is_some());
    }

    #[tokio::test]
    async fn dry_run_add_returns_preview() {
        let tool = CronTool::new();
        let args = serde_json::json!({
            "action": "add",
            "schedule": "0 2 * * *",
            "command": "/usr/bin/backup.sh"
        });
        let result = tool.execute(&args, true).await.unwrap();
        assert!(result.dry_run_preview.is_some());
        let preview = result.dry_run_preview.unwrap();
        assert!(preview.contains("0 2 * * *"));
        assert!(preview.contains("backup.sh"));
    }

    #[tokio::test]
    async fn dry_run_remove_returns_preview() {
        let tool = CronTool::new();
        let args = serde_json::json!({"action": "remove", "keyword": "backup"});
        let result = tool.execute(&args, true).await.unwrap();
        assert!(result.dry_run_preview.is_some());
        let preview = result.dry_run_preview.unwrap();
        assert!(preview.contains("backup"));
    }

    #[tokio::test]
    async fn add_without_schedule_returns_error() {
        let tool = CronTool::new();
        let args = serde_json::json!({"action": "add", "command": "/bin/backup.sh"});
        let result = tool.execute(&args, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn add_with_invalid_schedule_returns_error() {
        let tool = CronTool::new();
        let args = serde_json::json!({
            "action": "add",
            "schedule": "every day",
            "command": "/bin/backup.sh"
        });
        let result = tool.execute(&args, false).await;
        assert!(result.is_err());
    }
}
