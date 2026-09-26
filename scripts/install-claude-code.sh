#!/usr/bin/env bash
# 固定 sudo 白名单入口：只安装/升级 @anthropic-ai/claude-code，不读取任何参数。
set -euo pipefail

USER_NODE_RUN=/usr/local/bin/webclaw-user-node-run

if [ -x "$USER_NODE_RUN" ]; then
    # 新容器：装进 ubuntu 的 NVM 用户环境
    run_node() { "$USER_NODE_RUN" "$@"; }
else
    # 旧容器（无 runner）：回退到 system Node
    run_node() { "$@"; }
fi

CURRENT_VERSION="$(run_node claude --version 2>/dev/null | head -1 || true)"
if [ -n "$CURRENT_VERSION" ]; then
    echo "[INFO] 升级 Claude Code（当前: ${CURRENT_VERSION}）..."
else
    echo "[INFO] 安装 Claude Code..."
fi

run_node npm install -g --fetch-retries=5 --fetch-retry-mintimeout=20000 --fetch-retry-maxtimeout=120000 --fetch-timeout=300000 @anthropic-ai/claude-code@latest

NEW_VERSION="$(run_node claude --version 2>/dev/null | head -1 || true)"
echo "[INFO] Claude Code 已更新到 ${NEW_VERSION:-latest}"
