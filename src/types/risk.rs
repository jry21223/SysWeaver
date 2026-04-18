use serde::{Deserialize, Serialize};

/// 五级风险分类
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskLevel {
    /// 只读操作：df, ps, ls, cat
    Safe,
    /// 轻量写操作：创建文件、写 /tmp
    Low,
    /// 可逆系统变更：重启服务、创建用户、安装软件
    Medium,
    /// 不易恢复：删除用户（及数据）、修改关键配置、改权限
    High,
    /// 不可逆破坏：rm -rf /, mkfs, dd 写盘
    Critical,
}

impl RiskLevel {
    pub fn label(&self) -> &str {
        match self {
            RiskLevel::Safe => "SAFE",
            RiskLevel::Low => "LOW",
            RiskLevel::Medium => "MEDIUM",
            RiskLevel::High => "HIGH",
            RiskLevel::Critical => "CRITICAL",
        }
    }

    #[allow(dead_code)] // used in UI display formatting
    pub fn emoji(&self) -> &str {
        match self {
            RiskLevel::Safe => "✅",
            RiskLevel::Low => "🔵",
            RiskLevel::Medium => "🟡",
            RiskLevel::High => "⚠️ ",
            RiskLevel::Critical => "🚨",
        }
    }

    /// 是否需要用户确认才能执行
    #[allow(dead_code)] // confirmation logic handled in AgentLoop directly
    pub fn requires_confirmation(&self) -> bool {
        matches!(self, RiskLevel::High | RiskLevel::Critical)
    }

    /// 是否直接拒绝执行（不给确认机会）
    pub fn is_blocked(&self) -> bool {
        matches!(self, RiskLevel::Critical)
    }
}

/// 风险评估结果
#[derive(Debug, Clone)]
pub struct RiskAssessment {
    pub level: RiskLevel,
    /// 触发该风险级别的具体原因
    pub reason: String,
    /// 操作影响范围说明
    pub impact: String,
    /// 建议的替代方案（可选）
    pub alternative: Option<String>,
}
