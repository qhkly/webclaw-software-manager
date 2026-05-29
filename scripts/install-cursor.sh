#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

INSTALL_DIR="/opt/cursor"

if [ -f "${INSTALL_DIR}/AppRun" ]; then
    echo "[INFO] cursor 已安装，跳过"
    exit 0
fi

ARCH=$(dpkg --print-architecture)
case "$ARCH" in
    amd64) PLATFORM="linux-x64" ;;
    arm64) PLATFORM="linux-arm64" ;;
    *) echo "[ERROR] 不支持的架构: $ARCH"; exit 1 ;;
esac

echo "[INFO] 获取 Cursor 最新版本..."
API_RESP=$(curl -fsSL \
    --connect-timeout 15 \
    --max-time 30 \
    --retry 3 --retry-delay 5 \
    "https://www.cursor.com/api/download?platform=${PLATFORM}&releaseTrack=stable" 2>/dev/null || echo "")

if [ -z "$API_RESP" ]; then
    echo "[ERROR] 无法获取 Cursor 版本信息"
    exit 1
fi

VERSION=$(echo "$API_RESP" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('version',''))" 2>/dev/null || echo "")
DOWNLOAD_URL=$(echo "$API_RESP" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('downloadUrl',''))" 2>/dev/null || echo "")

if [ -z "$VERSION" ] || [ -z "$DOWNLOAD_URL" ]; then
    echo "[ERROR] API 响应解析失败: $API_RESP"
    exit 1
fi

echo "[INFO] 安装 Cursor v${VERSION} (${ARCH})"
echo "[INFO] 下载: ${DOWNLOAD_URL}"

TMP_DIR=$(mktemp -d)

# 带重试的下载
download_ok=false
for attempt in 1 2 3; do
    if curl -fsSL \
        --connect-timeout 30 \
        --max-time 600 \
        --retry 2 --retry-delay 10 \
        -L "$DOWNLOAD_URL" -o "${TMP_DIR}/cursor.AppImage"; then
        download_ok=true
        break
    fi
    echo "[WARN] 下载失败，第 ${attempt} 次重试..."
    sleep $((attempt * 10))
done

if [ "$download_ok" != "true" ]; then
    echo "[ERROR] Cursor 下载失败（3次重试后）"
    rm -rf "$TMP_DIR"
    exit 1
fi

chmod +x "${TMP_DIR}/cursor.AppImage"

EXTRACT_DIR="${TMP_DIR}/extracted"
mkdir -p "$EXTRACT_DIR"
cd "$EXTRACT_DIR"
"${TMP_DIR}/cursor.AppImage" --appimage-extract 2>/dev/null || true

if [ -d "${EXTRACT_DIR}/squashfs-root" ]; then
    APPDIR_SRC="${EXTRACT_DIR}/squashfs-root"
elif [ -d "${EXTRACT_DIR}/AppDir" ]; then
    APPDIR_SRC="${EXTRACT_DIR}/AppDir"
else
    echo "[INFO] --appimage-extract 失败，改用 unsquashfs..."
    command -v unsquashfs >/dev/null 2>&1 || apt-get install -y squashfs-tools -qq 2>/dev/null
    SQFS_OFFSET=$(LC_ALL=C grep -oba $'\x68\x73\x71\x73' "${TMP_DIR}/cursor.AppImage" 2>/dev/null | head -1 | cut -d: -f1 || true)
    if [ -n "$SQFS_OFFSET" ]; then
        unsquashfs -o "$SQFS_OFFSET" -d "${EXTRACT_DIR}/squashfs-root" "${TMP_DIR}/cursor.AppImage" 2>&1 | tail -2 || true
    else
        unsquashfs -d "${EXTRACT_DIR}/squashfs-root" "${TMP_DIR}/cursor.AppImage" 2>&1 | tail -2 || true
    fi
    APPDIR_SRC="${EXTRACT_DIR}/squashfs-root"
fi
cd /

rm -rf "$INSTALL_DIR"
mv "$APPDIR_SRC" "$INSTALL_DIR"
chmod -R a+rX "$INSTALL_DIR"

cat > /usr/local/bin/cursor <<'WRAPPER_EOF'
#!/bin/bash
export APPDIR="/opt/cursor"
cd "/opt/cursor"
exec "/opt/cursor/AppRun" --no-sandbox "$@"
WRAPPER_EOF
chmod +x /usr/local/bin/cursor

mkdir -p /opt/on-demand-icons
ICON_SRC=$(find "$INSTALL_DIR" -name "cursor.png" 2>/dev/null | head -1)
[ -n "$ICON_SRC" ] && cp "$ICON_SRC" /opt/on-demand-icons/cursor.png || true

rm -rf "$TMP_DIR"
echo "[INFO] Cursor 安装完成"
