#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

if [ -f "/.dockerenv" ] || [ "${WEBCLAW_DOCKER_BUILD:-}" = "1" ]; then
    export WEBCLAW_DOCKER_BUILD=1
fi

INSTALL_DIR="/opt/ondemand-apps/obsidian"

# 检查是否已安装
if [ -f "${INSTALL_DIR}/obsidian" ]; then
    echo "[INFO] obsidian 已安装，跳过"
    exit 0
fi

ARCH=$(dpkg --print-architecture)
case "$ARCH" in
    amd64) ARCH_SUFFIX="" ;;
    arm64) ARCH_SUFFIX="-arm64" ;;
    *)
        echo "[ERROR] 不支持的架构: $ARCH"
        exit 1
        ;;
esac

echo "[INFO] 获取 Obsidian 最新版本..."
# 通过 redirect 获取最新 release tag，不消耗 GitHub API 配额
LATEST_TAG=$(curl -fsS -o /dev/null -w '%{redirect_url}' \
    "https://github.com/obsidianmd/obsidian-releases/releases/latest" 2>/dev/null \
    | sed 's|.*/tag/v\?||' | tr -d '\r' || echo "")
if [ -z "$LATEST_TAG" ]; then
    # 降级：用 API（可能 403 rate limit）
    LATEST_TAG=$(curl -fsSL -H "User-Agent: webclaw-software-manager/0.1" \
        "https://api.github.com/repos/obsidianmd/obsidian-releases/releases/latest" \
        | python3 -c "import json,sys; print(json.load(sys.stdin)['tag_name'].lstrip('v'))" 2>/dev/null || echo "")
fi

if [ -z "$LATEST_TAG" ]; then
    echo "[ERROR] 无法获取最新版本，请检查网络连接"
    exit 1
fi
echo "[INFO] 安装 Obsidian v${LATEST_TAG} (${ARCH})"

APPIMAGE_NAME="Obsidian-${LATEST_TAG}${ARCH_SUFFIX}.AppImage"
DOWNLOAD_URL="https://github.com/obsidianmd/obsidian-releases/releases/download/v${LATEST_TAG}/${APPIMAGE_NAME}"
echo "[INFO] 下载: ${DOWNLOAD_URL}"

TMP_DIR=$(mktemp -d)
curl -fsSL --progress-bar -L "$DOWNLOAD_URL" -o "${TMP_DIR}/obsidian.AppImage"

echo "[INFO] 安装 AppImage（提取模式，无需 FUSE）..."
chmod +x "${TMP_DIR}/obsidian.AppImage"

EXTRACT_DIR="${TMP_DIR}/extracted"
mkdir -p "$EXTRACT_DIR"
cd "$EXTRACT_DIR"
"${TMP_DIR}/obsidian.AppImage" --appimage-extract 2>/dev/null || true

# Docker build / QEMU 环境下 --appimage-extract 可能失败
if [ ! -d "${EXTRACT_DIR}/squashfs-root" ]; then
    echo "[INFO] --appimage-extract 失败，改用 unsquashfs 提取..."
    command -v unsquashfs >/dev/null 2>&1 || apt-get install -y squashfs-tools -qq 2>/dev/null
    SQFS_OFFSET=$(LANG=C grep -oba $'\x68\x73\x71\x73' "${TMP_DIR}/obsidian.AppImage" 2>/dev/null | head -1 | cut -d: -f1 || true)
    if [ -n "$SQFS_OFFSET" ]; then
        unsquashfs -o "$SQFS_OFFSET" -d "${EXTRACT_DIR}/squashfs-root" "${TMP_DIR}/obsidian.AppImage" 2>&1 | tail -2 || true
    else
        unsquashfs -d "${EXTRACT_DIR}/squashfs-root" "${TMP_DIR}/obsidian.AppImage" 2>&1 | tail -2 || true
    fi
fi
cd /

mkdir -p "$(dirname "$INSTALL_DIR")"
rm -rf "$INSTALL_DIR"
mv "${EXTRACT_DIR}/squashfs-root" "${INSTALL_DIR}"

# 创建启动脚本（通过 AppRun 启动，解决 Electron WebView 路径问题）
cat > /usr/local/bin/obsidian <<WRAPPER_EOF
#!/bin/bash
export APPDIR="${INSTALL_DIR}"
cd "${INSTALL_DIR}"
exec "${INSTALL_DIR}/AppRun" --no-sandbox "\$@"
WRAPPER_EOF
chmod +x /usr/local/bin/obsidian

mkdir -p /opt/on-demand-icons
ICON_SRC=$(find "${INSTALL_DIR}" -name "*.png" -path "*/icons/*" | sort -r | head -n1)
[ -n "$ICON_SRC" ] && cp "$ICON_SRC" /opt/on-demand-icons/obsidian.png || true

rm -rf "$TMP_DIR"
echo "[INFO] Obsidian 安装完成"
