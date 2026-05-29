#!/usr/bin/env bash
set -euo pipefail
dpkg -r dockyard 2>/dev/null || apt-get remove -y dockyard 2>/dev/null || true
rm -f /opt/on-demand-icons/dockyard.png
echo "[INFO] dockyard 已卸载"
