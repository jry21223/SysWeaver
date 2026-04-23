use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;

use super::Tool;
use crate::types::tool::ToolResult;

/// 软件包管理工具 — 自动检测包管理器并安装/卸载/查询软件
pub struct PackageTool;

impl PackageTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for PackageTool {
    fn name(&self) -> &str {
        "package.manage"
    }

    fn description(&self) -> &str {
        "管理系统软件包：安装、卸载、查询、更新软件。\
         自动检测包管理器（apt/yum/dnf/pacman/brew/apk），\
         适配当前操作系统环境。安装/卸载属于中等风险，全量升级属于高风险。"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["install", "remove", "search", "info", "list-installed", "update-cache", "upgrade-all"],
                    "description": "操作：install=安装, remove=卸载, search=搜索, info=查询信息, list-installed=列出已安装, update-cache=更新包索引, upgrade-all=升级所有包"
                },
                "package": {
                    "type": "string",
                    "description": "包名（install/remove/search/info 时必填）"
                },
                "manager": {
                    "type": "string",
                    "enum": ["auto", "apt", "yum", "dnf", "pacman", "brew", "apk"],
                    "description": "指定包管理器，默认 auto（自动检测）"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("缺少 action 参数"))?;

        let manager_hint = args["manager"].as_str().unwrap_or("auto");
        let manager = if manager_hint == "auto" {
            detect_package_manager().await
        } else {
            manager_hint.to_string()
        };

        if manager == "unknown" {
            return Ok(ToolResult::failure(
                self.name(),
                "未检测到支持的包管理器（apt/yum/dnf/pacman/brew/apk）",
                1,
            ));
        }

        match action {
            "install" => {
                let pkg = args["package"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("install 操作需要 package 参数"))?;
                validate_package_name(pkg)?;

                if dry_run {
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        &format!("[{}] 将安装软件包: {}", manager, pkg),
                    ));
                }
                run_install(&manager, pkg).await
            }

            "remove" => {
                let pkg = args["package"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("remove 操作需要 package 参数"))?;
                validate_package_name(pkg)?;

                if dry_run {
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        &format!("[{}] 将卸载软件包: {}", manager, pkg),
                    ));
                }
                run_remove(&manager, pkg).await
            }

            "search" => {
                let pkg = args["package"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("search 操作需要 package 参数"))?;
                validate_package_name(pkg)?;

                if dry_run {
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        &format!("[{}] 将搜索软件包: {}", manager, pkg),
                    ));
                }
                run_search(&manager, pkg).await
            }

            "info" => {
                let pkg = args["package"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("info 操作需要 package 参数"))?;
                validate_package_name(pkg)?;

                if dry_run {
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        &format!("[{}] 将查询软件包信息: {}", manager, pkg),
                    ));
                }
                run_info(&manager, pkg).await
            }

            "list-installed" => {
                if dry_run {
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        &format!("[{}] 将列出已安装的软件包", manager),
                    ));
                }
                run_list_installed(&manager).await
            }

            "update-cache" => {
                if dry_run {
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        &format!("[{}] 将更新软件包索引缓存", manager),
                    ));
                }
                run_update_cache(&manager).await
            }

            "upgrade-all" => {
                if dry_run {
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        &format!("[{}] 将升级所有已安装的软件包（高风险操作）", manager),
                    ));
                }
                run_upgrade_all(&manager).await
            }

            _ => Ok(ToolResult::failure(self.name(), &format!("不支持的操作: {}", action), 1)),
        }
    }
}

/// 自动检测系统包管理器
async fn detect_package_manager() -> String {
    // 按优先级检测
    let candidates = [
        ("apt-get", "apt"),
        ("dnf", "dnf"),
        ("yum", "yum"),
        ("pacman", "pacman"),
        ("brew", "brew"),
        ("apk", "apk"),
    ];

    for (bin, name) in &candidates {
        if let Ok(out) = Command::new("which").arg(bin).output().await {
            if out.status.success() {
                return name.to_string();
            }
        }
    }
    "unknown".to_string()
}

/// 校验包名（防止注入）
fn validate_package_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow::anyhow!("包名不能为空"));
    }
    // 允许：字母、数字、连字符、点、加号、下划线、冒号（版本约束）
    let valid = name
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '.' | '+' | '_' | ':' | '='));
    if !valid {
        return Err(anyhow::anyhow!("包名包含非法字符: {}", name));
    }
    Ok(())
}

async fn run_install(manager: &str, pkg: &str) -> Result<ToolResult> {
    let (cmd, args): (&str, Vec<&str>) = match manager {
        "apt"    => ("apt-get", vec!["-y", "install", pkg]),
        "dnf"    => ("dnf", vec!["-y", "install", pkg]),
        "yum"    => ("yum", vec!["-y", "install", pkg]),
        "pacman" => ("pacman", vec!["--noconfirm", "-S", pkg]),
        "brew"   => ("brew", vec!["install", pkg]),
        "apk"    => ("apk", vec!["add", pkg]),
        _        => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
    };

    exec_tool_cmd(cmd, &args).await
}

async fn run_remove(manager: &str, pkg: &str) -> Result<ToolResult> {
    let (cmd, args): (&str, Vec<&str>) = match manager {
        "apt"    => ("apt-get", vec!["-y", "remove", pkg]),
        "dnf"    => ("dnf", vec!["-y", "remove", pkg]),
        "yum"    => ("yum", vec!["-y", "remove", pkg]),
        "pacman" => ("pacman", vec!["--noconfirm", "-R", pkg]),
        "brew"   => ("brew", vec!["uninstall", pkg]),
        "apk"    => ("apk", vec!["del", pkg]),
        _        => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
    };

    exec_tool_cmd(cmd, &args).await
}

async fn run_search(manager: &str, pkg: &str) -> Result<ToolResult> {
    let (cmd, args): (&str, Vec<&str>) = match manager {
        "apt"    => ("apt-cache", vec!["search", pkg]),
        "dnf"    => ("dnf", vec!["search", pkg]),
        "yum"    => ("yum", vec!["search", pkg]),
        "pacman" => ("pacman", vec!["-Ss", pkg]),
        "brew"   => ("brew", vec!["search", pkg]),
        "apk"    => ("apk", vec!["search", pkg]),
        _        => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
    };

    exec_tool_cmd(cmd, &args).await
}

async fn run_info(manager: &str, pkg: &str) -> Result<ToolResult> {
    let (cmd, args): (&str, Vec<&str>) = match manager {
        "apt"    => ("apt-cache", vec!["show", pkg]),
        "dnf"    => ("dnf", vec!["info", pkg]),
        "yum"    => ("yum", vec!["info", pkg]),
        "pacman" => ("pacman", vec!["-Si", pkg]),
        "brew"   => ("brew", vec!["info", pkg]),
        "apk"    => ("apk", vec!["info", "-a", pkg]),
        _        => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
    };

    exec_tool_cmd(cmd, &args).await
}

async fn run_list_installed(manager: &str) -> Result<ToolResult> {
    let (cmd, args): (&str, Vec<&str>) = match manager {
        "apt"    => ("dpkg", vec!["--get-selections"]),
        "dnf"    => ("dnf", vec!["list", "installed"]),
        "yum"    => ("yum", vec!["list", "installed"]),
        "pacman" => ("pacman", vec!["-Q"]),
        "brew"   => ("brew", vec!["list"]),
        "apk"    => ("apk", vec!["list", "--installed"]),
        _        => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
    };

    exec_tool_cmd(cmd, &args).await
}

async fn run_update_cache(manager: &str) -> Result<ToolResult> {
    let (cmd, args): (&str, Vec<&str>) = match manager {
        "apt"    => ("apt-get", vec!["update"]),
        "dnf"    => ("dnf", vec!["check-update"]),
        "yum"    => ("yum", vec!["check-update"]),
        "pacman" => ("pacman", vec!["-Sy"]),
        "brew"   => ("brew", vec!["update"]),
        "apk"    => ("apk", vec!["update"]),
        _        => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
    };

    exec_tool_cmd(cmd, &args).await
}

async fn run_upgrade_all(manager: &str) -> Result<ToolResult> {
    let (cmd, args): (&str, Vec<&str>) = match manager {
        "apt"    => ("apt-get", vec!["-y", "upgrade"]),
        "dnf"    => ("dnf", vec!["-y", "upgrade"]),
        "yum"    => ("yum", vec!["-y", "update"]),
        "pacman" => ("pacman", vec!["--noconfirm", "-Su"]),
        "brew"   => ("brew", vec!["upgrade"]),
        "apk"    => ("apk", vec!["upgrade"]),
        _        => return Ok(ToolResult::failure("package.manage", "不支持的包管理器", 1)),
    };

    exec_tool_cmd(cmd, &args).await
}

async fn exec_tool_cmd(cmd: &str, args: &[&str]) -> Result<ToolResult> {
    let start = std::time::Instant::now();
    let output = Command::new(cmd)
        .args(args)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("执行 {} 失败: {}", cmd, e))?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    Ok(ToolResult {
        success: output.status.success(),
        tool: "package.manage".to_string(),
        stdout,
        stderr,
        exit_code,
        duration_ms,
        dry_run_preview: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_package_names() {
        assert!(validate_package_name("nginx").is_ok());
        assert!(validate_package_name("python3-pip").is_ok());
        assert!(validate_package_name("g++").is_ok());
        assert!(validate_package_name("libssl1.1").is_ok());
        assert!(validate_package_name("gcc:amd64").is_ok());
        assert!(validate_package_name("package=1.2.3").is_ok());
    }

    #[test]
    fn rejects_empty_package_name() {
        assert!(validate_package_name("").is_err());
    }

    #[test]
    fn rejects_package_names_with_shell_metacharacters() {
        assert!(validate_package_name("nginx; rm -rf /").is_err());
        assert!(validate_package_name("pkg && evil").is_err());
        assert!(validate_package_name("$(cmd)").is_err());
        assert!(validate_package_name("pkg`cmd`").is_err());
        assert!(validate_package_name("pkg|pipe").is_err());
    }

    #[test]
    fn package_tool_has_correct_name() {
        assert_eq!(PackageTool::new().name(), "package.manage");
    }

    #[tokio::test]
    async fn dry_run_install_returns_preview() {
        let tool = PackageTool::new();
        let args = serde_json::json!({"action": "install", "package": "nginx"});
        let result = tool.execute(&args, true).await.unwrap();
        assert!(result.dry_run_preview.is_some());
        let preview = result.dry_run_preview.unwrap();
        assert!(preview.contains("nginx"));
        assert!(preview.contains("安装"));
    }

    #[tokio::test]
    async fn dry_run_remove_returns_preview() {
        let tool = PackageTool::new();
        let args = serde_json::json!({"action": "remove", "package": "curl"});
        let result = tool.execute(&args, true).await.unwrap();
        assert!(result.dry_run_preview.is_some());
    }

    #[tokio::test]
    async fn dry_run_upgrade_all_returns_preview() {
        let tool = PackageTool::new();
        let args = serde_json::json!({"action": "upgrade-all"});
        let result = tool.execute(&args, true).await.unwrap();
        assert!(result.dry_run_preview.is_some());
        let preview = result.dry_run_preview.unwrap();
        assert!(preview.contains("高风险") || preview.contains("升级"));
    }

    #[tokio::test]
    async fn missing_action_returns_error() {
        let tool = PackageTool::new();
        let args = serde_json::json!({"package": "nginx"});
        let result = tool.execute(&args, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn install_without_package_returns_error() {
        let tool = PackageTool::new();
        let args = serde_json::json!({"action": "install"});
        let result = tool.execute(&args, false).await;
        assert!(result.is_err());
    }
}
