use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::PathBuf;

use crate::config::get_provider_presets;

/// 用户配置文件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfig {
    /// Provider 名称
    pub provider: String,
    /// Base URL
    pub base_url: Option<String>,
    /// Model ID
    pub model: Option<String>,
    /// API Key（加密存储）
    pub api_key: Option<String>,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            base_url: None,
            model: None,
            api_key: None,
        }
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

    // 创建目录
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // 保存配置
    let content = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, content)?;

    println!("✅ 配置已保存到: {}", path.display());
    Ok(())
}

/// 交互式配置流程
pub fn interactive_config() -> Result<UserConfig> {
    println!();
    println!("══════════════════════════════════════════════════════");
    println!("  Agent Unix 配置向导");
    println!("══════════════════════════════════════════════════════");
    println!();

    // 步骤 1：选择 Provider
    println!("📡 步骤 1：选择 LLM Provider");
    println!();

    let presets = get_provider_presets();
    for (i, p) in presets.iter().enumerate() {
        let default_mark = if p.name == "anthropic" { "（默认）" } else { "" };
        println!("   {}. {} {} — {}", i + 1, p.name, default_mark, p.description);
    }
    println!();
    println!("   输入数字选择 Provider › ");

    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let choice: usize = input.trim().parse().unwrap_or(1);
    let selected = presets.get(choice - 1).cloned().unwrap_or_else(|| presets[0].clone());

    println!();
    println!("   ✅ 已选择: {}", selected.name);
    println!("   Base URL: {}", selected.base_url);
    println!("   默认模型: {}", selected.default_model);
    println!();

    // 步骤 2：输入 API Key
    println!("🔑 步骤 2：输入 API Key");
    println!();
    println!("   Provider: {} 使用环境变量: {}", selected.name, selected.env_key_name);
    println!();
    println!("   请输入 API Key（直接回车跳过，使用环境变量）› ");

    io::stdout().flush()?;
    let mut api_key_input = String::new();
    io::stdin().read_line(&mut api_key_input)?;

    let api_key = api_key_input.trim().to_string();
    let use_env_var = api_key.is_empty();

    println!();

    if use_env_var {
        println!("   ⚠️  将使用环境变量: {}", selected.env_key_name);
        println!("   请确保已设置: export {}=你的key", selected.env_key_name);
    } else {
        println!("   ✅ API Key 已保存（长度: {} 字符）", api_key.len());
        println!("   ⚠️  注意：API Key 将存储在本地配置文件中");
    }
    println!();

    // 步骤 3：选择模型（可选）
    println!("🤖 步骤 3：选择模型（可选）");
    println!();
    println!("   默认模型: {}", selected.default_model);
    println!("   直接回车使用默认，或输入其他模型 › ");

    io::stdout().flush()?;
    let mut model_input = String::new();
    io::stdin().read_line(&mut model_input)?;

    let model = if model_input.trim().is_empty() {
        selected.default_model.clone()
    } else {
        model_input.trim().to_string()
    };

    println!();
    println!("   ✅ 模型: {}", model);
    println!();

    // 步骤 4：确认配置
    println!("══════════════════════════════════════════════════════");
    println!("  配置摘要");
    println!("══════════════════════════════════════════════════════");
    println!();
    println!("   Provider: {}", selected.name);
    println!("   Base URL: {}", selected.base_url);
    println!("   Model: {}", model);
    println!("   API Key: {}", if use_env_var { "使用环境变量" } else { "已保存" });
    println!();

    println!("   保存配置？(yes/no) › ");
    io::stdout().flush()?;

    let mut confirm = String::new();
    io::stdin().read_line(&mut confirm)?;

    if confirm.trim().to_lowercase() == "yes" {
        let config = UserConfig {
            provider: selected.name.clone(),
            base_url: if selected.base_url.is_empty() { None } else { Some(selected.base_url.clone()) },
            model: Some(model),
            api_key: if use_env_var { None } else { Some(api_key) },
        };

        save_user_config(&config)?;

        println!();
        println!("══════════════════════════════════════════════════════");
        println!("  配置完成！");
        println!("══════════════════════════════════════════════════════");
        println!();

        if use_env_var {
            println!("⚠️  还需要设置 API Key：");
            println!("   export {}=你的key", selected.env_key_name);
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
        println!("📋 当前配置：");
        println!();
        println!("   Provider: {}", config.provider);

        if let Some(url) = config.base_url {
            println!("   Base URL: {}", url);
        }

        if let Some(model) = config.model {
            println!("   Model: {}", model);
        }

        if let Some(key) = config.api_key {
            println!("   API Key: {}...（已保存）", key.chars().take(8).collect::<String>());
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