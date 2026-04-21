use anyhow::Result;
use std::path::Path;

/// 反向解释器：读取配置/日志文件，用 LLM 生成自然语言解释
pub struct Explainer {
    /// 支持的文件类型
    supported_types: Vec<FileType>,
}

#[derive(Debug, Clone)]
pub enum FileType {
    /// 系统配置文件
    Config { name: String, path: String },
    /// 服务日志
    Log { name: String, path: String },
    /// 系统状态文件
    System { name: String, path: String },
}

impl Explainer {
    pub fn new() -> Self {
        Self {
            supported_types: vec![
                FileType::Config {
                    name: "nginx配置".to_string(),
                    path: "/etc/nginx/nginx.conf".to_string(),
                },
                FileType::Config {
                    name: "SSH配置".to_string(),
                    path: "/etc/ssh/sshd_config".to_string(),
                },
                FileType::Config {
                    name: "系统日志配置".to_string(),
                    path: "/etc/rsyslog.conf".to_string(),
                },
                FileType::Config {
                    name: "cron定时任务".to_string(),
                    path: "/etc/crontab".to_string(),
                },
                FileType::Log {
                    name: "系统日志".to_string(),
                    path: "/var/log/syslog".to_string(),
                },
                FileType::Log {
                    name: "认证日志".to_string(),
                    path: "/var/log/auth.log".to_string(),
                },
                FileType::Log {
                    name: "Nginx访问日志".to_string(),
                    path: "/var/log/nginx/access.log".to_string(),
                },
                FileType::System {
                    name: "主机信息".to_string(),
                    path: "/etc/hostname".to_string(),
                },
                FileType::System {
                    name: "用户列表".to_string(),
                    path: "/etc/passwd".to_string(),
                },
                FileType::System {
                    name: " fstab挂载配置".to_string(),
                    path: "/etc/fstab".to_string(),
                },
            ],
        }
    }

    /// 列出所有可解释的文件
    pub fn list_supported_files(&self) -> Vec<(String, String)> {
        self.supported_types
            .iter()
            .map(|ft| {
                let (name, path) = match ft {
                    FileType::Config { name, path } => (name, path),
                    FileType::Log { name, path } => (name, path),
                    FileType::System { name, path } => (name, path),
                };
                (format!("{} ({})", name, path), path.clone())
            })
            .collect()
    }

    /// 读取文件内容
    pub fn read_file(&self, path: &str) -> Result<String> {
        let path = Path::new(path);

        if !path.exists() {
            return Err(anyhow::anyhow!("文件不存在: {}", path.display()));
        }

        // 检查文件大小（避免读取超大文件）
        let metadata = std::fs::metadata(path)?;
        if metadata.len() > 1024 * 1024 {
            // 超过 1MB
            return Err(anyhow::anyhow!(
                "文件过大（{}字节），建议使用 file.search 搜索关键内容",
                metadata.len()
            ));
        }

        let content = std::fs::read_to_string(path)?;
        Ok(content)
    }

    /// 生成解释提示词
    pub fn build_explanation_prompt(&self, file_type: &str, content: &str) -> String {
        format!(
            r#"请解释以下 {} 文件的内容，用简洁的中文说明：

【文件内容】
```
{}
```

【解释要求】
1. 说明文件的作用和用途
2. 列出主要配置项及其含义
3. 暂不需要指出潜在问题或优化建议（除非有明显的安全问题）
4. 如果是日志文件，总结最近发生的事件

请用自然语言解释，避免技术术语堆砌。"#,
            file_type, content
        )
    }

    /// 检测文件类型
    pub fn detect_type(&self, path: &str) -> Option<String> {
        for ft in &self.supported_types {
            let ft_path = match ft {
                FileType::Config { name, path } => (name, path),
                FileType::Log { name, path } => (name, path),
                FileType::System { name, path } => (name, path),
            };
            if ft_path.1 == path {
                return Some(ft_path.0.clone());
            }
        }
        None
    }
}

impl Default for Explainer {
    fn default() -> Self {
        Self::new()
    }
}