#!/usr/bin/env bash
set -euo pipefail

DISABLE_ZENITY="${DISABLE_ZENITY:-0}"

if [ "$DISABLE_ZENITY" != "1" ]; then
    zenity --question \
      --title="卸载 DBeaver" \
      --text="<b>确定要卸载 DBeaver Community 吗？</b>" \
      --ok-label="卸载" --cancel-label="取消" \
      --width=400 --no-wrap || exit 0
fi

echo "[INFO] 卸载 DBeaver Community..."

if dpkg -s dbeaver-ce 2>/dev/null | grep -q "Status: install ok installed"; then
    dpkg -r dbeaver-ce || true
fi

rm -f /opt/on-demand-icons/dbeaver.png

if [ -x /usr/local/bin/update-desktop-icons ]; then
    sudo -u ubuntu /usr/local/bin/update-desktop-icons 2>/dev/null || true
fi

echo "[INFO] DBeaver Community 卸载完成"
