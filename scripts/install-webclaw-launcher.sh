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
VERSION=$(curl -fsSL \
    --connect-timeout 15 \
    --max-time 30 \
    --retry 3 --retry-delay 5 \
    "https://webclaw.qhkly.com/api/download/latest" \
    | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('version','').lstrip('v'))" 2>/dev/null || echo "")
[ -z "$VERSION" ] && echo "[ERROR] 无法获取最新版本" && exit 1
echo "[INFO] 安装 WebClaw Launcher v${VERSION}"

DOWNLOAD_URL="https://launcher.qhkly.com/launcher/versions/v${VERSION}/webclaw-launcher-linux-${ARCH_KEY}.zip"
echo "[INFO] 下载: ${DOWNLOAD_URL}"

TMP_DIR=$(mktemp -d)

# 带重试的下载
download_ok=false
for attempt in 1 2 3; do
    if curl -fsSL \
        --connect-timeout 30 \
        --max-time 600 \
        --retry 2 --retry-delay 10 \
        -L "$DOWNLOAD_URL" -o "${TMP_DIR}/webclaw-launcher.zip"; then
        download_ok=true
        break
    fi
    echo "[WARN] 下载失败，第 ${attempt} 次重试..."
    sleep $((attempt * 10))
done
if [ "$download_ok" != "true" ]; then
    echo "[ERROR] webclaw-launcher 下载失败（3次重试后）"
    rm -rf "$TMP_DIR"; exit 1
fi

command -v unzip >/dev/null 2>&1 || apt-get install -y unzip -qq 2>/dev/null || true
EXTRACT_TMP="${TMP_DIR}/extracted"
mkdir -p "$EXTRACT_TMP"
unzip -q "${TMP_DIR}/webclaw-launcher.zip" -d "$EXTRACT_TMP"

# zip 内含 AppImage → 先解压 AppImage
APPIMAGE_FILE=$(find "$EXTRACT_TMP" -maxdepth 2 -type f -name "*.AppImage" | head -1)
if [ -n "$APPIMAGE_FILE" ]; then
    echo "[INFO] 解压 AppImage: $(basename "$APPIMAGE_FILE")"
    chmod +x "$APPIMAGE_FILE"
    APPIMG_EXTRACT="${TMP_DIR}/appimg-extracted"
    mkdir -p "$APPIMG_EXTRACT"
    cd "$APPIMG_EXTRACT"
    "$APPIMAGE_FILE" --appimage-extract 2>/dev/null || true
    cd /

    if [ -d "${APPIMG_EXTRACT}/squashfs-root" ]; then
        APPDIR_SRC="${APPIMG_EXTRACT}/squashfs-root"
    elif [ -d "${APPIMG_EXTRACT}/AppDir" ]; then
        APPDIR_SRC="${APPIMG_EXTRACT}/AppDir"
    else
        # 尝试 unsquashfs
        command -v unsquashfs >/dev/null 2>&1 || apt-get install -y squashfs-tools -qq 2>/dev/null || true
        SQFS_OFFSET=$(LC_ALL=C grep -oba $'\x68\x73\x71\x73' "$APPIMAGE_FILE" 2>/dev/null | head -1 | cut -d: -f1 || echo "")
        mkdir -p "${APPIMG_EXTRACT}/squashfs-root"
        if [ -n "$SQFS_OFFSET" ]; then
            unsquashfs -o "$SQFS_OFFSET" -d "${APPIMG_EXTRACT}/squashfs-root" "$APPIMAGE_FILE" >/dev/null 2>&1 || true
        else
            unsquashfs -d "${APPIMG_EXTRACT}/squashfs-root" "$APPIMAGE_FILE" >/dev/null 2>&1 || true
        fi
        APPDIR_SRC="${APPIMG_EXTRACT}/squashfs-root"
    fi
else
    # zip 直接含 AppDir 结构
    APPDIR_SRC=$(find "$EXTRACT_TMP" -name "AppRun" -type f 2>/dev/null | head -1 | xargs -r -I{} dirname {} || echo "")
    [ -z "$APPDIR_SRC" ] && APPDIR_SRC="$EXTRACT_TMP"
fi

mkdir -p "$(dirname "$INSTALL_DIR")"
rm -rf "$INSTALL_DIR"
cp -a "$APPDIR_SRC" "$INSTALL_DIR"
chmod -R a+rX "$INSTALL_DIR"

cat > /usr/local/bin/webclaw-launcher <<'WRAPPER_EOF'
#!/bin/bash
export APPDIR="/opt/webclaw-launcher/AppDir"
cd "/opt/webclaw-launcher/AppDir"
exec "/opt/webclaw-launcher/AppDir/AppRun" --no-sandbox "$@"
WRAPPER_EOF
chmod +x /usr/local/bin/webclaw-launcher

rm -rf "$TMP_DIR"

if [ ! -f "${INSTALL_DIR}/AppRun" ]; then
    echo "[ERROR] 安装后未找到 ${INSTALL_DIR}/AppRun"
    ls "${INSTALL_DIR}/" 2>/dev/null | head -10 || true
    exit 1
fi

echo "[INFO] WebClaw Launcher 安装完成"
