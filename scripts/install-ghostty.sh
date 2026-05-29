#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

if [ -f "/.dockerenv" ] || [ "${WEBCLAW_DOCKER_BUILD:-}" = "1" ]; then
    export WEBCLAW_DOCKER_BUILD=1
fi

INSTALL_DIR="/opt/ondemand-apps/ghostty/AppDir"

if [ -f "${INSTALL_DIR}/bin/ghostty" ]; then
    echo "[INFO] ghostty 已安装，跳过"
    exit 0
fi

ARCH=$(dpkg --print-architecture)
case "$ARCH" in
    amd64) ARCH_KEY="x86_64" ;;
    arm64) ARCH_KEY="aarch64" ;;
    *) echo "[ERROR] 不支持的架构: $ARCH"; exit 1 ;;
esac

echo "[INFO] 获取 Ghostty 最新版本..."
LATEST_TAG=$(curl -fsSL -o /dev/null -w '%{url_effective}' \
    "https://github.com/pkgforge-dev/ghostty-appimage/releases/latest" 2>/dev/null \
    | sed -n 's|.*/tag/v\?||p' | tr -d '\r' || echo "")
if [ -z "$LATEST_TAG" ]; then
    LATEST_TAG=$(curl -fsSL -H "User-Agent: webclaw-software-manager/0.1" \
        "https://api.github.com/repos/pkgforge-dev/ghostty-appimage/releases/latest" \
        | python3 -c "import json,sys; print(json.load(sys.stdin)['tag_name'].lstrip('v'))" 2>/dev/null || echo "")
fi
[ -z "$LATEST_TAG" ] && echo "[ERROR] 无法获取最新版本" && exit 1

echo "[INFO] 安装 Ghostty v${LATEST_TAG} (${ARCH})"

APPIMAGE_NAME="Ghostty-${LATEST_TAG}-${ARCH_KEY}.AppImage"
DOWNLOAD_URL="https://github.com/pkgforge-dev/ghostty-appimage/releases/download/v${LATEST_TAG}/${APPIMAGE_NAME}"
echo "[INFO] 下载: ${DOWNLOAD_URL}"

TMP_DIR=$(mktemp -d)
curl -fsSL --progress-bar -L "$DOWNLOAD_URL" -o "${TMP_DIR}/ghostty.AppImage"
chmod +x "${TMP_DIR}/ghostty.AppImage"

EXTRACT_DIR="${TMP_DIR}/extracted"
mkdir -p "$EXTRACT_DIR"
cd "$EXTRACT_DIR"
"${TMP_DIR}/ghostty.AppImage" --appimage-extract 2>/dev/null || true

# pkgforge AppImage 解压到 AppDir（不是 squashfs-root）
if [ -d "${EXTRACT_DIR}/AppDir" ]; then
    APPDIR_SRC="${EXTRACT_DIR}/AppDir"
elif [ -d "${EXTRACT_DIR}/squashfs-root" ]; then
    APPDIR_SRC="${EXTRACT_DIR}/squashfs-root"
else
    echo "[INFO] --appimage-extract 失败，改用 unsquashfs..."
    command -v unsquashfs >/dev/null 2>&1 || apt-get install -y squashfs-tools -qq 2>/dev/null
    SQFS_OFFSET=$(LANG=C grep -oba $'\x68\x73\x71\x73' "${TMP_DIR}/ghostty.AppImage" 2>/dev/null | head -1 | cut -d: -f1 || true)
    if [ -n "$SQFS_OFFSET" ]; then
        unsquashfs -o "$SQFS_OFFSET" -d "${EXTRACT_DIR}/squashfs-root" "${TMP_DIR}/ghostty.AppImage" 2>&1 | tail -2 || true
    else
        unsquashfs -d "${EXTRACT_DIR}/squashfs-root" "${TMP_DIR}/ghostty.AppImage" 2>&1 | tail -2 || true
    fi
    APPDIR_SRC="${EXTRACT_DIR}/squashfs-root"
fi
cd /

mkdir -p "$(dirname "$INSTALL_DIR")"
rm -rf "$INSTALL_DIR"
mv "$APPDIR_SRC" "$INSTALL_DIR"
chmod -R a+rX "$INSTALL_DIR"

cat > /usr/local/bin/ghostty <<'WRAPPER_EOF'
#!/bin/bash
export APPDIR="/opt/ondemand-apps/ghostty/AppDir"
cd "/opt/ondemand-apps/ghostty/AppDir"
exec "/opt/ondemand-apps/ghostty/AppDir/AppRun" --no-sandbox "$@"
WRAPPER_EOF
chmod +x /usr/local/bin/ghostty

mkdir -p /opt/on-demand-icons
ICON_SRC=$(find "$INSTALL_DIR" -name "ghostty.png" 2>/dev/null | head -1)
[ -z "$ICON_SRC" ] && ICON_SRC=$(find "$INSTALL_DIR" -name "*.png" 2>/dev/null | head -1)
[ -n "$ICON_SRC" ] && cp "$ICON_SRC" /opt/on-demand-icons/ghostty.png || true

rm -rf "$TMP_DIR"
echo "[INFO] Ghostty 安装完成"
