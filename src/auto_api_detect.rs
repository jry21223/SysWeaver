use std::path::PathBuf;

/// 配置文件解析方式
#[derive(Debug, Clone)]
pub enum SourceFormat {
    /// JSON：按嵌套路径取字符串值，如 &["env", "ANTHROPIC_API_KEY"]
    JsonPath(&'static [&'static str]),
    /// JSON：在顶层对象中尝试多个字段名
    JsonFields(&'static [&'static str]),
    /// TOML：逐行匹配 `api_key = "..."` 正则
    TomlApiKey,
}

/// 一条已知的配置来源描述（仅描述，不含文件内容）
struct ConfigSource {
    tool_name: &'static str,
    file_path: PathBuf,
    provider_name: &'static str,
    format: SourceFormat,
}

/// 扫描结果：文件存在，但尚未读取内容
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub tool_name: &'static str,
    pub file_path: PathBuf,
    #[allow(dead_code)] // 供调用方做 provider 过滤决策，当前由 read_and_extract 内部使用
    pub provider_name: &'static str,
    #[allow(dead_code)] // 供 read_and_extract 内部匹配，不在结构体外直接访问
    pub format: SourceFormat,
}

/// 提取成功的 API Key 记录
#[derive(Debug, Clone)]
pub struct DetectedConfig {
    pub tool_name: String,
    #[allow(dead_code)] // 供调用方展示来源路径，当前仅用于调试
    pub file_path: PathBuf,
    pub provider_name: String,
    pub api_key: String,
}

fn home_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| if cfg!(windows) { "C:\\Temp".to_string() } else { "/tmp".to_string() });
    PathBuf::from(home)
}

/// 所有已知的 AI 工具配置来源
fn config_sources() -> Vec<ConfigSource> {
    let home = home_dir();
    vec![
        // Claude Code — 标准 ANTHROPIC_API_KEY 字段
        ConfigSource {
            tool_name: "Claude Code",
            file_path: home.join(".claude").join("settings.json"),
            provider_name: "anthropic",
            format: SourceFormat::JsonPath(&["env", "ANTHROPIC_API_KEY"]),
        },
        // Claude Code — 备用 ANTHROPIC_AUTH_TOKEN 字段（部分版本使用）
        ConfigSource {
            tool_name: "Claude Code",
            file_path: home.join(".claude").join("settings.json"),
            provider_name: "anthropic",
            format: SourceFormat::JsonPath(&["env", "ANTHROPIC_AUTH_TOKEN"]),
        },
        // Codex CLI auth.json — 支持 api_key / OPENAI_API_KEY 两种字段名
        ConfigSource {
            tool_name: "Codex CLI",
            file_path: home.join(".codex").join("auth.json"),
            provider_name: "openai",
            format: SourceFormat::JsonFields(&["api_key", "OPENAI_API_KEY"]),
        },
        // Codex CLI config.toml — TOML 行级 api_key 字段
        ConfigSource {
            tool_name: "Codex CLI",
            file_path: home.join(".codex").join("config.toml"),
            provider_name: "openai",
            format: SourceFormat::TomlApiKey,
        },
    ]
}

// ── 私有解析辅助 ────────────────────────────────────────────────────────────

fn extract_json_path(content: &str, path: &[&str]) -> Option<String> {
    let mut value: &serde_json::Value = &serde_json::from_str(content).ok()?;
    for key in path {
        value = value.get(key)?;
    }
    value.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

fn extract_json_fields(content: &str, fields: &[&str]) -> Option<String> {
    let obj: serde_json::Value = serde_json::from_str(content).ok()?;
    for field in fields {
        if let Some(v) = obj.get(field).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            return Some(v.to_string());
        }
    }
    None
}

fn extract_toml_api_key(content: &str) -> Option<String> {
    // 匹配：api_key = "sk-..."（允许前导空格，支持 TOML 表内的键）
    let re = regex::Regex::new(r#"^\s*api_key\s*=\s*"([^"]+)""#).ok()?;
    for line in content.lines() {
        if let Some(cap) = re.captures(line) {
            let key = cap.get(1)?.as_str();
            if !key.is_empty() {
                return Some(key.to_string());
            }
        }
    }
    None
}

// ── 公共 API ────────────────────────────────────────────────────────────────

/// 阶段一：仅检查文件是否存在，不读取任何内容。
/// 返回实际存在的配置文件列表，供用户授权前展示路径。
pub fn scan_for_configs() -> Vec<ScanResult> {
    // 去重：同一路径只显示一次（Claude Code settings.json 有两条记录）
    let mut seen_paths: Vec<PathBuf> = Vec::new();
    let mut results: Vec<ScanResult> = Vec::new();

    for src in config_sources() {
        if src.file_path.exists() && !seen_paths.contains(&src.file_path) {
            seen_paths.push(src.file_path.clone());
            results.push(ScanResult {
                tool_name: src.tool_name,
                file_path: src.file_path,
                provider_name: src.provider_name,
                format: src.format,
            });
        }
    }
    results
}

/// 阶段二：在用户同意后调用，读取文件并提取 API Key。
/// 对读取失败或未找到 Key 的条目静默跳过。
/// 为保证重复路径（如 Claude Code 两个字段）都被尝试，此处使用完整的 config_sources()。
pub fn read_and_extract(consented_paths: &[ScanResult]) -> Vec<DetectedConfig> {
    // 仅处理用户同意的路径
    let allowed: std::collections::HashSet<&PathBuf> =
        consented_paths.iter().map(|s| &s.file_path).collect();

    config_sources()
        .into_iter()
        .filter(|src| allowed.contains(&src.file_path))
        .filter_map(|src| {
            let content = std::fs::read_to_string(&src.file_path).ok()?;
            let api_key = match &src.format {
                SourceFormat::JsonPath(path) => extract_json_path(&content, path),
                SourceFormat::JsonFields(fields) => extract_json_fields(&content, fields),
                SourceFormat::TomlApiKey => extract_toml_api_key(&content),
            }?;
            Some(DetectedConfig {
                tool_name: src.tool_name.to_string(),
                file_path: src.file_path,
                provider_name: src.provider_name.to_string(),
                api_key,
            })
        })
        .collect()
}

/// 返回脱敏显示字符串（前 8 位明文 + ****）
pub fn mask_key(key: &str) -> String {
    let visible: String = key.chars().take(8).collect();
    format!("{}****", visible)
}
