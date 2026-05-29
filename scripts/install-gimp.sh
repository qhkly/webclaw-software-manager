#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

if dpkg -s gimp 2>/dev/null | grep -q "Status: install ok installed"; then
    echo "[INFO] gimp 已安装，跳过"
    exit 0
fi

echo "[INFO] 安装 GIMP..."
apt-get install -y gimp

echo "[INFO] GIMP 安装完成"
