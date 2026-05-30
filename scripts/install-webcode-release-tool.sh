#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

DEB_PKG="release-tool"
REPO="qhkly/webcode-release-tool"

if [ "${FORCE_UPGRADE:-0}" != "1" ] && \
   dpkg -s "$DEB_PKG" 2>/dev/null | grep -q "Status: install ok installed"; then
    echo "[INFO] $DEB_PKG 已安装，跳过"
    exit 0
fi

ARCH=$(dpkg --print-architecture)

echo "[INFO] 获取 Webcode Release Tool 最新版本..."
TAG=$(curl -fsSL -o /dev/null -w '%{url_effective}' \
    "https://github.com/${REPO}/releases/latest" 2>/dev/null \
    | sed 's|.*/tag/v\?||' | tr -d '\r' || echo "")
[ -z "$TAG" ] && echo "[ERROR] 无法获取版本" && exit 1

echo "[INFO] 查询 ${REPO} v${TAG} 的 Linux deb 资源..."
URL=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/tags/v${TAG}" 2>/dev/null \
    | ARCH="$ARCH" python3 -c "
import json, sys, os
data = json.load(sys.stdin)
arch = os.environ.get('ARCH', 'amd64')
for a in data.get('assets', []):
    name = a['name']
    if name.endswith(f'_{arch}.deb'):
        print(a['browser_download_url'])
        break
")

if [ -z "$URL" ]; then
    echo "[ERROR] 未找到 ${ARCH} 架构的 .deb 文件"
    exit 1
fi
echo "[INFO] 下载: ${URL}"

TMP=$(mktemp -d)
download_ok=false
for attempt in 1 2 3; do
    if curl -fsSL --connect-timeout 30 --max-time 300 -L "$URL" -o "${TMP}/app.deb"; then
        download_ok=true; break
    fi
    echo "[WARN] 下载失败，第 ${attempt} 次重试..."
    sleep $((attempt * 5))
done
if [ "$download_ok" != "true" ]; then
    echo "[ERROR] Webcode Release Tool 下载失败（3次重试后）"
    rm -rf "$TMP"; exit 1
fi

echo "[INFO] 安装 deb 包..."
sudo dpkg -i "${TMP}/app.deb" || sudo apt-get install -fy
rm -rf "$TMP"
echo "[INFO] Webcode Release Tool 安装完成"
