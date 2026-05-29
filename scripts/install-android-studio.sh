#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

INSTALL_DIR="/opt/android-studio"

if [ -f "${INSTALL_DIR}/bin/studio.sh" ]; then
    echo "[INFO] Android Studio 已安装，跳过"
    exit 0
fi

ARCH=$(dpkg --print-architecture)
if [ "$ARCH" != "amd64" ]; then
    echo "[ERROR] Android Studio 仅支持 amd64 架构，当前: $ARCH"
    exit 1
fi

echo "[INFO] 获取 Android Studio 最新版本..."
# 使用 Google 更新 XML 获取最新稳定版本
VERSION=$(curl -fsSL "https://dl.google.com/android/studio/patches/updates.xml" 2>/dev/null \
    | python3 -c "
import xml.etree.ElementTree as ET, sys
try:
    root = ET.parse(sys.stdin).getroot()
    for channel in root.iter('channel'):
        if channel.get('status') == 'release':
            for build in channel.iter('build'):
                v = build.get('version')
                if v: print(v); break
            break
except: pass
" 2>/dev/null || echo "")

if [ -z "$VERSION" ]; then
    echo "[ERROR] 无法获取最新版本，请检查网络"
    exit 1
fi
echo "[INFO] 安装 Android Studio v${VERSION}"

DOWNLOAD_URL="https://redirector.gvt1.com/edgedl/android/studio/ide-zips/${VERSION}/android-studio-${VERSION}-linux.tar.gz"
echo "[INFO] 下载: ${DOWNLOAD_URL}"

TMP_DIR=$(mktemp -d)
curl -fsSL --progress-bar -L "$DOWNLOAD_URL" -o "${TMP_DIR}/android-studio.tar.gz"

mkdir -p "${TMP_DIR}/extracted"
tar -xzf "${TMP_DIR}/android-studio.tar.gz" -C "${TMP_DIR}/extracted" --strip-components=1

rm -rf "$INSTALL_DIR"
mv "${TMP_DIR}/extracted" "$INSTALL_DIR"
chmod -R a+rX "$INSTALL_DIR"
chmod +x "${INSTALL_DIR}/bin/studio.sh"

mkdir -p /opt/on-demand-icons
ICON_SRC=$(find "$INSTALL_DIR" -name "studio.png" 2>/dev/null | sort -r | head -1)
[ -n "$ICON_SRC" ] && cp "$ICON_SRC" /opt/on-demand-icons/android-studio.png || true

rm -rf "$TMP_DIR"
echo "[INFO] Android Studio 安装完成"
