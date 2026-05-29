#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

if dpkg -s wireshark 2>/dev/null | grep -q "Status: install ok installed"; then
    echo "[INFO] wireshark 已安装，跳过"
    exit 0
fi

echo "[INFO] 安装 Wireshark..."
DEBIAN_FRONTEND=noninteractive apt-get install -y wireshark

# 允许非 root 用户捕获数据包
if id ubuntu &>/dev/null; then
    usermod -aG wireshark ubuntu || true
fi
if command -v setcap &>/dev/null && [ -f /usr/bin/dumpcap ]; then
    setcap cap_net_raw,cap_net_admin=eip /usr/bin/dumpcap || true
fi

echo "[INFO] Wireshark 安装完成"
