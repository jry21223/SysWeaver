use anyhow::{anyhow, Result};
use std::fmt;

/// Provider 类型：Anthropic 原生 API 或 OpenAI-compatible API
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LlmProviderKind {
    Anthropic,
    OpenAiCompatible,
}

impl fmt::Display for LlmProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmProviderKind::Anthropic => write!(f, "anthropic"),
            LlmProviderKind::OpenAiCompatible => write!(f, "openai-compatible"),
        }
    }
}

/// LLM 配置：provider、base URL、model、API key
#[derive(Clone)]
pub struct LlmConfig {
    pub provider_kind: LlmProviderKind,
    pub base_url: String,
    pub model: String,
    api_key: String,
}

/// 自定义 Debug 实现，隐藏 API key
impl fmt::Debug for LlmConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlmConfig")
            .field("provider_kind", &self.provider_kind)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

/// 验证 base URL 安全性
fn validate_base_url(url_str: &str) -> Result<String> {
    // 基本格式检查
    if url_str.is_empty() {
        return Err(anyhow!("Base URL 不能为空"));
    }

    // 必须使用 HTTPS
    if !url_str.starts_with("https://") {
        return Err(anyhow!("Base URL 必须使用 HTTPS 协议"));
    }

    // 检查是否包含嵌入的凭证
    if url_str.contains('@') && url_str.contains(':') {
        return Err(anyhow!("Base URL 不能包含嵌入的凭证"));
    }

    Ok(url_str.trim_end_matches('/').to_string())
}

/// 验证模型名称安全性
fn validate_model(model_str: &str) -> Result<String> {
    if model_str.is_empty() {
        return Err(anyhow!("Model ID 不能为空"));
    }

    // 只允许字母、数字、点、下划线、连字符
    let valid_chars = model_str.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_'
    });

    if !valid_chars {
        return Err(anyhow!("Model ID 包含无效字符，只允许字母、数字、点、下划线、连字符"));
    }

    if model_str.len() > 128 {
        return Err(anyhow!("Model ID 过长（最大 128 字符）"));
    }

    Ok(model_str.to_string())
}

impl LlmConfig {
    /// 从 CLI 参数和环境变量加载配置
    /// 优先级：CLI > ENV > 默认
    pub fn load(
        provider_cli: Option<&str>,
        model_cli: Option<&str>,
        base_url_cli: Option<&str>,
    ) -> Result<Self> {
        // 1. 确定 provider kind
        let provider_str = match provider_cli {
            Some(p) => p.to_string(),
            None => std::env::var("AGENT_UNIX_LLM_PROVIDER")
                .unwrap_or_else(|_| "anthropic".to_string()),
        };

        let provider_kind = match provider_str.to_lowercase().as_str() {
            "anthropic" => LlmProviderKind::Anthropic,
            "openai-compatible" | "openai" => LlmProviderKind::OpenAiCompatible,
            _ => return Err(anyhow!("未知的 provider 类型")),
        };

        // 2. 确定 base URL（两者都支持自定义端点）
        let base_url = match base_url_cli {
            Some(url) => validate_base_url(url)?,
            None => std::env::var("AGENT_UNIX_LLM_BASE_URL")
                .ok()
                .map(|url| validate_base_url(&url))
                .transpose()?
                .unwrap_or_else(|| {
                    // 默认端点：Anthropic 官方 / OpenAI 官方
                    if provider_kind == LlmProviderKind::Anthropic {
                        "https://api.anthropic.com".to_string()
                    } else {
                        "https://api.openai.com".to_string()
                    }
                }),
        };

        // 3. 确定 model（两者都支持自定义模型）
        let model = match model_cli {
            Some(m) => validate_model(m)?,
            None => std::env::var("AGENT_UNIX_LLM_MODEL")
                .ok()
                .map(|m| validate_model(&m))
                .transpose()?
                .unwrap_or_else(|| {
                    if provider_kind == LlmProviderKind::Anthropic {
                        "claude-sonnet-4-5".to_string()
                    } else {
                        "gpt-4o".to_string()
                    }
                }),
        };

        // 4. 确定 API key（统一优先级：自定义变量 > 官方变量）
        let api_key = std::env::var("AGENT_UNIX_LLM_API_KEY")
            .or_else(|_| {
                // 兼容各 provider 的官方环境变量
                if provider_kind == LlmProviderKind::Anthropic {
                    std::env::var("ANTHROPIC_API_KEY")
                } else {
                    std::env::var("OPENAI_API_KEY")
                }
            })
            .map_err(|_| anyhow!("未设置 API key。请设置 AGENT_UNIX_LLM_API_KEY 或对应 provider 的官方变量"))?;

        // 5. Claude 模型只能用于 Anthropic provider（保持原生 tool_use 语义）
        if model.to_lowercase().contains("claude") && provider_kind != LlmProviderKind::Anthropic {
            return Err(anyhow!("Claude 模型必须使用 anthropic provider"));
        }

        Ok(Self {
            provider_kind,
            base_url,
            model,
            api_key,
        })
    }

    /// 获取 API key（内部使用）
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// 获取 Anthropic Messages API 端点
    pub fn anthropic_endpoint(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }

    /// 获取 OpenAI-compatible Chat Completions API 端点
    pub fn openai_endpoint(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
    }
}