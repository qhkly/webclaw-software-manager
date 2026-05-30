#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

if dpkg -s trae 2>/dev/null | grep -q "Status: install ok installed"; then
    echo "[INFO] trae 已安装，跳过"
    exit 0
fi

ARCH=$(dpkg --print-architecture)
case "$ARCH" in
    amd64) ARCH_KEY="amd64" ;;
    arm64) ARCH_KEY="arm64" ;;
    *) echo "[ERROR] 不支持的架构: $ARCH"; exit 1 ;;
esac

# Trae 暂无公开版本 API，使用最新已知版本
TRAE_VERSION="2.3.21083"
DOWNLOAD_URL="https://lf-cdn.trae.ai/obj/trae-ai-us/pkg/app/releases/stable/${TRAE_VERSION}/linux/Trae-linux-${ARCH_KEY}.deb"
echo "[INFO] 下载 Trae ${TRAE_VERSION}: ${DOWNLOAD_URL}"

TMP_DIR=$(mktemp -d)
curl -fsSL --progress-bar -L "$DOWNLOAD_URL" -o "${TMP_DIR}/trae.deb"
sudo dpkg -i "${TMP_DIR}/trae.deb" || sudo apt-get install -fy
rm -rf "$TMP_DIR"

mkdir -p /opt/on-demand-icons
ICON_SRC=$(find /usr/share/icons /usr/share/pixmaps -name "trae.png" 2>/dev/null | head -1)
[ -n "$ICON_SRC" ] && cp "$ICON_SRC" /opt/on-demand-icons/trae.png || true

echo "[INFO] Trae 安装完成"
