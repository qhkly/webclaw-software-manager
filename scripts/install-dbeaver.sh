#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

# 检查是否已安装
if dpkg -s dbeaver-ce 2>/dev/null | grep -q "Status: install ok installed"; then
    echo "[INFO] dbeaver-ce 已安装，跳过"
    exit 0
fi

ARCH=$(dpkg --print-architecture)
case "$ARCH" in
    amd64) ARCH_KEY="x86_64" ;;
    arm64) ARCH_KEY="aarch64" ;;
    *)
        echo "[ERROR] 不支持的架构: $ARCH"
        exit 1
        ;;
esac

echo "[INFO] 获取 DBeaver 最新版本..."
# 通过 redirect 获取最新 release tag，不消耗 GitHub API 配额
LATEST_TAG=$(curl -fsS -o /dev/null -w '%{redirect_url}' \
    "https://github.com/dbeaver/dbeaver/releases/latest" 2>/dev/null \
    | sed 's|.*/tag/v\?||' | tr -d '\r' || echo "")
if [ -z "$LATEST_TAG" ]; then
    # 降级：用 API（可能 403 rate limit）
    LATEST_TAG=$(curl -fsSL -H "User-Agent: webclaw-software-manager/0.1" \
        "https://api.github.com/repos/dbeaver/dbeaver/releases/latest" \
        | python3 -c "import json,sys; print(json.load(sys.stdin)['tag_name'].lstrip('v'))" 2>/dev/null || echo "")
fi

if [ -z "$LATEST_TAG" ]; then
    echo "[ERROR] 无法获取最新版本，请检查网络连接"
    exit 1
fi
echo "[INFO] 安装 DBeaver Community v${LATEST_TAG} (${ARCH})"

DOWNLOAD_URL="https://github.com/dbeaver/dbeaver/releases/download/${LATEST_TAG}/dbeaver-ce-${LATEST_TAG}-linux-${ARCH_KEY}.deb"
echo "[INFO] 下载: ${DOWNLOAD_URL}"

TMP_DIR=$(mktemp -d)
curl -fsSL --progress-bar -L "$DOWNLOAD_URL" -o "${TMP_DIR}/dbeaver.deb"

echo "[INFO] 安装 deb 包..."
dpkg -i "${TMP_DIR}/dbeaver.deb" || apt-get install -fy

rm -rf "$TMP_DIR"

mkdir -p /opt/on-demand-icons
ICON_SRC=$(find /usr/share/icons -name "dbeaver.png" 2>/dev/null | sort -r | head -n1)
[ -n "$ICON_SRC" ] && cp "$ICON_SRC" /opt/on-demand-icons/dbeaver.png || true

echo "[INFO] DBeaver Community 安装完成"
