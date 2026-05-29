#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

INSTALL_DIR="/opt/webclaw-launcher/AppDir"

if [ -f "${INSTALL_DIR}/AppRun" ]; then
    echo "[INFO] webclaw-launcher 已安装，跳过"
    exit 0
fi

ARCH=$(dpkg --print-architecture)
case "$ARCH" in
    amd64) ARCH_KEY="x64" ;;
    arm64) ARCH_KEY="arm64" ;;
    *) echo "[ERROR] 不支持的架构: $ARCH"; exit 1 ;;
esac

echo "[INFO] 获取 WebClaw Launcher 最新版本..."
VERSION=$(curl -fsSL "https://webclaw.qhkly.com/api/download/latest" \
    | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('version','').lstrip('v'))" 2>/dev/null || echo "")
[ -z "$VERSION" ] && echo "[ERROR] 无法获取最新版本" && exit 1
echo "[INFO] 安装 WebClaw Launcher v${VERSION}"

DOWNLOAD_URL="https://launcher.qhkly.com/launcher/versions/v${VERSION}/webclaw-launcher-linux-${ARCH_KEY}.zip"
echo "[INFO] 下载: ${DOWNLOAD_URL}"

TMP_DIR=$(mktemp -d)
curl -fsSL --progress-bar -L "$DOWNLOAD_URL" -o "${TMP_DIR}/webclaw-launcher.zip"

command -v unzip >/dev/null 2>&1 || apt-get install -y unzip -qq 2>/dev/null
mkdir -p "${TMP_DIR}/extracted"
unzip -q "${TMP_DIR}/webclaw-launcher.zip" -d "${TMP_DIR}/extracted"

# 找到 AppRun 所在目录
APPDIR_SRC=$(find "${TMP_DIR}/extracted" -name "AppRun" -type f 2>/dev/null | head -1 | xargs -I{} dirname {} || echo "")
[ -z "$APPDIR_SRC" ] && APPDIR_SRC="${TMP_DIR}/extracted"

mkdir -p "$(dirname "$INSTALL_DIR")"
rm -rf "$INSTALL_DIR"
mv "$APPDIR_SRC" "$INSTALL_DIR"
chmod -R a+rX "$INSTALL_DIR"

cat > /usr/local/bin/webclaw-launcher <<'WRAPPER_EOF'
#!/bin/bash
export APPDIR="/opt/webclaw-launcher/AppDir"
cd "/opt/webclaw-launcher/AppDir"
exec "/opt/webclaw-launcher/AppDir/AppRun" --no-sandbox "$@"
WRAPPER_EOF
chmod +x /usr/local/bin/webclaw-launcher

rm -rf "$TMP_DIR"
echo "[INFO] WebClaw Launcher 安装完成"
