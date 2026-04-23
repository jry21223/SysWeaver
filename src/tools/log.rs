use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;

use super::Tool;
use crate::types::tool::ToolResult;

/// 合法的日志源白名单
const ALLOWED_LOG_PATHS: &[&str] = &[
    "/var/log/syslog",
    "/var/log/messages",
    "/var/log/auth.log",
    "/var/log/kern.log",
    "/var/log/dmesg",
    "/var/log/nginx/access.log",
    "/var/log/nginx/error.log",
    "/var/log/apache2/access.log",
    "/var/log/apache2/error.log",
    "/var/log/httpd/access_log",
    "/var/log/httpd/error_log",
    "/var/log/mysql/error.log",
    "/var/log/postgresql/postgresql.log",
    "/var/log/redis/redis-server.log",
];

/// 验证日志路径是否在白名单内（防止读取敏感文件）
fn is_allowed_log_path(path: &str) -> bool {
    ALLOWED_LOG_PATHS.iter().any(|&p| path == p)
        || path.starts_with("/var/log/")
}

/// 验证 filter 只包含安全字符
fn validate_filter(filter: &str) -> Result<()> {
    let ok = filter.chars().all(|c| {
        c.is_alphanumeric() || " -_.@:/[]()=".contains(c)
    });
    if ok {
        Ok(())
    } else {
        Err(anyhow::anyhow!("filter 包含非法字符，只允许字母数字及常见符号"))
    }
}

pub struct LogTailTool;

impl LogTailTool {
    pub fn new() -> Self {
        Self
    }

    async fn tail_journalctl(unit: &str, lines: u64, filter: &str) -> String {
        // journalctl -u <unit> -n <lines> --no-pager
        let mut cmd = Command::new("journalctl");
        cmd.arg("--no-pager").arg("-n").arg(lines.to_string());
        if !unit.is_empty() {
            cmd.arg("-u").arg(unit);
        }
        if !filter.is_empty() {
            cmd.arg("--grep").arg(filter);
        }
        match cmd.output().await {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                if stdout.trim().is_empty() && !stderr.trim().is_empty() {
                    stderr
                } else {
                    stdout
                }
            }
            Err(e) => format!("journalctl 不可用: {}", e),
        }
    }

    async fn tail_file(path: &str, lines: u64, filter: &str) -> String {
        if filter.is_empty() {
            // tail -n <lines> <path>
            match Command::new("tail")
                .arg("-n")
                .arg(lines.to_string())
                .arg(path)
                .output()
                .await
            {
                Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
                Err(e) => format!("读取日志失败: {}", e),
            }
        } else {
            // tail -n <lines*10> <path> | grep <filter> | tail -n <lines>
            // Use grep via Command to avoid shell injection
            let tail_out = Command::new("tail")
                .arg("-n")
                .arg((lines * 20).to_string())
                .arg(path)
                .output()
                .await;

            match tail_out {
                Ok(out) => {
                    let content = String::from_utf8_lossy(&out.stdout);
                    // In-process filter (safe, no shell injection)
                    let matched: Vec<&str> = content
                        .lines()
                        .filter(|l| l.contains(filter))
                        .collect();
                    let total = matched.len();
                    matched
                        .into_iter()
                        .rev()
                        .take(lines as usize)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>()
                        .join("\n")
                        + &if total == 0 {
                            format!("\n（未找到包含 '{}' 的日志行）", filter)
                        } else {
                            String::new()
                        }
                }
                Err(e) => format!("读取日志失败: {}", e),
            }
        }
    }
}

#[async_trait]
impl Tool for LogTailTool {
    fn name(&self) -> &str {
        "log.tail"
    }

    fn description(&self) -> &str {
        "读取系统或服务日志的最新条目。支持 journalctl（systemd 日志）\
         和常见日志文件（/var/log/syslog、nginx、apache 等）。\
         可按服务名过滤，适合诊断服务异常、查看错误信息。"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "日志来源。可以是：\
                        'journalctl'（systemd 系统日志，推荐）；\
                        服务单元名如 'nginx'、'sshd'（journalctl -u 过滤）；\
                        或文件路径如 '/var/log/syslog'（必须在 /var/log/ 下）",
                    "default": "journalctl"
                },
                "lines": {
                    "type": "integer",
                    "description": "读取最新行数，默认 50，最多 500",
                    "default": 50
                },
                "filter": {
                    "type": "string",
                    "description": "可选关键词过滤（大小写敏感），例如 'error'、'WARN'、'nginx'"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult> {
        let source = args["source"].as_str().unwrap_or("journalctl");
        let lines = args["lines"].as_u64().unwrap_or(50).min(500).max(1);
        let filter = args["filter"].as_str().unwrap_or("");

        if !filter.is_empty() {
            validate_filter(filter)
                .map_err(|e| anyhow::anyhow!("filter 校验失败: {}", e))?;
        }

        if dry_run {
            return Ok(ToolResult::dry_run_preview(
                self.name(),
                &format!("将读取日志：source={}, lines={}, filter={:?}", source, lines, filter),
            ));
        }

        let output = if source == "journalctl" {
            // Generic system journal
            Self::tail_journalctl("", lines, filter).await
        } else if source.starts_with('/') {
            // File path
            if !is_allowed_log_path(source) {
                return Ok(ToolResult::failure(
                    self.name(),
                    &format!("安全限制：只能读取 /var/log/ 下的日志文件，拒绝访问: {}", source),
                    -1,
                ));
            }
            Self::tail_file(source, lines, filter).await
        } else {
            // Treat as journalctl service unit name (e.g. "nginx", "sshd")
            Self::tail_journalctl(source, lines, filter).await
        };

        let trimmed = output.trim();
        if trimmed.is_empty() {
            Ok(ToolResult::success(self.name(), "（日志为空或无匹配内容）", 0))
        } else {
            Ok(ToolResult::success(self.name(), trimmed, 0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_allowed_log_path, validate_filter};

    #[test]
    fn allows_standard_var_log_paths() {
        assert!(is_allowed_log_path("/var/log/syslog"));
        assert!(is_allowed_log_path("/var/log/nginx/error.log"));
        assert!(is_allowed_log_path("/var/log/custom/app.log"));
    }

    #[test]
    fn rejects_paths_outside_var_log() {
        assert!(!is_allowed_log_path("/etc/passwd"));
        assert!(!is_allowed_log_path("/home/user/secret.txt"));
        assert!(!is_allowed_log_path("/tmp/log.txt"));
    }

    #[test]
    fn accepts_safe_filter_strings() {
        assert!(validate_filter("error").is_ok());
        assert!(validate_filter("nginx ERROR 404").is_ok());
        assert!(validate_filter("user@host").is_ok());
    }

    #[test]
    fn rejects_filter_with_shell_metacharacters() {
        assert!(validate_filter("'; rm -rf /").is_err());
        assert!(validate_filter("`whoami`").is_err());
        assert!(validate_filter("$(id)").is_err());
    }

    #[test]
    fn empty_filter_is_always_valid() {
        assert!(validate_filter("").is_ok());
    }
}
