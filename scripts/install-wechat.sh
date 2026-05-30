#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

if [ -f "/.dockerenv" ] || [ "${WEBCLAW_DOCKER_BUILD:-}" = "1" ]; then
    export WEBCLAW_DOCKER_BUILD=1
fi

# 检查是否已安装
if dpkg -s wechat 2>/dev/null | grep -q "Status: install ok installed"; then
    echo "[INFO] wechat 已安装，跳过"
    exit 0
fi

ARCH=$(dpkg --print-architecture)
case "$ARCH" in
    amd64) ARCH_SUFFIX="x86_64" ;;
    arm64) ARCH_SUFFIX="arm64" ;;
    *)
        echo "[ERROR] 不支持的架构: $ARCH"
        exit 1
        ;;
esac

DOWNLOAD_URL="https://dldir1v6.qq.com/weixin/Universal/Linux/WeChatLinux_${ARCH_SUFFIX}.deb"
echo "[INFO] 下载微信 (${ARCH})..."
echo "[INFO] URL: ${DOWNLOAD_URL}"

TMP_DIR=$(mktemp -d)
curl -fsSL --progress-bar "$DOWNLOAD_URL" -o "${TMP_DIR}/wechat.deb"

echo "[INFO] 安装 deb 包..."
sudo dpkg -i "${TMP_DIR}/wechat.deb" || sudo apt-get install -fy

rm -rf "$TMP_DIR"

mkdir -p /opt/on-demand-icons
ICON_SRC=$(find /usr/share/icons -name "wechat.png" 2>/dev/null | sort -r | head -n1)
[ -n "$ICON_SRC" ] && cp "$ICON_SRC" /opt/on-demand-icons/wechat.png || true

echo "[INFO] 微信安装完成"
