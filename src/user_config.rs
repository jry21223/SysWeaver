use anyhow::Result;
use inquire::{Confirm, Password, Select, Text};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::config::{
    ProviderPreset, find_preset, get_provider_presets, validate_base_url, validate_model,
};

/// 用户配置文件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfig {
    /// Provider 名称
    pub provider: String,
    /// 选中的预设名称
    pub provider_preset: Option<String>,
    /// Base URL
    pub base_url: Option<String>,
    /// Model ID
    pub model: Option<String>,
    /// API Key
    pub api_key: Option<String>,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            provider_preset: Some("anthropic".to_string()),
            base_url: None,
            model: None,
            api_key: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderOption {
    pub preset: ProviderPreset,
}

impl std::fmt::Display for ProviderOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.preset.display_name, self.preset.description)
    }
}

/// 配置文件路径
pub fn config_file_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".agent-unix").join("config.json")
}

/// 加载用户配置
pub fn load_user_config() -> Option<UserConfig> {
    let path = config_file_path();
    if !path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// 保存用户配置
pub fn save_user_config(config: &UserConfig) -> Result<()> {
    let path = config_file_path();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let content = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, content)?;

    println!("✅ 配置已保存到: {}", path.display());
    Ok(())
}

fn prompt_provider(initial_provider: Option<&str>) -> Result<ProviderPreset> {
    let presets = get_provider_presets();
    let options: Vec<ProviderOption> = presets
        .into_iter()
        .map(|preset| ProviderOption { preset })
        .collect();

    let starting_cursor = initial_provider
        .and_then(|name| {
            options
                .iter()
                .position(|option| option.preset.name == name || option.preset.matches_query(name))
        })
        .unwrap_or(0);

    let selected = Select::new("📡 Select provider:", options)
        .with_starting_cursor(starting_cursor)
        .with_help_message("↑↓ 切换，输入关键字过滤，Enter 选择")
        .with_page_size(10)
        .prompt()?;

    Ok(selected.preset)
}

fn prompt_base_url(selected: &ProviderPreset) -> Result<Option<String>> {
    let default_value = if selected.base_url.is_empty() {
        "https://"
    } else {
        &selected.base_url
    };

    let input = Text::new("🌐 Base URL:")
        .with_default(default_value)
        .with_help_message("可直接回车接受默认值；必须为 https://")
        .prompt()?;

    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    Ok(Some(validate_base_url(trimmed)?))
}

fn prompt_model(selected: &ProviderPreset) -> Result<Option<String>> {
    let default_value = if selected.default_model.is_empty() {
        ""
    } else {
        &selected.default_model
    };

    let mut prompt = Text::new("🤖 Model ID:");
    if !default_value.is_empty() {
        prompt = prompt.with_default(default_value);
    }

    let input = prompt
        .with_help_message("可直接回车接受默认值")
        .prompt()?;

    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    Ok(Some(validate_model(trimmed)?))
}

fn prompt_api_key(selected: &ProviderPreset) -> Result<Option<String>> {
    let api_key = Password::new("🔑 API Key（留空则使用环境变量）:")
        .without_confirmation()
        .with_help_message(&format!(
            "留空将使用环境变量：{}",
            selected.primary_env_key()
        ))
        .prompt()?;

    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

/// 交互式配置流程
pub fn interactive_config(initial_provider: Option<&str>) -> Result<UserConfig> {
    println!();
    println!("══════════════════════════════════════════════════════");
    println!("  Agent Unix 配置向导");
    println!("══════════════════════════════════════════════════════");
    println!();

    let selected = prompt_provider(initial_provider)?;

    println!();
    println!("   ✅ 已选择: {}", selected.display_name);
    println!("   Base URL: {}", if selected.base_url.is_empty() { "(需填写)" } else { &selected.base_url });
    println!("   默认模型: {}", if selected.default_model.is_empty() { "(需填写)" } else { &selected.default_model });
    println!();

    let base_url = prompt_base_url(&selected)?;
    let model = prompt_model(&selected)?;
    let api_key = prompt_api_key(&selected)?;

    println!();
    println!("══════════════════════════════════════════════════════");
    println!("  配置摘要");
    println!("══════════════════════════════════════════════════════");
    println!();
    println!("   Provider: {}", selected.display_name);
    println!(
        "   Base URL: {}",
        base_url.clone().unwrap_or_else(|| selected.base_url.clone())
    );
    println!(
        "   Model: {}",
        model.clone().unwrap_or_else(|| selected.default_model.clone())
    );
    println!(
        "   API Key: {}",
        if api_key.is_some() { "已保存" } else { "使用环境变量" }
    );
    println!();

    let confirm = Confirm::new("保存配置？")
        .with_default(true)
        .prompt()?;

    if confirm {
        let resolved_provider_name = selected.name.clone();
        let config = UserConfig {
            provider: resolved_provider_name.clone(),
            provider_preset: Some(resolved_provider_name),
            base_url: if let Some(url) = base_url {
                Some(url)
            } else if selected.base_url.is_empty() {
                None
            } else {
                Some(selected.base_url.clone())
            },
            model: if let Some(model) = model {
                Some(model)
            } else if selected.default_model.is_empty() {
                None
            } else {
                Some(selected.default_model.clone())
            },
            api_key,
        };

        save_user_config(&config)?;

        println!();
        println!("══════════════════════════════════════════════════════");
        println!("  配置完成！");
        println!("══════════════════════════════════════════════════════");
        println!();

        if config.api_key.is_none() {
            println!("⚠️  还需要设置 API Key：");
            println!("   export {}=你的key", selected.primary_env_key());
            println!();
        }

        println!("🎉 现可以开始对话：");
        println!("   agent-unix chat");
        println!();

        Ok(config)
    } else {
        println!();
        println!("⚠️  配置已取消");
        Err(anyhow::anyhow!("用户取消配置"))
    }
}

/// 显示当前配置
pub fn show_current_config() {
    let path = config_file_path();

    println!("📁 配置文件路径: {}", path.display());
    println!();

    if let Some(config) = load_user_config() {
        let preset_name = config
            .provider_preset
            .as_deref()
            .unwrap_or(config.provider.as_str());
        let preset = find_preset(preset_name);

        println!("📋 当前配置：");
        println!();
        println!(
            "   Provider: {}",
            preset
                .as_ref()
                .map(|p| p.display_name.clone())
                .unwrap_or_else(|| config.provider.clone())
        );
        println!("   Preset: {}", preset_name);

        if let Some(url) = config.base_url {
            println!("   Base URL: {}", url);
        }

        if let Some(model) = config.model {
            println!("   Model: {}", model);
        }

        if let Some(key) = config.api_key {
            println!("   API Key: {}...（已保存）", key.chars().take(8).collect::<String>());
        } else if let Some(preset) = preset {
            println!("   API Key: 使用环境变量 ({})", preset.primary_env_key());
        } else {
            println!("   API Key: 使用环境变量");
        }

        println!();
        println!("✅ 配置有效");
    } else {
        println!("⚠️  配置文件不存在");
        println!();
        println!("💡 创建配置：");
        println!("   agent-unix config --setup");
    }
}

/// 删除配置
pub fn delete_config() -> Result<()> {
    let path = config_file_path();

    if path.exists() {
        std::fs::remove_file(&path)?;
        println!("🗑️  配置文件已删除: {}", path.display());
    } else {
        println!("⚠️  配置文件不存在");
    }

    Ok(())
}
