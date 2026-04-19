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
            _ => RiskLevel::Low,
        };

        RiskAssessment {
            level,
            reason: "未检测到风险模式".to_string(),
            impact: String::new(),
            alternative: None,
        }
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
