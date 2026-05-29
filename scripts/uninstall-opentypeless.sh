#!/usr/bin/env bash
set -euo pipefail
dpkg -r opentypeless 2>/dev/null || apt-get remove -y opentypeless 2>/dev/null || true
rm -f /opt/on-demand-icons/opentypeless.png
echo "[INFO] opentypeless 已卸载"
