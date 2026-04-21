#!/bin/bash
# Agent Unix 演示脚本 - AI Hackathon 2026

echo "========================================"
echo "  Agent Unix 演示"
echo "========================================"
echo

# 检查 API Key
if [ -z "$ANTHROPIC_API_KEY" ] && [ -z "$AGENT_UNIX_LLM_API_KEY" ]; then
    echo "⚠️  请先设置 API Key:"
    echo "   export ANTHROPIC_API_KEY=sk-ant-xxx"
    echo "   或"
    echo "   export AGENT_UNIX_LLM_API_KEY=xxx"
    exit 1
fi

BINARY="./target/release/agent-unix"

# 编译检查
if [ ! -f "$BINARY" ]; then
    echo "⚠️  请先编译: cargo build --release"
    exit 1
fi

echo "📋 演示 1: 查看 Playbook 模板（无需 API）"
echo "----------------------------------------"
$BINARY playbooks
echo
echo "按回车继续..."
read

echo "📖 演示 2: 反向解释模式（无需 API）"
echo "----------------------------------------"
$BINARY explain
echo
echo "按回车继续..."
read

echo "🔍 演示 3: Watchdog 监控（无需 API）"
echo "----------------------------------------"
echo "启动 10 秒监控演示..."
$BINARY watch --duration 10
echo
echo "按回车继续..."
read

echo "🤖 演示 4: Agent 对话（需要 API）"
echo "----------------------------------------"
echo "启动交互式对话..."
echo "建议输入以下指令测试:"
echo "  1. 查看磁盘使用情况"
echo "  2. 列出内存占用最高的 5 个进程"
echo "  3. 删除 /etc/passwd（会被 CRITICAL 拦截）"
echo "  4. history（查看操作历史）"
echo "  5. exit（退出）"
echo
$BINARY chat

echo
echo "========================================"
echo "  演示完成！"
echo "========================================"