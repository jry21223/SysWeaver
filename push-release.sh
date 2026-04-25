#!/usr/bin/env bash
# push-release.sh — 一键打 tag 并推送，触发 GitHub Actions 多平台编译
# 用法：
#   ./push-release.sh            # 使用 Cargo.toml 当前版本打 tag
#   ./push-release.sh 0.8.0      # 指定新版本（同时更新 Cargo.toml）
#   ./push-release.sh --dispatch # 仅触发 workflow_dispatch，不打 tag

set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$REPO_DIR"

# ── 颜色 ────────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; RESET='\033[0m'

info()  { echo -e "${CYAN}▶ $*${RESET}"; }
ok()    { echo -e "${GREEN}✓ $*${RESET}"; }
warn()  { echo -e "${YELLOW}⚠ $*${RESET}"; }
die()   { echo -e "${RED}✗ $*${RESET}" >&2; exit 1; }

# ── 检查依赖 ─────────────────────────────────────────────────────────────────
command -v git  >/dev/null 2>&1 || die "git 未安装"
command -v gh   >/dev/null 2>&1 || warn "gh CLI 未安装，跳过浏览器预览步骤"

# ── 读取当前版本 ──────────────────────────────────────────────────────────────
current_version=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

# ── 仅触发 workflow_dispatch 模式 ─────────────────────────────────────────────
if [[ "${1:-}" == "--dispatch" ]]; then
    command -v gh >/dev/null 2>&1 || die "gh CLI 未安装，无法触发 workflow_dispatch"
    info "触发 workflow_dispatch（不打 tag）…"
    gh workflow run release.yml
    ok "已触发 workflow，查看进度："
    echo -e "  ${BOLD}https://github.com/jry21223/agent-unix/actions${RESET}"
    exit 0
fi

# ── 确定目标版本 ───────────────────────────────────────────────────────────────
if [[ -n "${1:-}" ]]; then
    new_version="$1"
    # 校验格式
    [[ "$new_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "版本格式错误，应为 x.y.z，收到: $new_version"
else
    new_version="$current_version"
fi

TAG="v${new_version}"

echo -e "${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo -e "  ${BOLD}sysweaver 一键发布脚本${RESET}"
echo -e "  当前版本: ${YELLOW}v${current_version}${RESET}  →  目标 tag: ${GREEN}${TAG}${RESET}"
echo -e "${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"

# ── 检查 tag 是否已存在 ────────────────────────────────────────────────────────
if git tag --list | grep -q "^${TAG}$"; then
    die "tag ${TAG} 已存在，请先 git tag -d ${TAG} 删除或指定新版本号"
fi

# ── 更新 Cargo.toml（如果版本变更）────────────────────────────────────────────
if [[ "$new_version" != "$current_version" ]]; then
    info "更新 Cargo.toml: $current_version → $new_version"
    sed -i.bak "s/^version = \"${current_version}\"/version = \"${new_version}\"/" Cargo.toml
    rm -f Cargo.toml.bak
    ok "Cargo.toml 已更新"
fi

# ── 暂存并提交未提交的变更 ────────────────────────────────────────────────────
if ! git diff --quiet || ! git diff --staged --quiet; then
    info "检测到未提交变更，自动暂存并提交…"
    git add -A
    git commit -m "chore: release ${TAG}"
    ok "变更已提交"
else
    info "工作区干净，无需提交"
fi

# ── 打 tag ────────────────────────────────────────────────────────────────────
info "创建 tag ${TAG}…"
git tag -a "${TAG}" -m "Release ${TAG}"
ok "tag 已创建"

# ── 推送（分支 + tag）──────────────────────────────────────────────────────────
current_branch=$(git rev-parse --abbrev-ref HEAD)
info "推送分支 ${current_branch} 到 origin…"
git push origin "${current_branch}"
ok "分支推送完成"

info "推送 tag ${TAG} 到 origin（触发 GitHub Actions）…"
git push origin "${TAG}"
ok "tag 推送完成，workflow 已触发！"

# ── 打印结果 ──────────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo -e "  ${GREEN}✓ 发布流程已启动${RESET}"
echo -e "  Tag: ${BOLD}${TAG}${RESET}"
echo -e "  构建进度: ${CYAN}https://github.com/jry21223/agent-unix/actions${RESET}"
echo -e "  Release:  ${CYAN}https://github.com/jry21223/agent-unix/releases/tag/${TAG}${RESET}"
echo -e "${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"

# ── 用 gh 打开 Actions 页（可选）────────────────────────────────────────────
if command -v gh >/dev/null 2>&1; then
    read -r -p "$(echo -e "${YELLOW}在浏览器打开 Actions 页面？[y/N] ${RESET}")" open_browser
    if [[ "${open_browser}" =~ ^[Yy]$ ]]; then
        gh browse --repo jry21223/agent-unix actions
    fi
fi
