use anyhow::{Result, anyhow};
use std::fmt;
use std::path::PathBuf;

/// Provider 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl std::str::FromStr for LlmProviderKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "anthropic" => Ok(LlmProviderKind::Anthropic),
            "openai-compatible" | "openai" => Ok(LlmProviderKind::OpenAiCompatible),
            _ => Err(anyhow!("未知的 provider: {}。支持: anthropic, openai-compatible", s)),
        }
    }
}

/// 预设 Provider 配置
#[derive(Debug, Clone)]
pub struct ProviderPreset {
    pub name: String,
    pub display_name: String,
    pub provider_kind: LlmProviderKind,
    pub base_url: String,
    pub default_model: String,
    pub suggested_env_keys: Vec<String>,
    pub aliases: Vec<String>,
    #[allow(dead_code)] // Reserved for future UI that guides users to fill in custom values
    pub requires_custom_base_url: bool,
    #[allow(dead_code)] // Reserved for future UI that guides users to fill in custom values
    pub requires_custom_model: bool,
    pub description: String,
}

impl ProviderPreset {
    pub fn matches_query(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }

        self.name.to_lowercase().contains(&q)
            || self.display_name.to_lowercase().contains(&q)
            || self.description.to_lowercase().contains(&q)
            || self.aliases.iter().any(|alias| alias.to_lowercase().contains(&q))
    }

    pub fn primary_env_key(&self) -> &str {
        self.suggested_env_keys
            .first()
            .map(|s| s.as_str())
            .unwrap_or("AGENT_UNIX_LLM_API_KEY")
    }
}

fn preset(
    name: &str,
    display_name: &str,
    provider_kind: LlmProviderKind,
    base_url: &str,
    default_model: &str,
    suggested_env_keys: &[&str],
    aliases: &[&str],
    description: &str,
) -> ProviderPreset {
    ProviderPreset {
        name: name.to_string(),
        display_name: display_name.to_string(),
        provider_kind,
        base_url: base_url.to_string(),
        default_model: default_model.to_string(),
        suggested_env_keys: suggested_env_keys.iter().map(|s| s.to_string()).collect(),
        aliases: aliases.iter().map(|s| s.to_string()).collect(),
        requires_custom_base_url: base_url.is_empty(),
        requires_custom_model: default_model.is_empty(),
        description: description.to_string(),
    }
}

/// 所有支持的 Provider 预设
pub fn get_provider_presets() -> Vec<ProviderPreset> {
    vec![
        preset(
            "anthropic",
            "Anthropic",
            LlmProviderKind::Anthropic,
            "https://api.anthropic.com",
            "claude-sonnet-4-5",
            &["ANTHROPIC_API_KEY"],
            &["claude"],
            "Claude（原生 tool_use 支持）",
        ),
        preset(
            "openai",
            "OpenAI",
            LlmProviderKind::OpenAiCompatible,
            "https://api.openai.com",
            "gpt-4o",
            &["OPENAI_API_KEY"],
            &["gpt"],
            "OpenAI GPT 系列",
        ),
        preset(
            "openrouter",
            "OpenRouter",
            LlmProviderKind::OpenAiCompatible,
            "https://openrouter.ai/api",
            "openai/gpt-4o",
            &["OPENROUTER_API_KEY"],
            &["router"],
            "OpenRouter（多模型聚合）",
        ),
        preset(
            "groq",
            "Groq",
            LlmProviderKind::OpenAiCompatible,
            "https://api.groq.com/openai",
            "llama-3.3-70b-versatile",
            &["GROQ_API_KEY"],
            &["llama"],
            "Groq（超快推理）",
        ),
        preset(
            "glm",
            "GLM",
            LlmProviderKind::OpenAiCompatible,
            "https://open.bigmodel.cn/api/paas/v4",
            "glm-4.5",
            &["GLM_API_KEY", "BIGMODEL_API_KEY"],
            &["zhipu", "bigmodel", "智谱"],
            "智谱 GLM（OpenAI-compatible）",
        ),
        preset(
            "kimi",
            "Kimi",
            LlmProviderKind::OpenAiCompatible,
            "https://api.moonshot.cn/v1",
            "moonshot-v1-128k",
            &["KIMI_API_KEY", "MOONSHOT_API_KEY"],
            &["moonshot", "月之暗面"],
            "Moonshot Kimi（长上下文）",
        ),
        preset(
            "deepseek",
            "DeepSeek",
            LlmProviderKind::OpenAiCompatible,
            "https://api.deepseek.com",
            "deepseek-chat",
            &["DEEPSEEK_API_KEY"],
            &["ds"],
            "DeepSeek（国产大模型）",
        ),
        preset(
            "minimax",
            "MiniMax",
            LlmProviderKind::OpenAiCompatible,
            "https://api.minimaxi.com/v1",
            "MiniMax-M1",
            &["MINIMAX_API_KEY"],
            &["abab", "海螺"],
            "MiniMax（国产模型）",
        ),
        preset(
            "bailian",
            "Bailian",
            LlmProviderKind::OpenAiCompatible,
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "qwen-coding-plus",
            &["BAILIAN_API_KEY", "DASHSCOPE_API_KEY"],
            &["qwen", "qwen-coding-plan", "codingplan", "百炼", "dashscope"],
            "阿里云百炼（Qwen Coding）",
        ),
        preset(
            "custom",
            "Custom",
            LlmProviderKind::OpenAiCompatible,
            "",
            "",
            &["AGENT_UNIX_LLM_API_KEY"],
            &["self-hosted", "proxy"],
            "自定义 OpenAI-compatible 端点",
        ),
    ]
}

/// 查找预设 Provider
pub fn find_preset(name: &str) -> Option<ProviderPreset> {
    let target = name.to_lowercase();
    get_provider_presets().into_iter().find(|p| {
        p.name == target || p.aliases.iter().any(|alias| alias.to_lowercase() == target)
    })
}

/// 用户配置文件结构
#[derive(Debug, Clone, serde::Deserialize)]
struct UserConfigFile {
    provider: String,
    provider_preset: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
}

/// 配置文件路径
fn user_config_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".agent-unix").join("config.json")
}

/// 从文件加载用户配置
fn load_user_config_file() -> Option<UserConfigFile> {
    let path = user_config_path();
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// LLM 配置
#[derive(Clone)]
pub struct LlmConfig {
    pub provider_kind: LlmProviderKind,
    pub base_url: String,
    pub model: String,
    api_key: String,
}

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

pub fn validate_base_url(url_str: &str) -> Result<String> {
    if url_str.is_empty() {
        return Err(anyhow!("Base URL 不能为空"));
    }
    if !url_str.starts_with("https://") {
        return Err(anyhow!("Base URL 必须使用 HTTPS 协议"));
    }
    if url_str.contains('@') && url_str.contains(':') {
        return Err(anyhow!("Base URL 不能包含嵌入的凭证"));
    }
    Ok(url_str.trim_end_matches('/').to_string())
}

pub fn validate_model(model_str: &str) -> Result<String> {
    if model_str.is_empty() {
        return Err(anyhow!("Model ID 不能为空"));
    }
    let valid_chars = model_str
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' || c == '/');
    if !valid_chars {
        return Err(anyhow!("Model ID 包含无效字符"));
    }
    if model_str.len() > 128 {
        return Err(anyhow!("Model ID 过长"));
    }
    Ok(model_str.to_string())
}

impl LlmConfig {
    /// 从 CLI 参数、配置文件和环境变量加载配置
    /// 优先级：CLI > 配置文件 > ENV > 预设默认
    pub fn load(
        provider_cli: Option<&str>,
        model_cli: Option<&str>,
        base_url_cli: Option<&str>,
        api_key_cli: Option<&str>,
    ) -> Result<Self> {
        let user_config = load_user_config_file();
        let env_provider = std::env::var("AGENT_UNIX_LLM_PROVIDER").ok();

        let provider_str = provider_cli
            .or_else(|| user_config.as_ref().and_then(|c| c.provider_preset.as_deref()))
            .or_else(|| user_config.as_ref().map(|c| c.provider.as_str()))
            .or_else(|| env_provider.as_deref());

        let preset = provider_str.and_then(find_preset);
        let provider_kind = preset
            .as_ref()
            .map(|p| p.provider_kind)
            .unwrap_or(LlmProviderKind::Anthropic);

        let base_url = if let Some(url) = base_url_cli {
            validate_base_url(url)?
        } else if let Ok(url) = std::env::var("AGENT_UNIX_LLM_BASE_URL") {
            validate_base_url(&url)?
        } else if let Some(ref config) = user_config {
            if let Some(url) = &config.base_url {
                validate_base_url(url)?
            } else if let Some(p) = &preset {
                if p.base_url.is_empty() {
                    "https://api.anthropic.com".to_string()
                } else {
                    p.base_url.clone()
                }
            } else {
                "https://api.anthropic.com".to_string()
            }
        } else if let Some(p) = &preset {
            if p.base_url.is_empty() {
                "https://api.anthropic.com".to_string()
            } else {
                p.base_url.clone()
            }
        } else {
            "https://api.anthropic.com".to_string()
        };

        let model = if let Some(m) = model_cli {
            validate_model(m)?
        } else if let Ok(m) = std::env::var("AGENT_UNIX_LLM_MODEL") {
            validate_model(&m)?
        } else if let Some(ref config) = user_config {
            if let Some(m) = &config.model {
                validate_model(m)?
            } else if let Some(p) = &preset {
                if p.default_model.is_empty() {
                    default_model_for(provider_kind)
                } else {
                    validate_model(&p.default_model)?
                }
            } else {
                default_model_for(provider_kind)
            }
        } else if let Some(p) = &preset {
            if p.default_model.is_empty() {
                default_model_for(provider_kind)
            } else {
                validate_model(&p.default_model)?
            }
        } else {
            default_model_for(provider_kind)
        };

        let api_key = if let Some(key) = api_key_cli {
            key.to_string()
        } else if let Ok(key) = std::env::var("AGENT_UNIX_LLM_API_KEY") {
            key
        } else if let Some(ref config) = user_config {
            if let Some(key) = &config.api_key {
                key.clone()
            } else {
                get_fallback_api_key(provider_kind, preset.as_ref())?
            }
        } else {
            get_fallback_api_key(provider_kind, preset.as_ref())?
        };

        if model.to_lowercase().contains("claude") && provider_kind != LlmProviderKind::Anthropic {
            return Err(anyhow!(
                "Claude 模型必须使用 anthropic provider\n提示：使用 --provider anthropic"
            ));
        }

        Ok(Self {
            provider_kind,
            base_url,
            model,
            api_key,
        })
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn anthropic_endpoint(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }

    pub fn openai_endpoint(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
    }

    #[allow(dead_code)]
    pub fn display_summary(&self) -> String {
        format!(
            "Provider: {}\nBase URL: {}\nModel: {}\nAPI Key: {}...",
            self.provider_kind,
            self.base_url,
            self.model,
            self.api_key.chars().take(8).collect::<String>()
        )
    }
}

fn default_model_for(provider_kind: LlmProviderKind) -> String {
    if provider_kind == LlmProviderKind::Anthropic {
        "claude-sonnet-4-5".to_string()
    } else {
        "gpt-4o".to_string()
    }
}

fn get_fallback_api_key(
    provider_kind: LlmProviderKind,
    preset: Option<&ProviderPreset>,
) -> Result<String> {
    if let Some(preset) = preset {
        for env_key in &preset.suggested_env_keys {
            if let Ok(key) = std::env::var(env_key) {
                return Ok(key);
            }
        }
    }

    if provider_kind == LlmProviderKind::Anthropic {
        std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow!("请设置 ANTHROPIC_API_KEY 或运行 'agent-unix config --setup'"))
    } else {
        std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow!("请设置对应 provider 的 API Key，或运行 'agent-unix config --setup'"))
    }
}
