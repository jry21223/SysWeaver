use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;

use super::Tool;
use crate::types::tool::ToolResult;

/// 系统信息查询工具（优先使用这个而非 shell.exec）
pub struct SystemTool;

impl SystemTool {
    pub fn new() -> Self {
        Self
    }

    async fn run(&self, cmd: &str) -> String {
        let out = Command::new("sh").arg("-c").arg(cmd).output().await;
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(e) => format!("ERROR: {}", e),
        }
    }

    /// 验证 filter 参数只包含安全字符（防止 shell 注入）
    fn validate_filter(filter: &str) -> Result<()> {
        if filter
            .chars()
            .all(|c| c.is_alphanumeric() || "-_.@:".contains(c))
        {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "filter 参数包含非法字符，只允许字母、数字和 -_.@:"
            ))
        }
    }
}

#[async_trait]
impl Tool for SystemTool {
    fn name(&self) -> &str {
        "system.info"
    }

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
                    "description": "可选过滤关键词，例如进程名、用户名、服务名（仅字母数字和 -_.@:）"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult> {
        let query = args["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("缺少 query 参数"))?;
        let filter = args["filter"].as_str().unwrap_or("");

        // 验证 filter 防止 shell 注入（仅在 filter 非空时）
        if !filter.is_empty() {
            Self::validate_filter(filter)
                .map_err(|e| anyhow::anyhow!("filter 参数校验失败: {}", e))?;
        }

        if dry_run {
            return Ok(ToolResult::dry_run_preview(
                self.name(),
                &format!("将查询系统信息: {} (filter: {})", query, filter),
            ));
        }

        // 对 filter 涉及的命令，使用 Command::new 直接传参避免注入
        let output = match query {
            "disk" => self.run("df -h").await,
            "memory" => {
                if cfg!(target_os = "macos") {
                    self.run(
                        "PAGE=$(sysctl -n hw.pagesize); TOTAL=$(sysctl -n hw.memsize); \
                         vm_stat | awk -v page=$PAGE -v total=$TOTAL \
                         '/Pages active/{gsub(/\\./, \"\", $3); a=$3+0} \
                          /Pages wired down/{gsub(/\\./, \"\", $4); w=$4+0} \
                          /Pages occupied by compressor/{gsub(/\\./, \"\", $5); c=$5+0} \
                          END{ \
                            used=(a+w+c)*page; free=total-used; \
                            printf \"Total RAM : %.1f GB\\nUsed      : %.1f GB\\nFree      : %.1f GB\\nUsage     : %.1f%%\\n\", \
                              total/1073741824, used/1073741824, free/1073741824, used/total*100 \
                          }'",
                    ).await
                } else {
                    self.run(
                        "free -h && echo '---' && cat /proc/meminfo \
                         | grep -E 'MemTotal|MemFree|MemAvailable|Cached'",
                    ).await
                }
            }
            "cpu" => {
                if cfg!(target_os = "macos") {
                    self.run(
                        "uptime && echo '---' \
                         && sysctl -n hw.logicalcpu \
                         && (sysctl -n machdep.cpu.brand_string 2>/dev/null || sysctl -n hw.model)",
                    ).await
                } else {
                    self.run(
                        "uptime && echo '---' && nproc \
                         && cat /proc/cpuinfo | grep 'model name' | head -1",
                    ).await
                }
            }
            "process" => {
                if filter.is_empty() {
                    if cfg!(target_os = "macos") {
                        // macOS ps 不支持 --sort 参数，改用 sort 管道
                        self.run("ps aux | sort -rnk 4 | head -20").await
                    } else {
                        self.run("ps aux --sort=-%mem | head -20").await
                    }
                } else {
                    let out = Command::new("ps")
                        .arg("aux")
                        .output()
                        .await
                        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                        .unwrap_or_default();
                    out.lines()
                        .filter(|l| l.contains(filter))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            "user" => {
                if filter.is_empty() {
                    self.run("cut -d: -f1,3,7 /etc/passwd | grep -v nologin | grep -v false")
                        .await
                } else {
                    let id_out = Command::new("id")
                        .arg(filter)
                        .output()
                        .await
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .unwrap_or_else(|e| format!("ERROR: {}", e));
                    let last_out = Command::new("last")
                        .arg(filter)
                        .output()
                        .await
                        .map(|o| {
                            String::from_utf8_lossy(&o.stdout)
                                .lines()
                                .take(5)
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default();
                    format!("{}\n---\n{}", id_out, last_out)
                }
            }
            "network" => {
                if cfg!(target_os = "macos") {
                    self.run(
                        "netstat -an | grep LISTEN | head -20 \
                         && echo '---' \
                         && ifconfig | grep -E 'inet [0-9]'",
                    ).await
                } else {
                    self.run("ss -tlnp && echo '---' && ip -br addr").await
                }
            }
            "service" => {
                if cfg!(target_os = "macos") {
                    if filter.is_empty() {
                        self.run(
                            "launchctl list 2>/dev/null \
                             | awk 'NF>=3 && $1~/^[0-9]+$/{print $1\"\\t\"$3}' \
                             | head -20",
                        ).await
                    } else {
                        // launchctl list 过滤指定服务名（直接通过 awk 匹配避免注入）
                        let out = Command::new("launchctl")
                            .arg("list")
                            .output()
                            .await
                            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                            .unwrap_or_default();
                        out.lines()
                            .filter(|l| l.contains(filter))
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                } else if filter.is_empty() {
                    self.run(
                        "systemctl list-units --type=service \
                         --state=running --no-pager | head -20",
                    ).await
                } else {
                    Command::new("systemctl")
                        .arg("status")
                        .arg(filter)
                        .arg("--no-pager")
                        .output()
                        .await
                        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                        .unwrap_or_else(|e| format!("ERROR: {}", e))
                }
            }
            "os" => {
                if cfg!(target_os = "macos") {
                    self.run("uname -a && echo '---' && sw_vers").await
                } else {
                    self.run(
                        "uname -a && echo '---' \
                         && cat /etc/os-release 2>/dev/null \
                         || cat /etc/redhat-release 2>/dev/null",
                    ).await
                }
            }
            _ => return Err(anyhow::anyhow!("不支持的查询类型: {}", query)),
        };

        Ok(ToolResult::success(self.name(), &output, 0))
    }
}
