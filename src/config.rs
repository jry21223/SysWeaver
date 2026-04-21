use anyhow::{anyhow, Result};
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
    pub provider_kind: LlmProviderKind,
    pub base_url: String,
    pub default_model: String,
    pub env_key_name: String,
    pub description: String,
}

/// 所有支持的 Provider 预设
pub fn get_provider_presets() -> Vec<ProviderPreset> {
    vec![
        ProviderPreset {
            name: "anthropic".to_string(),
            provider_kind: LlmProviderKind::Anthropic,
            base_url: "https://api.anthropic.com".to_string(),
            default_model: "claude-sonnet-4-5".to_string(),
            env_key_name: "ANTHROPIC_API_KEY".to_string(),
            description: "Anthropic Claude（原生 tool_use 支持）".to_string(),
        },
        ProviderPreset {
            name: "openai".to_string(),
            provider_kind: LlmProviderKind::OpenAiCompatible,
            base_url: "https://api.openai.com".to_string(),
            default_model: "gpt-4o".to_string(),
            env_key_name: "OPENAI_API_KEY".to_string(),
            description: "OpenAI GPT 系列".to_string(),
        },
        ProviderPreset {
            name: "openrouter".to_string(),
            provider_kind: LlmProviderKind::OpenAiCompatible,
            base_url: "https://openrouter.ai/api".to_string(),
            default_model: "openai/gpt-4o".to_string(),
            env_key_name: "OPENROUTER_API_KEY".to_string(),
            description: "OpenRouter（多模型聚合）".to_string(),
        },
        ProviderPreset {
            name: "groq".to_string(),
            provider_kind: LlmProviderKind::OpenAiCompatible,
            base_url: "https://api.groq.com".to_string(),
            default_model: "llama-3.3-70b-versatile".to_string(),
            env_key_name: "GROQ_API_KEY".to_string(),
            description: "Groq（超快推理）".to_string(),
        },
        ProviderPreset {
            name: "deepseek".to_string(),
            provider_kind: LlmProviderKind::OpenAiCompatible,
            base_url: "https://api.deepseek.com".to_string(),
            default_model: "deepseek-chat".to_string(),
            env_key_name: "DEEPSEEK_API_KEY".to_string(),
            description: "DeepSeek（国产大模型）".to_string(),
        },
        ProviderPreset {
            name: "custom".to_string(),
            provider_kind: LlmProviderKind::OpenAiCompatible,
            base_url: "".to_string(), // 需要用户指定
            default_model: "".to_string(), // 需要用户指定
            env_key_name: "AGENT_UNIX_LLM_API_KEY".to_string(),
            description: "自定义 OpenAI-compatible 端点".to_string(),
        },
    ]
}

/// 查找预设 Provider
pub fn find_preset(name: &str) -> Option<ProviderPreset> {
    get_provider_presets()
        .iter()
        .find(|p| p.name == name.to_lowercase())
        .cloned()
}

/// 用户配置文件结构
#[derive(Debug, Clone, serde::Deserialize)]
struct UserConfigFile {
    provider: String,
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

fn validate_base_url(url_str: &str) -> Result<String> {
    if url_str.is_empty() {
        return Err(anyhow!("Base URL 不能为空"));
    }
    if !url_str.starts_with("https://") && !url_str.starts_with("http://") {
        return Err(anyhow!("Base URL 必须使用 HTTP/HTTPS 协议"));
    }
    if url_str.contains('@') && url_str.contains(':') {
        return Err(anyhow!("Base URL 不能包含嵌入的凭证"));
    }
    Ok(url_str.trim_end_matches('/').to_string())
}

fn validate_model(model_str: &str) -> Result<String> {
    if model_str.is_empty() {
        return Err(anyhow!("Model ID 不能为空"));
    }
    let valid_chars = model_str.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' || c == '/'
    });
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
        // 0. 尝试从配置文件加载
        let user_config = load_user_config_file();
        let env_provider = std::env::var("AGENT_UNIX_LLM_PROVIDER").ok();

        // 1. 查找预设并获取配置
        let provider_str = provider_cli
            .or_else(|| user_config.as_ref().map(|c| c.provider.as_str()))
            .or_else(|| env_provider.as_deref());

        let (provider_kind, preset_base_url, preset_model, preset_env_key) = {
            let preset = provider_str.and_then(|p| find_preset(p));

            if let Some(p) = preset {
                (p.provider_kind, Some(p.base_url), Some(p.default_model), Some(p.env_key_name))
            } else {
                (LlmProviderKind::Anthropic, None, None, None)
            }
        };

        // 2. 确定 base URL
        let base_url = if let Some(url) = base_url_cli {
            validate_base_url(url)?
        } else if let Ok(url) = std::env::var("AGENT_UNIX_LLM_BASE_URL") {
            validate_base_url(&url)?
        } else if let Some(ref config) = user_config {
            if let Some(url) = &config.base_url {
                validate_base_url(url)?
            } else if let Some(url) = preset_base_url {
                url
            } else {
                "https://api.anthropic.com".to_string()
            }
        } else if let Some(url) = preset_base_url {
            url
        } else {
            "https://api.anthropic.com".to_string()
        };

        // 3. 确定 model
        let model = if let Some(m) = model_cli {
            validate_model(m)?
        } else if let Ok(m) = std::env::var("AGENT_UNIX_LLM_MODEL") {
            validate_model(&m)?
        } else if let Some(ref config) = user_config {
            if let Some(m) = &config.model {
                validate_model(m)?
            } else {
                default_model_for(provider_kind)
            }
        } else if let Some(m) = preset_model {
            if m.is_empty() {
                default_model_for(provider_kind)
            } else {
                m
            }
        } else {
            default_model_for(provider_kind)
        };

        // 4. 确定 API key
        let api_key = if let Some(key) = api_key_cli {
            key.to_string()
        } else if let Ok(key) = std::env::var("AGENT_UNIX_LLM_API_KEY") {
            key
        } else if let Some(ref config) = user_config {
            if let Some(key) = &config.api_key {
                key.clone()
            } else {
                get_fallback_api_key(provider_kind, preset_env_key.as_deref())?
            }
        } else if let Some(env_key) = preset_env_key {
            if let Ok(key) = std::env::var(&env_key) {
                key
            } else {
                get_fallback_api_key(provider_kind, Some(&env_key))?
            }
        } else {
            get_fallback_api_key(provider_kind, None)?
        };

        // 5. Claude 模型限制
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

    /// 显示配置摘要
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

fn get_fallback_api_key(provider_kind: LlmProviderKind, preset_env_key: Option<&str>) -> Result<String> {
    // 优先使用预设的环境变量名
    if let Some(env_key) = preset_env_key {
        if let Ok(key) = std::env::var(env_key) {
            return Ok(key);
        }
    }

    // 然后根据 provider 类型尝试官方变量
    if provider_kind == LlmProviderKind::Anthropic {
        std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow!("请设置 ANTHROPIC_API_KEY 或运行 'agent-unix config --setup'"))
    } else {
        std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow!("请设置 OPENAI_API_KEY 或运行 'agent-unix config --setup'"))
    }
}