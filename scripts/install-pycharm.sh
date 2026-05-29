#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

INSTALL_DIR="/opt/pycharm"

if [ -f "${INSTALL_DIR}/bin/pycharm.sh" ]; then
    echo "[INFO] PyCharm Community 已安装，跳过"
    exit 0
fi

ARCH=$(dpkg --print-architecture)
case "$ARCH" in
    amd64) ARCH_SUFFIX="" ;;
    arm64) ARCH_SUFFIX="-aarch64" ;;
    *) echo "[ERROR] 不支持的架构: $ARCH"; exit 1 ;;
esac

echo "[INFO] 获取 PyCharm Community 最新版本..."
VERSION=$(curl -fsSL "https://data.services.jetbrains.com/products/releases?code=PCP&latest=true&type=release" \
    | python3 -c "import json,sys; d=json.load(sys.stdin); print(d['PCP'][0]['version'])" 2>/dev/null || echo "")
[ -z "$VERSION" ] && echo "[ERROR] 无法获取最新版本" && exit 1
echo "[INFO] 安装 PyCharm Community v${VERSION} (${ARCH})"

DOWNLOAD_URL="https://download.jetbrains.com/python/pycharm-community-${VERSION}${ARCH_SUFFIX}.tar.gz"
echo "[INFO] 下载: ${DOWNLOAD_URL}"

TMP_DIR=$(mktemp -d)
curl -fsSL --progress-bar -L "$DOWNLOAD_URL" -o "${TMP_DIR}/pycharm.tar.gz"

mkdir -p "${TMP_DIR}/extracted"
tar -xzf "${TMP_DIR}/pycharm.tar.gz" -C "${TMP_DIR}/extracted" --strip-components=1

rm -rf "$INSTALL_DIR"
mv "${TMP_DIR}/extracted" "$INSTALL_DIR"
chmod -R a+rX "$INSTALL_DIR"
chmod +x "${INSTALL_DIR}/bin/pycharm.sh"

mkdir -p /opt/on-demand-icons
ICON_SRC=$(find "$INSTALL_DIR" -name "pycharm.png" 2>/dev/null | sort -r | head -1)
[ -n "$ICON_SRC" ] && cp "$ICON_SRC" /opt/on-demand-icons/pycharm.png || true

rm -rf "$TMP_DIR"
echo "[INFO] PyCharm Community 安装完成"
