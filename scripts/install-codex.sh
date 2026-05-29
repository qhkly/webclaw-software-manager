#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

if command -v codex &>/dev/null; then
    echo "[INFO] codex 已安装，跳过"
    exit 0
fi

echo "[INFO] 安装 Codex CLI..."
npm install -g --fetch-retries=5 --fetch-retry-mintimeout=20000 --fetch-retry-maxtimeout=120000 --fetch-timeout=300000 @openai/codex

echo "[INFO] Codex CLI 安装完成"
