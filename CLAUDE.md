# Agent Unix — CLAUDE.md

> 这是 Claude Code 的项目上下文文件。每次开始工作前请先阅读此文件。

## 项目概述

**Agent Unix** 是一个用 Rust 编写的操作系统智能代理，将自然语言转换为可控的 Linux 系统操作。

- 比赛：AI Hackathon 2026 · 超聚变 αFUSION 预赛
- 语言：**Rust 1.75+**（强约束，不得改用其他语言）
- 核心架构：工具驱动的 ReAct Loop + 五级风险分类安全层

## 项目结构

```
src/
├── main.rs              # CLI 入口（clap），命令：chat / run / undo / playbooks
├── config.rs            # ★ LLM Provider 配置（Anthropic / OpenAI-compatible）
├── agent/
│   ├── loop.rs          # ★ Agent 核心循环（ReAct 模式已实现）
│   ├── planner.rs       # 任务分解（单步/多步/消歧）
│   └── memory.rs        # 会话历史 + 操作记录 + Playbook
├── llm/
│   ├── client.rs        # ★ LLM API 调用（支持 Anthropic + OpenAI-compatible tool_use）
│   └── prompt.rs        # System Prompt 构建（注入系统上下文）
├── tools/
│   ├── mod.rs           # Tool trait + ToolManager dispatcher
│   ├── shell.rs         # shell.exec（受限执行）
│   ├── file.rs          # file.read / file.write / file.search
│   └── system.rs        # system.info（disk/memory/process/user/service）
├── safety/
│   ├── classifier.rs    # 五级风险分类（regex 规则引擎）
│   ├── patterns.rs      # CRITICAL/HIGH/MEDIUM 规则库
│   └── audit.rs         # 审计日志（JSON Lines）
├── executor/
│   └── local.rs         # 本地 subprocess 执行器
├── context/
│   └── system_scan.rs   # 启动时系统环境扫描
└── types/
    ├── tool.rs           # ToolCall / ToolResult / OperationRecord / Playbook
    ├── risk.rs           # RiskLevel 枚举（Safe/Low/Medium/High/Critical）
    └── error.rs          # AgentError 统一错误类型
```

## 当前开发状态

### ✅ 已完成
- 所有模块的文件和基本类型定义
- 五级风险分类器（`safety/classifier.rs` + `patterns.rs`）
- 工具框架（`Tool` trait + `ToolManager`）
- 基础工具：`shell.exec`、`file.read`、`file.write`、`file.search`、`system.info`
- **LLM Client**（`llm/client.rs`）— 支持自定义 Provider
- System Prompt 构建（`llm/prompt.rs`）
- 系统环境扫描（`context/system_scan.rs`）
- CLI 入口（`main.rs`）— 支持交互式 chat 和单条 run
- **Agent 核心循环**（`agent/loop.rs`）— ReAct 模式已实现
- **自定义 Provider 配置**（`config.rs`）— 支持 Anthropic 和 OpenAI-compatible

### 🚧 需要实现（按优先级）

**Phase 2（安全，对应 15 分）**

1. **确认对话框 UI** — HIGH 风险时终端交互确认
   - 展示：风险等级、具体原因、影响范围、替代建议
   - 等待用户输入 `yes` 才继续

2. **Dry-Run 模式** — `--dry-run` 标志透传到所有工具
   - `ToolCall.dry_run = true` 时，工具返回预览而不执行

**Phase 3（环境感知，对应 20 分）**

3. **`memory.rs`** 中的系统状态更新机制
   - 多轮对话后，主动刷新系统状态快照

**Phase 4（连续任务，对应 15 分）**

4. **`agent/planner.rs`** — 接入 LLM 做真正的任务分解（现在是规则匹配）

5. **Undo 功能** — `main.rs` 中 `Commands::Undo`
   - 从 `Memory.last_undoable()` 取回滚方案并执行

6. **Playbook** — `Commands::Playbooks` 和保存/读取逻辑

**Phase 5（创新，对应主观分）**
7. **反向解释模式** — 读取配置文件并让 LLM 解释
8. **Watchdog** — `tokio::spawn` 后台监控 + `mpsc::channel` 发送告警

## 关键约束（必须遵守）

### 绝对禁止
- ❌ 不得让 LLM 直接生成 bash 字符串后不经安全层就执行
- ❌ 不得绕过 `RiskClassifier`，即使是"测试代码"
- ❌ 不得在代码里硬编码 API Key（使用环境变量）
- ❌ 不得删除 `audit.rs` 的审计日志逻辑
- ❌ Claude 模型不得配置到 `openai-compatible` provider

### 必须保持
- ✅ 所有工具调用路径：`ToolCall` → `RiskClassifier` → 用户确认（如需）→ `ToolManager.dispatch()`
- ✅ `RiskLevel::Critical` 的操作必须直接拒绝，不给用户确认机会
- ✅ 所有 `Tool::execute` 实现都必须处理 `dry_run: bool` 参数
- ✅ Rust 编译必须无 `warning`（使用 `#[allow()]` 时需注释原因）
- ✅ API Key 通过环境变量配置（`AGENT_UNIX_LLM_API_KEY` 或官方变量）
- ✅ Base URL 必须是 HTTPS

## 开发命令

```bash
# 编译检查（不运行）
cargo check

# 运行（Anthropic 默认）
export ANTHROPIC_API_KEY=sk-ant-...
cargo run -- chat

# 自定义 Anthropic 端点（如代理/中转）
export AGENT_UNIX_LLM_PROVIDER=anthropic
export AGENT_UNIX_LLM_BASE_URL=https://your-anthropic-proxy.com
export AGENT_UNIX_LLM_MODEL=claude-sonnet-4-5
export AGENT_UNIX_LLM_API_KEY=your-key
cargo run -- chat

# OpenAI-compatible Provider（OpenRouter、vLLM、LocalAI 等）
export AGENT_UNIX_LLM_PROVIDER=openai-compatible
export AGENT_UNIX_LLM_BASE_URL=https://api.openai.com  # 或自定义端点
export AGENT_UNIX_LLM_MODEL=gpt-4o  # 或其他模型
export AGENT_UNIX_LLM_API_KEY=your-key
cargo run -- chat

# 或通过 CLI 参数配置
cargo run -- --provider openai-compatible --base-url https://api.groq.com --model llama-3.1-70b -- chat

# 单条指令模式
cargo run -- run "查看磁盘使用情况"

# Dry-Run 预览
cargo run -- run --dry-run "清理 30 天前的日志"

# Release 构建
cargo build --release
```

### 环境变量优先级

CLI 参数 > 环境变量 > 默认值：

| 变量 | CLI 参数 | 说明 |
|------|----------|------|
| `AGENT_UNIX_LLM_PROVIDER` | `--provider` | `anthropic` 或 `openai-compatible` |
| `AGENT_UNIX_LLM_BASE_URL` | `--base-url` | API 端点 URL |
| `AGENT_UNIX_LLM_MODEL` | `--model` | 模型 ID |
| `AGENT_UNIX_LLM_API_KEY` | - | API Key（优先于官方变量） |
| `ANTHROPIC_API_KEY` | - | Anthropic 官方变量（兼容） |
| `OPENAI_API_KEY` | - | OpenAI 官方变量（兼容） |

## LLM 接入规范

支持两类 LLM Provider，**都必须使用 tool_use/tool_calls 模式**：

### 1. Anthropic Provider

使用 Anthropic Messages API 原生 `tool_use` 格式：

```rust
// Anthropic 请求格式
POST {base_url}/v1/messages
Headers: x-api-key, anthropic-version
Payload: {
    "model": "claude-sonnet-4-5",
    "tools": [{ "name", "description", "input_schema" }],
    "messages": [...]
}

// Anthropic 响应格式
{
    "stop_reason": "tool_use",
    "content": [{ "type": "tool_use", "id", "name", "input" }]
}
```

### 2. OpenAI-compatible Provider

使用 OpenAI Chat Completions API `tools` 格式（适配 OpenRouter、vLLM、LocalAI 等）：

```rust
// OpenAI-compatible 请求格式
POST {base_url}/v1/chat/completions
Headers: Authorization: Bearer {api_key}
Payload: {
    "model": "gpt-4o",
    "tools": [{ "type": "function", "function": { "name", "description", "parameters" } }],
    "messages": [{ "role", "content", "tool_calls"? }]
}

// OpenAI-compatible 响应格式
{
    "choices": [{
        "message": { "content", "tool_calls": [{ "id", "function": { "name", "arguments" } }] }
    }]
}
```

### 关键约束

- ❌ 不得让 LLM 直接生成 bash 字符串后不经安全层就执行
- ❌ Claude 模型必须使用 `anthropic` provider（保持原生 tool_use 语义）
- ✅ Base URL 必须使用 HTTPS 协议
- ✅ Base URL 不能包含嵌入的凭证（如 `user:pass@host`）
- ✅ Model ID 只允许字母、数字、点、下划线、连字符

## 风险分类决策流程

```
输入 ToolCall
  → RiskClassifier::assess()
  → Critical?  → 直接拒绝 + 记录审计日志，返回错误说明
  → High?      → 展示风险详情 → 等待用户输入 "yes" → 继续 / 取消
  → Medium?    → 在 --safe 模式下需确认，--normal 模式下直接执行
  → Low/Safe   → 直接执行
```

## 工具 Schema 示例（发给 LLM）

```json
{
  "name": "system.info",
  "description": "查询系统信息：磁盘/内存/CPU/进程/用户/网络/服务状态",
  "input_schema": {
    "type": "object",
    "properties": {
      "query": { "type": "string", "enum": ["disk","memory","cpu","process","user","network","service","os"] },
      "filter": { "type": "string", "description": "可选过滤关键词" }
    },
    "required": ["query"]
  }
}
```

## 演示场景（开发时自测用）

开发时必须确保这四类场景能跑通：

```
# A: 基础
"查看磁盘使用情况"
"列出内存占用最高的 5 个进程"
"创建用户 testuser"

# B: 高危拦截
"删除 /etc/passwd"           → 必须被 CRITICAL 拒绝
"停止 SSH 服务"               → 必须触发 HIGH 确认
"rm -rf /"                   → 必须被 CRITICAL 拒绝并说明

# C: 环境感知
"帮我装一个 Web 服务器"       → 根据 OS 自动选 apt/yum

# D: 多步骤
"把 nginx 配置到 8080 并重启" → 自动分解为多步执行
```

## 文件不要动

以下文件是已完成的骨架，除非有 bug，不要随意重构：

- `src/types/` 下所有文件（类型定义是整个系统的基础）
- `src/safety/patterns.rs`（风险规则库，只能增加规则，不能删减）
- `src/safety/audit.rs`（审计日志，评委会查日志验证功能）
- `Cargo.toml`（依赖版本已锁定，不要随意升级）

## 参考资料

- 完整技术方案：`../Agent_Unix_工程蓝图.md`
- 比赛要求原文：`../AI_Hackathon_2026.pdf`
- Anthropic API 文档：https://docs.anthropic.com/en/api/messages
- Claude tool_use 文档：https://docs.anthropic.com/en/docs/build-with-claude/tool-use
- OpenAI Chat Completions API：https://platform.openai.com/docs/api-reference/chat
- OpenAI Function Calling：https://platform.openai.com/docs/guides/function-calling
