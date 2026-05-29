#!/usr/bin/env bash
set -euo pipefail

DISABLE_ZENITY="${DISABLE_ZENITY:-0}"

if [ "$DISABLE_ZENITY" != "1" ]; then
    zenity --question \
      --title="卸载 Obsidian" \
      --text="<b>确定要卸载 Obsidian 吗？</b>" \
      --ok-label="卸载" --cancel-label="取消" \
      --width=400 --no-wrap || exit 0
fi

echo "[INFO] 卸载 Obsidian..."

rm -f /usr/local/bin/obsidian
rm -rf /opt/ondemand-apps/obsidian
rm -f /opt/on-demand-icons/obsidian.png

if [ -x /usr/local/bin/update-desktop-icons ]; then
    sudo -u ubuntu /usr/local/bin/update-desktop-icons 2>/dev/null || true
fi

echo "[INFO] Obsidian 卸载完成"
