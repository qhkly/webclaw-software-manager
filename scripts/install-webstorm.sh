#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

INSTALL_DIR="/opt/webstorm"

if [ -f "${INSTALL_DIR}/bin/webstorm.sh" ]; then
    echo "[INFO] WebStorm 已安装，跳过"
    exit 0
fi

ARCH=$(dpkg --print-architecture)
case "$ARCH" in
    amd64) ;;
    arm64) ;;
    *) echo "[ERROR] 不支持的架构: $ARCH"; exit 1 ;;
esac

echo "[INFO] 获取 WebStorm 最新版本..."
API_RESPONSE=$(curl -fsSL "https://data.services.jetbrains.com/products/releases?code=WS&latest=true&type=release")
VERSION=$(echo "$API_RESPONSE" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d['WS'][0]['version'])" 2>/dev/null || echo "")
[ -z "$VERSION" ] && echo "[ERROR] 无法获取最新版本" && exit 1
echo "[INFO] 安装 WebStorm v${VERSION} (${ARCH})"

if [ "$ARCH" = "arm64" ]; then
    DOWNLOAD_URL=$(echo "$API_RESPONSE" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d['WS'][0]['downloads']['linuxARM64']['link'])" 2>/dev/null || echo "")
else
    DOWNLOAD_URL=$(echo "$API_RESPONSE" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d['WS'][0]['downloads']['linux']['link'])" 2>/dev/null || echo "")
fi
[ -z "$DOWNLOAD_URL" ] && echo "[ERROR] 无法获取下载链接" && exit 1
echo "[INFO] 下载: ${DOWNLOAD_URL}"

TMP_DIR=$(mktemp -d)
curl -fsSL --progress-bar -L "$DOWNLOAD_URL" -o "${TMP_DIR}/webstorm.tar.gz"

mkdir -p "${TMP_DIR}/extracted"
tar -xzf "${TMP_DIR}/webstorm.tar.gz" -C "${TMP_DIR}/extracted" --strip-components=1

rm -rf "$INSTALL_DIR"
mv "${TMP_DIR}/extracted" "$INSTALL_DIR"
chmod -R a+rX "$INSTALL_DIR"
chmod +x "${INSTALL_DIR}/bin/webstorm.sh"

mkdir -p /opt/on-demand-icons
ICON_SRC=$(find "$INSTALL_DIR" -name "webstorm.png" 2>/dev/null | sort -r | head -1)
[ -n "$ICON_SRC" ] && cp "$ICON_SRC" /opt/on-demand-icons/webstorm.png || true

rm -rf "$TMP_DIR"
echo "[INFO] WebStorm 安装完成"
