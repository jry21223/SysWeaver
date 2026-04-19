/// 任务规划器：判断输入是单步还是多步任务，生成执行计划
// Phase 4: LLM-based task decomposition — scaffold retained for future integration
#[allow(dead_code)]
pub struct Planner;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum TaskPlan {
    /// 单步任务，直接进入 Agent Loop
    Single { description: String },
    /// 多步任务，包含步骤列表（初始估计，实际执行时 LLM 会细化）
    Multi {
        description: String,
        estimated_steps: Vec<String>,
    },
    /// 模糊任务，需要向用户消歧
    Ambiguous { options: Vec<DisambiguationOption> },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DisambiguationOption {
    pub label: String,       // "A", "B", "C"
    pub description: String, // 这个选项的自然语言说明
    pub preview: String,     // 预计操作的摘要
}

#[allow(dead_code)]
impl Planner {
    pub fn new() -> Self {
        Self
    }

    /// 分析用户输入，返回任务计划
    /// 简单的关键词分析，后期可以让 LLM 来做这一步
    pub fn analyze(&self, input: &str) -> TaskPlan {
        let input_lower = input.to_lowercase();

        // 检查是否为明确的模糊请求
        if self.is_ambiguous(&input_lower) {
            return self.build_disambiguation_options(&input_lower);
        }

        // 检查是否为多步任务
        if self.is_multi_step(&input_lower) {
            return TaskPlan::Multi {
                description: input.to_string(),
                estimated_steps: self.estimate_steps(&input_lower),
            };
        }

        TaskPlan::Single {
            description: input.to_string(),
        }
    }

    fn is_ambiguous(&self, input: &str) -> bool {
        // 过于笼统的指令
        let vague_keywords = ["清理磁盘", "优化系统", "清理空间", "释放内存", "修复问题"];
        vague_keywords.iter().any(|k| input.contains(k))
    }

    fn is_multi_step(&self, input: &str) -> bool {
        // 包含安装+配置、创建+设置等组合操作
        let multi_step_patterns = [
            ("安装", "配置"),
            ("安装", "启动"),
            ("创建", "设置"),
            ("备份", "删除"),
            ("停止", "修改"),
            ("检查", "修复"),
        ];
        multi_step_patterns
            .iter()
            .any(|(a, b)| input.contains(a) && input.contains(b))
    }

    fn estimate_steps(&self, input: &str) -> Vec<String> {
        // 简单的步骤估计，实际由 LLM 在执行时细化
        if input.contains("安装") && input.contains("配置") {
            vec![
                "检查目标软件是否已安装".to_string(),
                "安装软件包".to_string(),
                "修改配置文件".to_string(),
                "重启服务".to_string(),
                "验证服务状态".to_string(),
            ]
        } else if input.contains("备份") {
            vec![
                "检查源文件/目录".to_string(),
                "创建备份".to_string(),
                "验证备份完整性".to_string(),
            ]
        } else {
            vec![
                "分析任务".to_string(),
                "执行操作".to_string(),
                "验证结果".to_string(),
            ]
        }
    }

    fn build_disambiguation_options(&self, input: &str) -> TaskPlan {
        if input.contains("清理磁盘") || input.contains("清理空间") || input.contains("释放空间")
        {
            return TaskPlan::Ambiguous {
                options: vec![
                    DisambiguationOption {
                        label: "A".to_string(),
                        description: "删除 /tmp 临时文件".to_string(),
                        preview: "预计释放 ~1-2GB，操作安全".to_string(),
                    },
                    DisambiguationOption {
                        label: "B".to_string(),
                        description: "删除 30 天前的日志文件".to_string(),
                        preview: "需要先统计文件大小，再确认删除".to_string(),
                    },
                    DisambiguationOption {
                        label: "C".to_string(),
                        description: "找出最大的目录/文件，由你决定".to_string(),
                        preview: "仅查询，不删除，你来决定处理方式".to_string(),
                    },
                ],
            };
        }

        // 默认：返回单步，让 LLM 处理
        TaskPlan::Single {
            description: input.to_string(),
        }
    }
}

impl Default for Planner {
    fn default() -> Self {
        Self::new()
    }
}
