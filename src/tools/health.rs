use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;

use super::Tool;
use crate::executor::ssh::SshConfig;
use crate::types::tool::ToolResult;

/// 系统健康诊断工具 — 综合评估磁盘、内存、CPU、进程、服务和日志
pub struct HealthCheckTool;

impl HealthCheckTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for HealthCheckTool {
    fn name(&self) -> &str {
        "health.check"
    }

    fn description(&self) -> &str {
        "综合系统健康诊断：一键检查磁盘使用率、内存占用、CPU 负载、\
         高资源进程、关键服务状态和近期错误日志，\
         输出带严重等级（OK/WARNING/CRITICAL）的诊断报告。\
         适用于快速了解系统整体健康状态。"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "checks": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["disk", "memory", "cpu", "process", "service", "log", "all"]
                    },
                    "description": "要执行的检查项；默认 all（全部）。可选：disk、memory、cpu、process、service、log"
                },
                "disk_warn_pct": {
                    "type": "integer",
                    "description": "磁盘使用率 WARNING 阈值（百分比），默认 80"
                },
                "disk_crit_pct": {
                    "type": "integer",
                    "description": "磁盘使用率 CRITICAL 阈值（百分比），默认 90"
                },
                "mem_warn_pct": {
                    "type": "integer",
                    "description": "内存使用率 WARNING 阈值（百分比），默认 80"
                },
                "mem_crit_pct": {
                    "type": "integer",
                    "description": "内存使用率 CRITICAL 阈值（百分比），默认 90"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult> {
        if dry_run {
            return Ok(ToolResult::dry_run_preview(
                self.name(),
                "将运行：磁盘/内存/CPU/进程/服务/日志综合诊断，生成健康报告",
            ));
        }
        run_health_check(args, None).await
    }
}

/// SSH 远程模式入口（由 tools::dispatch_ssh_health 调用）。
/// 与本地版本共享 run_health_check 实现，仅 run_cmd 路由到 ssh.exec。
pub(crate) async fn execute_remote(args: &serde_json::Value, ssh: &SshConfig) -> Result<ToolResult> {
    run_health_check(args, Some(ssh)).await
}

async fn run_health_check(args: &serde_json::Value, ssh: Option<&SshConfig>) -> Result<ToolResult> {
    let checks_val = args["checks"].as_array();
    let run_all = checks_val.map_or(true, |arr| {
        arr.is_empty() || arr.iter().any(|v| v.as_str() == Some("all"))
    });
    let want = |name: &str| -> bool {
        run_all || checks_val.map_or(false, |arr| {
            arr.iter().any(|v| v.as_str() == Some(name))
        })
    };

    let disk_warn = args["disk_warn_pct"].as_u64().unwrap_or(80) as u32;
    let disk_crit = args["disk_crit_pct"].as_u64().unwrap_or(90) as u32;
    let mem_warn  = args["mem_warn_pct"].as_u64().unwrap_or(80) as u32;
    let mem_crit  = args["mem_crit_pct"].as_u64().unwrap_or(90) as u32;

    let mut sections: Vec<String> = Vec::new();
    let mut overall_ok = true;
    let mut critical_count = 0u32;
    let mut warning_count  = 0u32;

    if want("disk") {
        let section = check_disk(disk_warn, disk_crit, ssh).await;
        if section.contains("CRITICAL") { critical_count += 1; overall_ok = false; }
        if section.contains("WARNING")  { warning_count  += 1; overall_ok = false; }
        sections.push(section);
    }

    if want("memory") {
        let section = check_memory(mem_warn, mem_crit, ssh).await;
        if section.contains("CRITICAL") { critical_count += 1; overall_ok = false; }
        if section.contains("WARNING")  { warning_count  += 1; overall_ok = false; }
        sections.push(section);
    }

    if want("cpu") {
        let section = check_cpu(ssh).await;
        if section.contains("CRITICAL") { critical_count += 1; overall_ok = false; }
        if section.contains("WARNING")  { warning_count  += 1; overall_ok = false; }
        sections.push(section);
    }

    if want("process") {
        let section = check_top_processes(ssh).await;
        sections.push(section);
    }

    if want("service") {
        let section = check_services(ssh).await;
        if section.contains("CRITICAL") { critical_count += 1; overall_ok = false; }
        if section.contains("WARNING")  { warning_count  += 1; overall_ok = false; }
        sections.push(section);
    }

    if want("log") {
        let section = check_recent_errors(ssh).await;
        if section.contains("CRITICAL") { critical_count += 1; overall_ok = false; }
        if section.contains("WARNING")  { warning_count  += 1; overall_ok = false; }
        sections.push(section);
    }

    let overall_status = if overall_ok {
        "✅ 整体状态：健康 (OK)"
    } else if critical_count > 0 {
        "🚨 整体状态：存在严重问题 (CRITICAL)"
    } else {
        "⚠️  整体状态：存在警告 (WARNING)"
    };

    let host_label = match ssh {
        Some(s) => format!(" [SSH:{}]", s.display()),
        None => String::new(),
    };

    let summary = format!(
        "{}{}\n   CRITICAL 项：{}  |  WARNING 项：{}",
        overall_status, host_label, critical_count, warning_count
    );

    let mut output = vec![
        "═══════════════════════════════════════════════════".to_string(),
        "        系统健康诊断报告 (health.check)            ".to_string(),
        "═══════════════════════════════════════════════════".to_string(),
        summary,
        "───────────────────────────────────────────────────".to_string(),
    ];
    output.extend(sections);
    output.push("═══════════════════════════════════════════════════".to_string());

    Ok(ToolResult::success("health.check", &output.join("\n"), output.len() as u64))
}

// ── 各子检查函数 ─────────────────────────────────────────────────────────────

async fn check_disk(warn_pct: u32, crit_pct: u32, ssh: Option<&SshConfig>) -> String {
    let output = run_cmd("df -h --output=target,pcent,size,used,avail 2>/dev/null || df -h 2>&1", ssh)
        .await
        .unwrap_or_else(|e| e.to_string());

    let mut lines = vec!["【磁盘使用率】".to_string()];
    let mut has_issue = false;

    for line in output.lines().skip(1) {
        // 提取百分比（形如 "85%"）
        let pct_opt = line.split_whitespace()
            .find(|s| s.ends_with('%'))
            .and_then(|s| s.trim_end_matches('%').parse::<u32>().ok());

        if let Some(pct) = pct_opt {
            let label = if pct >= crit_pct {
                has_issue = true;
                format!("🚨 CRITICAL ({pct}%)")
            } else if pct >= warn_pct {
                has_issue = true;
                format!("⚠️  WARNING  ({pct}%)")
            } else {
                format!("✅ OK       ({pct}%)")
            };
            lines.push(format!("   {}  {}", label, line));
        }
    }

    if !has_issue {
        lines.push(format!("   ✅ 所有分区磁盘使用率正常（阈值: ⚠️ {}% / 🚨 {}%）", warn_pct, crit_pct));
    }
    lines.join("\n")
}

async fn check_memory(warn_pct: u32, crit_pct: u32, ssh: Option<&SshConfig>) -> String {
    let output = run_cmd(
        "free -b 2>/dev/null || vm_stat 2>/dev/null || echo 'memory_unavailable'",
        ssh,
    ).await.unwrap_or_default();

    let mut lines = vec!["【内存使用率】".to_string()];

    // Linux: free -b 输出 "Mem: total used free ..."
    if let Some(mem_line) = output.lines().find(|l| l.starts_with("Mem:")) {
        let nums: Vec<u64> = mem_line.split_whitespace()
            .skip(1)
            .filter_map(|s| s.parse().ok())
            .collect();
        if nums.len() >= 2 {
            let total = nums[0];
            let used  = nums[1];
            if total > 0 {
                let pct = (used * 100 / total) as u32;
                let total_gb = total as f64 / 1_073_741_824.0;
                let used_gb  = used  as f64 / 1_073_741_824.0;
                let label = if pct >= crit_pct {
                    format!("🚨 CRITICAL ({pct}%)")
                } else if pct >= warn_pct {
                    format!("⚠️  WARNING  ({pct}%)")
                } else {
                    format!("✅ OK       ({pct}%)")
                };
                lines.push(format!(
                    "   {}  已用 {:.1}GiB / 总计 {:.1}GiB",
                    label, used_gb, total_gb
                ));
                return lines.join("\n");
            }
        }
    }

    // macOS: vm_stat 近似计算
    if output.contains("Pages free:") {
        let page_size: u64 = 4096;
        let extract = |prefix: &str| -> u64 {
            output.lines()
                .find(|l| l.contains(prefix))
                .and_then(|l| l.split(':').nth(1))
                .and_then(|s| s.trim().trim_end_matches('.').parse::<u64>().ok())
                .map(|pages| pages * page_size)
                .unwrap_or(0)
        };
        let free   = extract("Pages free:");
        let active = extract("Pages active:");
        let wired  = extract("Pages wired down:");
        let total  = free + active + wired;
        if total > 0 {
            let used = active + wired;
            let pct  = (used * 100 / total) as u32;
            let label = if pct >= crit_pct {
                format!("🚨 CRITICAL ({pct}%)")
            } else if pct >= warn_pct {
                format!("⚠️  WARNING  ({pct}%)")
            } else {
                format!("✅ OK       ({pct}%)")
            };
            lines.push(format!("   {}  已用 {} MiB / 近似总计 {} MiB",
                label,
                used / 1_048_576,
                total / 1_048_576,
            ));
            return lines.join("\n");
        }
    }

    lines.push("   ⚪ 内存信息不可用（可能需要 root 权限）".to_string());
    lines.join("\n")
}

async fn check_cpu(ssh: Option<&SshConfig>) -> String {
    let uptime_out = run_cmd("uptime 2>&1", ssh).await.unwrap_or_default();
    let nproc_out  = run_cmd("nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 1", ssh)
        .await.unwrap_or_else(|_| "1".to_string());
    let nproc: f64 = nproc_out.trim().parse().unwrap_or(1.0);

    let mut lines = vec!["【CPU 负载】".to_string()];

    // 提取 load average
    let load_opt = uptime_out.split("load average")
        .nth(1)
        .or_else(|| uptime_out.split("load averages").nth(1))
        .and_then(|s| s.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
        .or_else(|| {
            uptime_out.split_whitespace()
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .next()
                .map(|s| s.trim_end_matches(',').to_string())
        });

    if let Some(loads) = load_opt {
        let load1: f64 = loads.split(',')
            .next()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0.0);

        let ratio = load1 / nproc;
        let label = if ratio >= 2.0 {
            format!("🚨 CRITICAL (load={:.2}, cores={:.0}, ratio={:.1}x)", load1, nproc, ratio)
        } else if ratio >= 1.0 {
            format!("⚠️  WARNING  (load={:.2}, cores={:.0}, ratio={:.1}x)", load1, nproc, ratio)
        } else {
            format!("✅ OK       (load={:.2}, cores={:.0}, ratio={:.1}x)", load1, nproc, ratio)
        };
        lines.push(format!("   {}  负载值: {}", label, loads.trim()));
    } else {
        lines.push(format!("   ⚪ 无法解析 load average: {}", uptime_out.trim()));
    }
    lines.join("\n")
}

async fn check_top_processes(ssh: Option<&SshConfig>) -> String {
    let out = run_cmd(
        "ps aux --sort=-%cpu 2>/dev/null | head -6 || ps -arcwwwxo 'command pid %cpu %mem' 2>/dev/null | head -6",
        ssh,
    ).await.unwrap_or_default();

    let mut lines = vec!["【高资源进程 (Top 5 CPU)】".to_string()];
    for l in out.lines().take(6) {
        lines.push(format!("   {}", l));
    }
    lines.join("\n")
}

async fn check_services(ssh: Option<&SshConfig>) -> String {
    let systemctl_out = run_cmd(
        "systemctl --failed --no-pager --no-legend 2>/dev/null | head -10",
        ssh,
    ).await.unwrap_or_default();

    let mut lines = vec!["【关键服务状态】".to_string()];

    let failed: Vec<&str> = systemctl_out.lines()
        .filter(|l| !l.trim().is_empty())
        .collect();

    if failed.is_empty() {
        lines.push("   ✅ 无失败服务".to_string());
    } else {
        lines.push(format!("   🚨 CRITICAL  检测到 {} 个失败服务：", failed.len()));
        for f in &failed {
            lines.push(format!("      {}", f));
        }
    }

    // 额外检查 sshd / cron
    for svc in &["sshd", "ssh", "cron", "crond"] {
        let status = run_cmd(&format!("systemctl is-active {} 2>/dev/null", svc), ssh)
            .await.unwrap_or_default();
        let active = status.trim() == "active";
        if active {
            lines.push(format!("   ✅ {}  运行中", svc));
        }
    }

    lines.join("\n")
}

async fn check_recent_errors(ssh: Option<&SshConfig>) -> String {
    let cmds = [
        "journalctl -p err -n 20 --no-pager 2>/dev/null",
        "grep -i 'error\\|critical\\|panic' /var/log/syslog 2>/dev/null | tail -20",
        "grep -i 'error\\|critical\\|panic' /var/log/messages 2>/dev/null | tail -20",
    ];

    let mut lines = vec!["【近期系统错误日志 (最近 20 条)】".to_string()];

    for cmd in &cmds {
        let out = run_cmd(cmd, ssh).await.unwrap_or_default();
        if !out.trim().is_empty() && !out.contains("-- No entries --") {
            let count = out.lines().count();
            if count > 0 {
                lines.push(format!("   ⚠️  WARNING  发现 {} 条错误记录（近期）：", count));
                for l in out.lines().take(10) {
                    lines.push(format!("      {}", l));
                }
                if count > 10 {
                    lines.push(format!("      ...（共 {} 条，此处仅显示前 10 条）", count));
                }
            }
            return lines.join("\n");
        }
    }

    lines.push("   ✅ 未发现近期错误日志".to_string());
    lines.join("\n")
}

async fn run_cmd(cmd: &str, ssh: Option<&SshConfig>) -> Result<String> {
    if let Some(s) = ssh {
        let (stdout, stderr, _exit, _dur) = s.exec(cmd).await?;
        return Ok(if stdout.is_empty() { stderr } else { stdout });
    }
    let out = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .await?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    Ok(if stdout.is_empty() { stderr } else { stdout })
}

// ── 测试 ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> HealthCheckTool {
        HealthCheckTool::new()
    }

    #[test]
    fn tool_name_is_health_check() {
        assert_eq!(tool().name(), "health.check");
    }

    #[test]
    fn description_mentions_key_terms() {
        let t = tool();
        let d = t.description();
        assert!(d.contains("磁盘") || d.contains("内存"));
        assert!(d.contains("CPU") || d.contains("服务"));
    }

    #[test]
    fn schema_has_checks_property() {
        let s = tool().schema();
        assert!(s["properties"]["checks"].is_object());
    }

    #[test]
    fn schema_checks_is_array_type() {
        let s = tool().schema();
        assert_eq!(s["properties"]["checks"]["type"].as_str(), Some("array"));
    }

    #[test]
    fn schema_has_disk_warn_threshold() {
        let s = tool().schema();
        assert!(s["properties"]["disk_warn_pct"].is_object());
    }

    #[test]
    fn schema_has_mem_crit_threshold() {
        let s = tool().schema();
        assert!(s["properties"]["mem_crit_pct"].is_object());
    }

    #[tokio::test]
    async fn dry_run_returns_preview() {
        let args = serde_json::json!({});
        let result = tool().execute(&args, true).await.unwrap();
        assert!(result.dry_run_preview.is_some());
        assert!(result.dry_run_preview.unwrap().contains("诊断"));
    }

    #[tokio::test]
    async fn full_check_returns_report_header() {
        let args = serde_json::json!({});
        let result = tool().execute(&args, false).await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("health.check") || result.stdout.contains("健康诊断报告"));
    }

    #[tokio::test]
    async fn disk_only_check_runs_without_panic() {
        let args = serde_json::json!({ "checks": ["disk"] });
        let result = tool().execute(&args, false).await.unwrap();
        assert!(result.stdout.contains("磁盘") || result.stdout.contains("disk") || result.success);
    }

    #[tokio::test]
    async fn memory_only_check_runs_without_panic() {
        let args = serde_json::json!({ "checks": ["memory"] });
        let result = tool().execute(&args, false).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn cpu_only_check_runs_without_panic() {
        let args = serde_json::json!({ "checks": ["cpu"] });
        let result = tool().execute(&args, false).await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("CPU") || result.stdout.contains("load"));
    }

    #[tokio::test]
    async fn custom_thresholds_accepted() {
        let args = serde_json::json!({
            "checks": ["disk"],
            "disk_warn_pct": 70,
            "disk_crit_pct": 85
        });
        let result = tool().execute(&args, false).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn all_checks_produce_ok_or_warning_label() {
        let args = serde_json::json!({ "checks": ["all"] });
        let result = tool().execute(&args, false).await.unwrap();
        assert!(result.success);
        let out = &result.stdout;
        assert!(
            out.contains("OK") || out.contains("WARNING") || out.contains("CRITICAL"),
            "Expected status label in output: {}", out
        );
    }

    #[test]
    fn schema_required_is_empty_array() {
        let s = tool().schema();
        let req = s["required"].as_array().unwrap();
        assert!(req.is_empty());
    }
}
