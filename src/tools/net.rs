use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;

use super::Tool;
use crate::types::tool::ToolResult;

/// 验证 host/IP：只允许字母、数字、点、连字符（防止注入）
fn validate_host(host: &str) -> Result<()> {
    if host.is_empty() {
        return Err(anyhow::anyhow!("host 不能为空"));
    }
    let ok = host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if ok {
        Ok(())
    } else {
        Err(anyhow::anyhow!("host 包含非法字符，只允许字母数字和 .-_"))
    }
}

/// 验证端口号（1–65535）
fn validate_port(port: u64) -> Result<()> {
    if port == 0 || port > 65535 {
        Err(anyhow::anyhow!("port 必须在 1–65535 范围内，当前: {}", port))
    } else {
        Ok(())
    }
}

pub struct NetCheckTool;

impl NetCheckTool {
    pub fn new() -> Self {
        Self
    }

    async fn ping(host: &str, count: u32) -> String {
        let count_str = count.to_string();
        // macOS: -c count; Linux: -c count; -W timeout=2s
        let out = if cfg!(target_os = "macos") {
            Command::new("ping")
                .arg("-c")
                .arg(&count_str)
                .arg("-W")
                .arg("2000") // milliseconds on macOS
                .arg(host)
                .output()
                .await
        } else {
            Command::new("ping")
                .arg("-c")
                .arg(&count_str)
                .arg("-W")
                .arg("2") // seconds on Linux
                .arg(host)
                .output()
                .await
        };

        match out {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                if stdout.trim().is_empty() {
                    stderr
                } else {
                    stdout
                }
            }
            Err(e) => format!("ping 命令不可用: {}", e),
        }
    }

    async fn port_check(host: &str, port: u16) -> String {
        // Try nc first, fall back to bash /dev/tcp
        let nc_out = Command::new("nc")
            .arg("-zv")
            .arg("-w")
            .arg("3")
            .arg(host)
            .arg(port.to_string())
            .output()
            .await;

        match nc_out {
            Ok(o) => {
                let combined = format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                if o.status.success() {
                    format!("✅ {}:{} 端口开放\n{}", host, port, combined.trim())
                } else {
                    format!("❌ {}:{} 端口未响应\n{}", host, port, combined.trim())
                }
            }
            Err(_) => {
                // Fallback: use bash /dev/tcp (available on most Linux)
                let bash_out = Command::new("bash")
                    .arg("-c")
                    .arg(format!(
                        "timeout 3 bash -c 'echo >/dev/tcp/{}/{} && echo open || echo closed'",
                        host, port
                    ))
                    .output()
                    .await;
                match bash_out {
                    Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
                    Err(e) => format!("端口检查失败（nc 和 bash 均不可用）: {}", e),
                }
            }
        }
    }

    async fn dns_lookup(host: &str) -> String {
        // Try dig, then nslookup, then getent
        let dig_out = Command::new("dig")
            .arg("+short")
            .arg(host)
            .output()
            .await;

        if let Ok(o) = dig_out {
            let result = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !result.is_empty() {
                return format!("DNS 解析结果 ({}):\n{}", host, result);
            }
        }

        // fallback: nslookup
        let ns_out = Command::new("nslookup")
            .arg(host)
            .output()
            .await;

        match ns_out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
            Err(_) => {
                // Last resort: getent hosts
                match Command::new("getent").arg("hosts").arg(host).output().await {
                    Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
                    Err(e) => format!("DNS 查询失败: {}", e),
                }
            }
        }
    }

    async fn route_check() -> String {
        // Show default route and primary interface
        if cfg!(target_os = "macos") {
            let out = Command::new("netstat")
                .arg("-rn")
                .output()
                .await;
            match out {
                Ok(o) => {
                    let text = String::from_utf8_lossy(&o.stdout);
                    // Only show first 20 lines to avoid clutter
                    text.lines().take(20).collect::<Vec<_>>().join("\n")
                }
                Err(e) => format!("路由查询失败: {}", e),
            }
        } else {
            match Command::new("ip").arg("route").output().await {
                Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
                Err(_) => match Command::new("route").arg("-n").output().await {
                    Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
                    Err(e) => format!("路由查询失败: {}", e),
                },
            }
        }
    }
}

#[async_trait]
impl Tool for NetCheckTool {
    fn name(&self) -> &str {
        "net.check"
    }

    fn description(&self) -> &str {
        "网络诊断工具。支持 ping 连通性测试、端口开放检测、DNS 解析、路由查看。\
         适合排查网络问题、验证服务可达性、检查 DNS 配置。"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["ping", "port", "dns", "route"],
                    "description": "操作类型：ping=连通测试, port=端口检测, dns=DNS解析, route=路由查看"
                },
                "host": {
                    "type": "string",
                    "description": "目标主机名或 IP 地址（ping/port/dns 时必填）"
                },
                "port": {
                    "type": "integer",
                    "description": "端口号 1-65535（action=port 时必填）"
                },
                "count": {
                    "type": "integer",
                    "description": "ping 发包数量，默认 4，最多 10",
                    "default": 4
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("缺少 action 参数"))?;

        if dry_run {
            return Ok(ToolResult::dry_run_preview(
                self.name(),
                &format!("将执行网络诊断：action={}", action),
            ));
        }

        let output = match action {
            "ping" => {
                let host = args["host"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("ping 操作需要 host 参数"))?;
                validate_host(host)?;
                let count = args["count"].as_u64().unwrap_or(4).min(10).max(1) as u32;
                Self::ping(host, count).await
            }
            "port" => {
                let host = args["host"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("port 操作需要 host 参数"))?;
                validate_host(host)?;
                let port = args["port"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("port 操作需要 port 参数"))?;
                validate_port(port)?;
                Self::port_check(host, port as u16).await
            }
            "dns" => {
                let host = args["host"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("dns 操作需要 host 参数"))?;
                validate_host(host)?;
                Self::dns_lookup(host).await
            }
            "route" => Self::route_check().await,
            _ => return Err(anyhow::anyhow!("不支持的 action: {}", action)),
        };

        let trimmed = output.trim();
        if trimmed.is_empty() {
            Ok(ToolResult::success(self.name(), "（无输出）", 0))
        } else {
            Ok(ToolResult::success(self.name(), trimmed, 0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_host, validate_port};

    #[test]
    fn accepts_valid_hostnames_and_ips() {
        assert!(validate_host("localhost").is_ok());
        assert!(validate_host("192.168.1.1").is_ok());
        assert!(validate_host("google.com").is_ok());
        assert!(validate_host("my-server.example.com").is_ok());
    }

    #[test]
    fn rejects_host_with_shell_metacharacters() {
        assert!(validate_host("host; rm -rf /").is_err());
        assert!(validate_host("`whoami`").is_err());
        assert!(validate_host("$(id)").is_err());
        assert!(validate_host("host|cat /etc/passwd").is_err());
    }

    #[test]
    fn rejects_empty_host() {
        assert!(validate_host("").is_err());
    }

    #[test]
    fn accepts_valid_port_range() {
        assert!(validate_port(1).is_ok());
        assert!(validate_port(80).is_ok());
        assert!(validate_port(65535).is_ok());
    }

    #[test]
    fn rejects_out_of_range_ports() {
        assert!(validate_port(0).is_err());
        assert!(validate_port(65536).is_err());
        assert!(validate_port(u64::MAX).is_err());
    }
}
