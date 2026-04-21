use crate::types::tool::Playbook;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

/// Playbook 来源类型（优先级从高到低）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlaybookSource {
    /// 项目级：`.agent-unix/playbooks/`
    Project = 3,
    /// 用户级：`~/.agent-unix/playbooks/`
    User = 2,
    /// 内置：系统预定义模板
    Bundled = 1,
}

/// Playbook 管理器：负责多来源加载、合并和查询
pub struct PlaybookManager {
    /// 所有 Playbook（按名称索引，同名时高优先级覆盖低优先级）
    playbooks: HashMap<String, Playbook>,
    /// 各来源的加载路径
    source_paths: HashMap<PlaybookSource, PathBuf>,
    /// 统计信息
    stats: LoadStats,
}

#[derive(Debug, Clone, Default)]
pub struct LoadStats {
    pub bundled_count: usize,
    pub user_count: usize,
    pub project_count: usize,
    pub overridden_count: usize,
}

impl PlaybookManager {
    pub fn new() -> Self {
        Self {
            playbooks: HashMap::new(),
            source_paths: HashMap::new(),
            stats: LoadStats::default(),
        }
    }

    /// 初始化并加载所有来源的 Playbook
    pub fn initialize(&mut self, project_dir: Option<&PathBuf>) -> Result<()> {
        // 1. 加载内置 Playbook（最低优先级）
        self.load_bundled_playbooks()?;

        // 2. 加载用户级 Playbook
        self.load_user_playbooks()?;

        // 3. 加载项目级 Playbook（最高优先级）
        if let Some(dir) = project_dir {
            self.load_project_playbooks(dir)?;
        }

        tracing::info!(
            "Playbook 加载完成：内置 {}，用户 {}，项目 {}，覆盖 {}",
            self.stats.bundled_count,
            self.stats.user_count,
            self.stats.project_count,
            self.stats.overridden_count
        );

        Ok(())
    }

    /// 加载内置 Playbook（系统预定义模板）
    fn load_bundled_playbooks(&mut self) -> Result<()> {
        let bundled = self.get_bundled_definitions();

        for playbook in bundled {
            let name = playbook.name.clone();
            if self.playbooks.contains_key(&name) {
                self.stats.overridden_count += 1;
            }
            self.playbooks.insert(name, playbook);
            self.stats.bundled_count += 1;
        }

        Ok(())
    }

    /// 内置 Playbook 定义
    fn get_bundled_definitions(&self) -> Vec<Playbook> {
        use chrono::Utc;
        use serde_json::json;

        vec![
            // 系统维护模板
            Playbook {
                name: "system-health-check".to_string(),
                description: "检查系统健康状态：磁盘、内存、CPU、关键服务".to_string(),
                steps: vec![
                    crate::types::tool::ToolCall {
                        tool: "system.info".to_string(),
                        args: json!({ "query": "disk" }),
                        reason: Some("检查磁盘使用率".to_string()),
                        dry_run: false,
                    },
                    crate::types::tool::ToolCall {
                        tool: "system.info".to_string(),
                        args: json!({ "query": "memory" }),
                        reason: Some("检查内存使用".to_string()),
                        dry_run: false,
                    },
                    crate::types::tool::ToolCall {
                        tool: "system.info".to_string(),
                        args: json!({ "query": "service" }),
                        reason: Some("检查关键服务状态".to_string()),
                        dry_run: false,
                    },
                ],
                created_at: Utc::now(),
                run_count: 0,
            },
            // Web 服务器安装模板
            Playbook {
                name: "install-web-server".to_string(),
                description: "安装并配置 Nginx Web 服务器".to_string(),
                steps: vec![
                    crate::types::tool::ToolCall {
                        tool: "shell.exec".to_string(),
                        args: json!({ "command": "sudo apt update && sudo apt install -y nginx" }),
                        reason: Some("安装 Nginx".to_string()),
                        dry_run: false,
                    },
                    crate::types::tool::ToolCall {
                        tool: "shell.exec".to_string(),
                        args: json!({ "command": "sudo systemctl start nginx && sudo systemctl enable nginx" }),
                        reason: Some("启动并启用 Nginx 服务".to_string()),
                        dry_run: false,
                    },
                    crate::types::tool::ToolCall {
                        tool: "system.info".to_string(),
                        args: json!({ "query": "service", "filter": "nginx" }),
                        reason: Some("验证 Nginx 状态".to_string()),
                        dry_run: false,
                    },
                ],
                created_at: Utc::now(),
                run_count: 0,
            },
            // 日志清理模板
            Playbook {
                name: "cleanup-old-logs".to_string(),
                description: "清理 30 天前的旧日志文件（安全模式）".to_string(),
                steps: vec![
                    crate::types::tool::ToolCall {
                        tool: "shell.exec".to_string(),
                        args: json!({ "command": "find /var/log -type f -mtime +30 | head -20" }),
                        reason: Some("查找超过 30 天的日志文件".to_string()),
                        dry_run: false,
                    },
                    crate::types::tool::ToolCall {
                        tool: "shell.exec".to_string(),
                        args: json!({ "command": "find /var/log -type f -mtime +30 -exec ls -lh {} \\; | awk '{print $5}'" }),
                        reason: Some("计算可释放空间".to_string()),
                        dry_run: false,
                    },
                    crate::types::tool::ToolCall {
                        tool: "shell.exec".to_string(),
                        args: json!({ "command": "find /var/log -type f -mtime +30 -delete" }),
                        reason: Some("删除旧日志文件".to_string()),
                        dry_run: true, // 默认 dry_run，用户确认后执行
                    },
                ],
                created_at: Utc::now(),
                run_count: 0,
            },
        ]
    }

    /// 加载用户级 Playbook（`~/.agent-unix/playbooks/`）
    fn load_user_playbooks(&mut self) -> Result<()> {
        let user_home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| "/tmp".to_string());

        let user_path = PathBuf::from(user_home).join(".agent-unix").join("playbooks");
        self.source_paths.insert(PlaybookSource::User, user_path.clone());

        self.load_from_directory(&user_path, PlaybookSource::User)?;

        Ok(())
    }

    /// 加载项目级 Playbook（`.agent-unix/playbooks/`）
    fn load_project_playbooks(&mut self, project_dir: &PathBuf) -> Result<()> {
        let project_path = project_dir.join(".agent-unix").join("playbooks");
        self.source_paths.insert(PlaybookSource::Project, project_path.clone());

        self.load_from_directory(&project_path, PlaybookSource::Project)?;

        Ok(())
    }

    /// 从指定目录加载 Playbook 文件
    fn load_from_directory(&mut self, dir: &PathBuf, source: PlaybookSource) -> Result<()> {
        if !dir.exists() {
            tracing::debug!("Playbook 目录不存在：{}", dir.display());
            return Ok(());
        }

        let entries = std::fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // 只加载 .json 文件
            if path.extension().map_or(false, |ext| ext == "json") {
                self.load_playbook_file(&path, source)?;
            }
        }

        Ok(())
    }

    /// 加载单个 Playbook 文件
    fn load_playbook_file(&mut self, path: &PathBuf, source: PlaybookSource) -> Result<()> {
        let content = std::fs::read_to_string(path)?;
        let playbook: Playbook = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Playbook 文件解析失败 {}: {}", path.display(), e))?;

        let name = playbook.name.clone();

        // 检查是否被覆盖
        if self.playbooks.contains_key(&name) {
            let existing_source = self.get_playbook_source(&name);
            if source > existing_source {
                tracing::debug!(
                    "Playbook '{}' 被 {} 级覆盖",
                    name,
                    if source == PlaybookSource::Project { "项目" } else { "用户" }
                );
                self.stats.overridden_count += 1;
            }
        }

        self.playbooks.insert(name, playbook);

        match source {
            PlaybookSource::Project => self.stats.project_count += 1,
            PlaybookSource::User => self.stats.user_count += 1,
            PlaybookSource::Bundled => {}
        }

        Ok(())
    }

    /// 获取 Playbook 的来源（根据加载顺序推断）
    fn get_playbook_source(&self, _name: &str) -> PlaybookSource {
        // 简化实现：假设已存在的 Playbook 来源低于新加载的
        // 实际应该在 Playbook 结构中记录来源
        PlaybookSource::Bundled
    }

    /// 查询 Playbook
    pub fn get(&self, name: &str) -> Option<&Playbook> {
        self.playbooks.get(name)
    }

    /// 列出所有 Playbook
    pub fn list(&self) -> Vec<&Playbook> {
        self.playbooks.values().collect()
    }

    /// 搜索 Playbook（按名称或描述）
    pub fn search(&self, keyword: &str) -> Vec<&Playbook> {
        self.playbooks
            .values()
            .filter(|p| {
                p.name.contains(keyword) || p.description.contains(keyword)
            })
            .collect()
    }

    /// 保存 Playbook 到指定来源
    pub fn save(&self, playbook: &Playbook, source: PlaybookSource) -> Result<PathBuf> {
        let dir = self
            .source_paths
            .get(&source)
            .cloned()
            .unwrap_or_else(|| {
                // 默认保存到用户级
                let user_home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                PathBuf::from(user_home).join(".agent-unix").join("playbooks")
            });

        // 创建目录（如果不存在）
        std::fs::create_dir_all(&dir)?;

        let file_path = dir.join(format!("{}.json", playbook.name));
        let content = serde_json::to_string_pretty(playbook)?;
        std::fs::write(&file_path, content)?;

        tracing::info!("Playbook '{}' 已保存到 {}", playbook.name, file_path.display());

        Ok(file_path)
    }

    /// 获取加载统计
    pub fn stats(&self) -> &LoadStats {
        &self.stats
    }
}

impl Default for PlaybookManager {
    fn default() -> Self {
        Self::new()
    }
}