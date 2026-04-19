use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;

use super::Tool;
use crate::types::tool::ToolResult;

// ─── 受保护路径（file.read / file.write 禁止直接操作） ─────────────────────
const PROTECTED_PATHS: &[&str] = &[
    "/etc/passwd",
    "/etc/shadow",
    "/etc/sudoers",
    "/etc/ssh/",
    "/boot/",
    "/dev/",
    "/proc/",
    "/sys/",
];

fn is_protected_path(path: &str) -> bool {
    PROTECTED_PATHS.iter().any(|p| path.starts_with(p))
}

/// 解析并规范化路径，防止路径遍历攻击
/// 返回规范化的绝对路径字符串，或错误
async fn canonicalize_path(raw: &str) -> Result<String> {
    // 对于不存在的文件（write 场景），canonicalize 会失败
    // 只规范化已存在路径的父目录，再拼接文件名
    let path = std::path::Path::new(raw);
    let canonical = if path.exists() {
        tokio::fs::canonicalize(path).await?
    } else {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("无效路径: 无法获取父目录"))?;
        let canonical_parent = tokio::fs::canonicalize(parent)
            .await
            .map_err(|_| anyhow::anyhow!("父目录不存在: {:?}", parent))?;
        let file_name = path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("无效路径: 无文件名"))?;
        canonical_parent.join(file_name)
    };
    Ok(canonical.to_string_lossy().to_string())
}

// ─── file.read ────────────────────────────────────────────────────────────

pub struct FileReadTool;

impl FileReadTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file.read"
    }

    fn description(&self) -> &str {
        "读取服务器上的文件内容。支持读取配置文件、日志等文本文件。\
         可指定读取行数范围，支持从头或从尾读取。"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "文件绝对路径" },
                "max_lines": {
                    "type": "integer",
                    "description": "最多读取行数，默认 200",
                    "default": 200
                },
                "tail": {
                    "type": "boolean",
                    "description": "true 则读取最后 N 行（类似 tail 命令），默认 false",
                    "default": false
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult> {
        let raw_path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("缺少 path 参数"))?;
        let max_lines = args["max_lines"].as_u64().unwrap_or(200) as usize;
        let tail = args["tail"].as_bool().unwrap_or(false);

        if dry_run {
            return Ok(ToolResult::dry_run_preview(
                self.name(),
                &format!("将读取文件: {}", raw_path),
            ));
        }

        // 规范化路径防止路径遍历（../../../etc/shadow 等）
        let path = canonicalize_path(raw_path)
            .await
            .map_err(|e| anyhow::anyhow!("路径解析失败 {}: {}", raw_path, e))?;

        if is_protected_path(&path) {
            return Ok(ToolResult::failure(
                self.name(),
                &format!("拒绝读取受保护路径: {}", path),
                -1,
            ));
        }

        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| anyhow::anyhow!("读取文件失败 {}: {}", path, e))?;

        let lines: Vec<&str> = content.lines().collect();
        let selected: Vec<&str> = if tail {
            lines
                .iter()
                .rev()
                .take(max_lines)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect()
        } else {
            lines.into_iter().take(max_lines).collect()
        };

        Ok(ToolResult::success(self.name(), &selected.join("\n"), 0))
    }
}

// ─── file.write ───────────────────────────────────────────────────────────

pub struct FileWriteTool;

impl FileWriteTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file.write"
    }

    fn description(&self) -> &str {
        "向服务器上的文件写入内容。支持覆盖写（overwrite）或追加写（append）。\
         系统关键文件（/etc/passwd, /etc/shadow 等）受保护，不可直接写入。"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "文件绝对路径" },
                "content": { "type": "string", "description": "要写入的文本内容" },
                "mode": {
                    "type": "string",
                    "enum": ["overwrite", "append"],
                    "description": "写入模式：overwrite（覆盖）| append（追加），默认 overwrite",
                    "default": "overwrite"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult> {
        let raw_path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("缺少 path 参数"))?;
        let content = args["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("缺少 content 参数"))?;
        let mode = args["mode"].as_str().unwrap_or("overwrite");

        // 规范化路径防止路径遍历
        let path = canonicalize_path(raw_path)
            .await
            .map_err(|e| anyhow::anyhow!("路径解析失败 {}: {}", raw_path, e))?;

        // 受保护路径检查（在规范化后检查，防止 /etc/../etc/shadow 绕过）
        if is_protected_path(&path) {
            return Ok(ToolResult::failure(
                self.name(),
                &format!(
                    "拒绝写入受保护路径: {}。请通过 shell.exec 并经安全确认后操作。",
                    path
                ),
                -1,
            ));
        }

        if dry_run {
            let preview_len = content.chars().count().min(100);
            let preview: String = content.chars().take(preview_len).collect();
            return Ok(ToolResult::dry_run_preview(
                self.name(),
                &format!(
                    "将{}写入文件: {}\n内容预览（前100字）: {}",
                    if mode == "append" { "追加" } else { "覆盖" },
                    path,
                    preview
                ),
            ));
        }

        // 确保父目录存在
        if let Some(parent) = std::path::Path::new(&path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| anyhow::anyhow!("创建目录失败: {}", e))?;
        }

        if mode == "append" {
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
                .map_err(|e| anyhow::anyhow!("打开文件失败 {}: {}", path, e))?;
            file.write_all(content.as_bytes())
                .await
                .map_err(|e| anyhow::anyhow!("写入失败: {}", e))?;
        } else {
            tokio::fs::write(&path, content)
                .await
                .map_err(|e| anyhow::anyhow!("写入文件失败 {}: {}", path, e))?;
        }

        Ok(ToolResult::success(
            self.name(),
            &format!(
                "已{}写入 {} 字节到 {}",
                if mode == "append" { "追加" } else { "覆盖" },
                content.len(),
                path
            ),
            0,
        ))
    }
}

// ─── file.search ──────────────────────────────────────────────────────────

pub struct FileSearchTool;

impl FileSearchTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for FileSearchTool {
    fn name(&self) -> &str {
        "file.search"
    }

    fn description(&self) -> &str {
        "在文件或目录中搜索内容（类似 grep）。支持按关键词搜索文件内容，\
         或按文件名模式查找文件（类似 find）。"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "搜索关键词（内容搜索）或文件名 glob 模式（文件查找）"
                },
                "path": {
                    "type": "string",
                    "description": "搜索路径，默认为当前目录",
                    "default": "."
                },
                "mode": {
                    "type": "string",
                    "enum": ["content", "filename"],
                    "description": "搜索模式：content（搜索文件内容）| filename（按文件名查找），默认 content",
                    "default": "content"
                },
                "max_results": {
                    "type": "integer",
                    "description": "最大返回结果数，默认 50",
                    "default": 50
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("缺少 pattern 参数"))?;
        let path = args["path"].as_str().unwrap_or(".");
        let mode = args["mode"].as_str().unwrap_or("content");
        let max_results = args["max_results"].as_u64().unwrap_or(50);

        if dry_run {
            return Ok(ToolResult::dry_run_preview(
                self.name(),
                &format!(
                    "将在 {} 中{}搜索: {}",
                    path,
                    if mode == "filename" {
                        "按文件名"
                    } else {
                        "按内容"
                    },
                    pattern
                ),
            ));
        }

        // 使用 Command::new 直接传参（不经 shell 插值，防止注入）
        let output = if mode == "filename" {
            Command::new("find")
                .arg(path)
                .arg("-name")
                .arg(pattern)
                .output()
                .await
        } else {
            Command::new("grep")
                .arg("-rn")
                .arg("--include=*.txt")
                .arg("--include=*.log")
                .arg("--include=*.conf")
                .arg("--include=*.cfg")
                .arg("--include=*.json")
                .arg("--include=*.yaml")
                .arg("--include=*.yml")
                .arg("--include=*.sh")
                .arg("--include=*.py")
                .arg("--include=*.js")
                .arg("--include=*.ts")
                .arg(format!("-m{}", max_results))
                .arg(pattern)
                .arg(path)
                .output()
                .await
        };

        let raw = match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
            Err(e) => return Err(anyhow::anyhow!("搜索命令执行失败: {}", e)),
        };

        // 限制返回行数
        let result: String = raw
            .lines()
            .take(max_results as usize)
            .collect::<Vec<_>>()
            .join("\n");

        let result = if result.trim().is_empty() {
            format!("未找到匹配 '{}' 的结果", pattern)
        } else {
            result
        };

        Ok(ToolResult::success(self.name(), &result, 0))
    }
}
