#!/usr/bin/env bash
set -euo pipefail

export DISPLAY="${DISPLAY:-:1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true

INSTALL_DIR="/opt/eclipse"

if [ -f "${INSTALL_DIR}/eclipse" ]; then
    echo "[INFO] Eclipse IDE 已安装，跳过"
    exit 0
fi

ARCH=$(dpkg --print-architecture)
case "$ARCH" in
    amd64) ARCH_KEY="x86_64" ;;
    arm64) ARCH_KEY="aarch64" ;;
    *) echo "[ERROR] 不支持的架构: $ARCH"; exit 1 ;;
esac

# Eclipse 使用固定 release 路径，定期更新
ECLIPSE_RELEASE="2026-03"
ECLIPSE_MILESTONE="R"
DOWNLOAD_URL="https://www.eclipse.org/downloads/download.php?file=/technology/epp/downloads/release/${ECLIPSE_RELEASE}/${ECLIPSE_MILESTONE}/eclipse-committers-${ECLIPSE_RELEASE}-${ECLIPSE_MILESTONE}-linux-gtk-${ARCH_KEY}.tar.gz&r=1"
echo "[INFO] 下载 Eclipse IDE ${ECLIPSE_RELEASE}-${ECLIPSE_MILESTONE}: ${DOWNLOAD_URL}"

TMP_DIR=$(mktemp -d)
curl -fsSL --progress-bar -L "$DOWNLOAD_URL" -o "${TMP_DIR}/eclipse.tar.gz"

mkdir -p "${TMP_DIR}/extracted"
tar -xzf "${TMP_DIR}/eclipse.tar.gz" -C "${TMP_DIR}/extracted" --strip-components=1

rm -rf "$INSTALL_DIR"
mv "${TMP_DIR}/extracted" "$INSTALL_DIR"
chmod -R a+rX "$INSTALL_DIR"
chmod +x "${INSTALL_DIR}/eclipse"

mkdir -p /opt/on-demand-icons
ICON_SRC=$(find "$INSTALL_DIR" -name "eclipse.png" 2>/dev/null | sort -r | head -1)
[ -n "$ICON_SRC" ] && cp "$ICON_SRC" /opt/on-demand-icons/eclipse.png || true

rm -rf "$TMP_DIR"
echo "[INFO] Eclipse IDE 安装完成"
