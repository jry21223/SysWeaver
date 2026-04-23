use super::patterns::*;
use crate::types::{
    risk::{RiskAssessment, RiskLevel},
    tool::ToolCall,
};
use regex::Regex;

pub struct RiskClassifier {
    critical: Vec<Regex>,
    high: Vec<Regex>,
    medium: Vec<Regex>,
}

impl RiskClassifier {
    pub fn new() -> Self {
        let compile = |patterns: &[&str]| -> Vec<Regex> {
            patterns
                .iter()
                .map(|p| Regex::new(p).expect("Invalid risk pattern"))
                .collect()
        };

        Self {
            critical: compile(CRITICAL_PATTERNS),
            high: compile(HIGH_PATTERNS),
            medium: compile(MEDIUM_PATTERNS),
        }
    }

    /// 对 ToolCall 进行风险评估
    pub fn assess(&self, call: &ToolCall) -> RiskAssessment {
        // 首先对结构化工具的 action 字段做语义评估
        if let Some(assessment) = self.assess_structured_tool(call) {
            return assessment;
        }

        // 提取所有需要检查的字符串（tool name + args）
        let check_target = self.extract_check_string(call);

        // 从高到低依次检查
        if let Some(pattern) = self.matches_any(&self.critical, &check_target) {
            return RiskAssessment {
                level: RiskLevel::Critical,
                reason: format!("检测到极危险操作模式: `{}`", pattern),
                impact: "此操作可能导致系统不可恢复的损坏，已被强制阻止".to_string(),
                alternative: Some("请明确说明您的实际需求，我会为您提供安全的替代方案".to_string()),
            };
        }

        if let Some(pattern) = self.matches_any(&self.high, &check_target) {
            return RiskAssessment {
                level: RiskLevel::High,
                reason: format!("检测到高风险操作: `{}`", pattern),
                impact: self.describe_high_risk_impact(call, &pattern),
                alternative: None,
            };
        }

        if self.matches_any(&self.medium, &check_target).is_some() {
            return RiskAssessment {
                level: RiskLevel::Medium,
                reason: "此操作会修改系统状态".to_string(),
                impact: "操作通常可逆，但建议确认操作范围".to_string(),
                alternative: None,
            };
        }

        // 默认：读操作或已知安全工具
        let level = match call.tool.as_str() {
            t if t.starts_with("system.info") => RiskLevel::Safe,
            t if t.starts_with("file.read") => RiskLevel::Safe,
            t if t.starts_with("file.search") => RiskLevel::Safe,
            t if t.starts_with("process.manage") => {
                // process.manage list/find/info 是安全的，kill 是高风险
                if call.args["action"].as_str() == Some("kill") {
                    RiskLevel::High
                } else {
                    RiskLevel::Safe
                }
            }
            t if t.starts_with("user.manage") => {
                // user.manage list/info 安全，create 中等，delete 高风险
                match call.args["action"].as_str() {
                    Some("list") | Some("info") => RiskLevel::Safe,
                    Some("delete") => RiskLevel::High,
                    Some("create") | Some("passwd") => RiskLevel::Medium,
                    _ => RiskLevel::Low,
                }
            }
            t if t.starts_with("service.manage") => {
                // service.manage list/status 安全，start/restart 中等，stop 高风险
                match call.args["action"].as_str() {
                    Some("list") | Some("status") => RiskLevel::Safe,
                    Some("stop") | Some("disable") => RiskLevel::High,
                    Some("start") | Some("restart") | Some("enable") => RiskLevel::Medium,
                    _ => RiskLevel::Low,
                }
            }
            _ => RiskLevel::Low,
        };

        RiskAssessment {
            level,
            reason: "未检测到风险模式".to_string(),
            impact: String::new(),
            alternative: None,
        }
    }

    /// 对结构化工具的语义评估（user.manage delete / service.manage stop sshd 等）
    fn assess_structured_tool(&self, call: &ToolCall) -> Option<RiskAssessment> {
        match call.tool.as_str() {
            "user.manage" => {
                let action = call.args["action"].as_str()?;
                if action == "delete" {
                    let username = call.args["username"].as_str().unwrap_or("该用户");
                    return Some(RiskAssessment {
                        level: RiskLevel::High,
                        reason: format!("删除用户 '{}' 是不可逆操作", username),
                        impact: format!(
                            "将永久删除用户 {} 及其家目录下的所有文件，此操作无法自动恢复",
                            username
                        ),
                        alternative: Some(format!(
                            "如需临时禁用，可改为：usermod -L {} 锁定账号",
                            username
                        )),
                    });
                }
            }
            "service.manage" => {
                let action = call.args["action"].as_str()?;
                let service = call.args["service"].as_str().unwrap_or("");
                // 停止 SSH 服务：极度危险（可能断开当前远程连接）
                if action == "stop" && (service.contains("ssh") || service == "sshd") {
                    return Some(RiskAssessment {
                        level: RiskLevel::High,
                        reason: "停止 SSH 服务将断开所有远程连接".to_string(),
                        impact: "包括当前会话在内的所有 SSH 连接将立即断开，可能导致无法远程访问服务器".to_string(),
                        alternative: Some("若需重启 SSH，建议改用 systemctl restart sshd".to_string()),
                    });
                }
                if action == "stop" || action == "disable" {
                    return Some(RiskAssessment {
                        level: RiskLevel::High,
                        reason: format!("停止/禁用服务 '{}' 可能影响系统功能", service),
                        impact: format!("服务 {} 停止后，依赖该服务的功能将不可用", service),
                        alternative: None,
                    });
                }
            }
            "process.manage" => {
                let action = call.args["action"].as_str()?;
                if action == "kill" {
                    let pid = call.args["pid"].as_i64().unwrap_or(0);
                    let signal = call.args["signal"].as_str().unwrap_or("TERM");
                    return Some(RiskAssessment {
                        level: RiskLevel::High,
                        reason: format!("终止进程 PID {} (信号: {})", pid, signal),
                        impact: "强制终止进程可能导致数据丢失或服务中断".to_string(),
                        alternative: Some("建议先用 TERM 信号优雅停止，再考虑 KILL".to_string()),
                    });
                }
            }
            _ => {}
        }
        None
    }

    fn extract_check_string(&self, call: &ToolCall) -> String {
        let args_str = call.args.to_string();
        format!("{} {}", call.tool, args_str)
    }

    fn matches_any(&self, patterns: &[Regex], target: &str) -> Option<String> {
        for regex in patterns {
            if regex.is_match(target) {
                return Some(regex.as_str().to_string());
            }
        }
        None
    }

    fn describe_high_risk_impact(&self, call: &ToolCall, pattern: &str) -> String {
        if pattern.contains("userdel") {
            let username = call
                .args
                .get("username")
                .and_then(|v| v.as_str())
                .unwrap_or("该用户");
            return format!(
                "将永久删除用户 {} 及其家目录下的所有文件，此操作不可逆",
                username
            );
        }
        if pattern.contains("sshd") {
            return "停止 SSH 服务将导致所有远程连接立即断开，包括当前会话".to_string();
        }
        if pattern.contains("iptables") || pattern.contains("ufw") {
            return "清空防火墙规则将使所有端口对外开放，存在安全风险".to_string();
        }
        "此操作风险较高，请确认后再执行".to_string()
    }
}

impl Default for RiskClassifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_call(tool: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool: tool.to_string(),
            args,
            reason: None,
            dry_run: false,
        }
    }

    #[test]
    fn read_only_tools_are_safe() {
        let clf = RiskClassifier::new();
        let call = make_call("system.info", json!({"query": "disk"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Safe);

        let call = make_call("file.read", json!({"path": "/etc/hostname"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Safe);

        let call = make_call("file.search", json!({"pattern": "*.log", "path": "/var/log"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Safe);
    }

    #[test]
    fn rm_rf_root_is_critical() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "rm -rf /etc"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Critical);
    }

    #[test]
    fn mkfs_is_critical() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "mkfs.ext4 /dev/sdb1"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Critical);
    }

    #[test]
    fn dd_to_disk_is_critical() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "dd if=/dev/zero of=/dev/sda"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Critical);
    }

    #[test]
    fn userdel_is_high() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "userdel -r john"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::High);
    }

    #[test]
    fn stop_ssh_is_high() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "systemctl stop sshd"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::High);
    }

    #[test]
    fn iptables_flush_is_high() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "iptables -F"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::High);
    }

    #[test]
    fn useradd_is_medium() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "useradd testuser"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Medium);
    }

    #[test]
    fn systemctl_restart_is_medium() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "systemctl restart nginx"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Medium);
    }

    #[test]
    fn df_command_is_low() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "df -h"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Low);
    }

    #[test]
    fn critical_blocked_flag() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "rm -rf /home"}));
        let assessment = clf.assess(&call);
        assert!(assessment.level.is_blocked());
    }

    #[test]
    fn high_is_not_blocked() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "userdel john"}));
        let assessment = clf.assess(&call);
        assert!(!assessment.level.is_blocked());
        assert_eq!(assessment.level, RiskLevel::High);
    }

    // ── structured tool risk tests ──────────────────────────────────────────

    #[test]
    fn user_manage_list_is_safe() {
        let clf = RiskClassifier::new();
        let call = make_call("user.manage", json!({"action": "list"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Safe);
    }

    #[test]
    fn user_manage_create_is_medium() {
        let clf = RiskClassifier::new();
        let call = make_call("user.manage", json!({"action": "create", "username": "testuser"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Medium);
    }

    #[test]
    fn user_manage_delete_is_high() {
        let clf = RiskClassifier::new();
        let call = make_call("user.manage", json!({"action": "delete", "username": "john"}));
        let result = clf.assess(&call);
        assert_eq!(result.level, RiskLevel::High);
        assert!(result.reason.contains("john"));
    }

    #[test]
    fn service_manage_status_is_safe() {
        let clf = RiskClassifier::new();
        let call = make_call("service.manage", json!({"action": "status", "service": "nginx"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Safe);
    }

    #[test]
    fn service_manage_restart_is_medium() {
        let clf = RiskClassifier::new();
        let call = make_call("service.manage", json!({"action": "restart", "service": "nginx"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Medium);
    }

    #[test]
    fn service_manage_stop_sshd_is_high() {
        let clf = RiskClassifier::new();
        let call = make_call("service.manage", json!({"action": "stop", "service": "sshd"}));
        let result = clf.assess(&call);
        assert_eq!(result.level, RiskLevel::High);
        assert!(result.impact.contains("SSH"));
    }

    #[test]
    fn service_manage_stop_nginx_is_high() {
        let clf = RiskClassifier::new();
        let call = make_call("service.manage", json!({"action": "stop", "service": "nginx"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::High);
    }

    #[test]
    fn process_manage_list_is_safe() {
        let clf = RiskClassifier::new();
        let call = make_call("process.manage", json!({"action": "list"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Safe);
    }

    #[test]
    fn process_manage_kill_is_high() {
        let clf = RiskClassifier::new();
        let call = make_call("process.manage", json!({"action": "kill", "pid": 1234}));
        let result = clf.assess(&call);
        assert_eq!(result.level, RiskLevel::High);
        assert!(result.reason.contains("1234"));
    }

    #[test]
    fn rm_passwd_without_flags_is_critical() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "rm /etc/passwd"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Critical);
    }

    #[test]
    fn rm_shadow_without_flags_is_critical() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "sudo rm /etc/shadow"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Critical);
    }

    #[test]
    fn rm_ssh_authorized_keys_is_high() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "rm /home/user/.ssh/authorized_keys"}));
        let result = clf.assess(&call);
        // Matches HIGH pattern for removing .ssh authorized_keys
        assert!(result.level == RiskLevel::High || result.level == RiskLevel::Critical);
    }

    #[test]
    fn rm_rf_etc_is_critical() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "rm -rf /etc/passwd"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Critical);
    }

    #[test]
    fn rm_rf_root_slash_is_critical() {
        let clf = RiskClassifier::new();
        // rm -rf / in JSON-encoded args: check_target = 'shell.exec {"command":"rm -rf /"}'
        let call = make_call("shell.exec", json!({"command": "rm -rf /"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Critical);
    }

    #[test]
    fn sudo_rm_passwd_is_critical() {
        let clf = RiskClassifier::new();
        let call = make_call("shell.exec", json!({"command": "sudo rm /etc/passwd"}));
        assert_eq!(clf.assess(&call).level, RiskLevel::Critical);
    }
}
