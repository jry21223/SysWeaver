# jij

> 用自然语言管理你的 Linux / macOS / Windows 系统

**jij** 是一个由 AI 驱动的操作系统智能代理，将自然语言指令转换为可控的系统操作，内置五级风险安全层。

---

## 快速开始（1 分钟上手）

### 第一步：设置 API Key

无需任何配置文件，直接设置环境变量即可：

```bash
# 使用 Anthropic Claude（推荐）
export ANTHROPIC_API_KEY=sk-ant-xxxxxxxx

# 或使用 OpenAI GPT
export OPENAI_API_KEY=sk-xxxxxxxx

# 或使用其他 Provider（DeepSeek、Groq、Kimi 等）
export DEEPSEEK_API_KEY=xxxxxxxx
```

jij 会自动检测已设置的 API Key 并选择对应的 Provider，**启动时会显示检测结果**，无需手动指定。

### 第二步：启动

```bash
jij chat          # 交互式 TUI 对话（推荐）
jij chat --no-tui # 纯文本 CLI 模式
jij run "查看磁盘使用情况"  # 单条指令
```

---

## 安装

### 从源码编译

```bash
git clone <repo>
cd agent-unix
cargo build --release
# 二进制文件在 target/release/jij
```

### 添加到 PATH

```bash
# macOS / Linux
cp target/release/jij /usr/local/bin/

# 或添加到 ~/.bashrc / ~/.zshrc
export PATH="$PATH:/path/to/agent-unix/target/release"
```

### Windows

```powershell
# 将编译好的 jij.exe 添加到 PATH
# 设置 API Key
$env:ANTHROPIC_API_KEY = "sk-ant-xxxxxxxx"
jij chat
```

---

## 配置详解

### 方式一：环境变量（推荐，无需配置文件）

| 环境变量 | 对应 Provider | 说明 |
|---------|-------------|------|
| `ANTHROPIC_API_KEY` | Anthropic Claude | 自动检测，优先级最高 |
| `OPENAI_API_KEY` | OpenAI GPT | 自动检测 |
| `DEEPSEEK_API_KEY` | DeepSeek | 自动检测 |
| `GROQ_API_KEY` | Groq | 自动检测 |
| `KIMI_API_KEY` / `MOONSHOT_API_KEY` | Moonshot Kimi | 自动检测 |
| `GLM_API_KEY` / `BIGMODEL_API_KEY` | 智谱 GLM | 自动检测 |
| `BAILIAN_API_KEY` / `DASHSCOPE_API_KEY` | 阿里云百炼 | 自动检测 |
| `MINIMAX_API_KEY` | MiniMax | 自动检测 |
| `OPENROUTER_API_KEY` | OpenRouter | 自动检测 |
| `AGENT_UNIX_LLM_API_KEY` | 通用（最高优先级） | 覆盖所有其他 Key |

**自动检测顺序**：`AGENT_UNIX_LLM_API_KEY` > `ANTHROPIC_API_KEY` > 各 Provider 专属 Key > `OPENAI_API_KEY`

如需自定义模型或 Base URL，也可通过环境变量指定：

```bash
export AGENT_UNIX_LLM_PROVIDER=anthropic         # 强制指定 Provider
export AGENT_UNIX_LLM_BASE_URL=https://your-proxy.com  # 自定义端点
export AGENT_UNIX_LLM_MODEL=claude-opus-4-5       # 指定模型
```

### 方式二：配置文件（可选）

运行交互式配置向导，按提示选择 Provider 并填写 API Key：

```bash
jij config --setup
```

配置文件自动保存到：

| 系统 | 路径 |
|------|------|
| macOS / Linux | `~/.jij/config.json` |
| Windows | `%USERPROFILE%\.jij\config.json` |

查看、删除配置：

```bash
jij config --show    # 查看当前配置
jij config --list    # 列出所有支持的 Provider
jij config --delete  # 删除配置文件
```

### 方式三：CLI 参数（优先级最高）

```bash
jij --provider openai --model gpt-4o chat
jij --provider anthropic --base-url https://my-proxy.com chat
```

### 优先级总结

```
CLI 参数 > 配置文件 > AGENT_UNIX_LLM_* 环境变量 > Provider 专属 API Key 环境变量 > 默认值
```

---

## 其他数据文件路径

| 文件 | macOS / Linux | Windows |
|------|--------------|---------|
| 配置文件 | `~/.jij/config.json` | `%USERPROFILE%\.jij\config.json` |
| 对话历史 | `~/.jij/history.txt` | `%USERPROFILE%\.jij\history.txt` |
| 审计日志 | `~/.jij/audit-YYYYMMDD.jsonl` | `%USERPROFILE%\.jij\audit-YYYYMMDD.jsonl` |
| 用户 Playbook | `~/.jij/playbooks/` | `%USERPROFILE%\.jij\playbooks\` |
| 项目 Playbook | `./.jij/playbooks/` | `.\.jij\playbooks\` |

---

## 常用命令

```bash
jij chat                        # 交互式 TUI 对话
jij chat --no-tui               # CLI 纯文本对话
jij run "查看磁盘使用情况"         # 单条指令执行
jij run --dry-run "清理30天前日志"  # 预览模式（不实际执行）
jij explain /etc/nginx/nginx.conf  # 解释配置文件
jij watch --duration 120         # 后台监控 120 秒
jij info                         # 显示系统信息
jij config --setup               # 交互式配置
```

### 对话内快捷命令

| 命令 | 功能 |
|------|------|
| `/help` | 显示帮助 |
| `/status` | 实时系统状态 |
| `/history` | 操作历史 |
| `/undo` | 撤销上一步 |
| `/clear` | 清除对话上下文 |
| `/report` | 系统健康报告 |
| `/playbook list` | 列出 Playbook |
| `/playbook save <名称>` | 保存当前操作为 Playbook |
| `/playbook run <名称>` | 执行 Playbook |
| `/exit` | 退出 |

### TUI 快捷键

| 快捷键 | 功能 |
|--------|------|
| `Ctrl+Y` | 复制最后一条 Agent 回复到剪贴板 |
| `Ctrl+P / N` | 浏览历史输入 |
| `PgUp / PgDn` | 滚动对话区 |
| `↑ / ↓` | 逐行滚动 |
| `End` | 滚到最底部 |
| `Ctrl+C / Esc` | 退出 |

---

## 安全机制

所有操作在执行前经过五级风险分类：

| 级别 | 处理方式 | 示例 |
|------|---------|------|
| **Critical** | 直接拒绝，不可确认 | `rm -rf /`、删除 `/etc/passwd` |
| **High** | 弹窗要求输入 `Y` 确认 | 停止 SSH 服务、格式化磁盘 |
| **Medium** | `--mode safe` 下要求确认 | 修改系统配置文件 |
| **Low** | 直接执行 | 查看文件内容 |
| **Safe** | 直接执行 | 读取系统信息 |

```bash
jij --mode safe chat   # 对 Medium 风险也要求确认
jij --mode auto chat   # 自动模式（Medium 不确认）
```

---

## 支持的 Provider

| 名称 | CLI `--provider` | 默认模型 |
|------|----------------|---------|
| Anthropic | `anthropic` | `claude-sonnet-4-5` |
| OpenAI | `openai` | `gpt-4o` |
| DeepSeek | `deepseek` | `deepseek-chat` |
| Groq | `groq` | `llama-3.3-70b-versatile` |
| Moonshot Kimi | `kimi` | `moonshot-v1-128k` |
| 智谱 GLM | `glm` | `glm-4.5` |
| 阿里云百炼 | `bailian` | `qwen-coding-plus` |
| MiniMax | `minimax` | `MiniMax-M1` |
| OpenRouter | `openrouter` | `openai/gpt-4o` |
| 自定义端点 | `custom` | 手动填写 |

---

## SSH 远程模式

```bash
# 连接到远程服务器执行操作
jij --ssh root@192.168.1.100 chat
jij --ssh admin@server.example.com:2222 --ssh-key ~/.ssh/id_ed25519 chat
```
