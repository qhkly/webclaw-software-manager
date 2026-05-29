#!/usr/bin/env bash
set -euo pipefail
dpkg -r trae 2>/dev/null || apt-get remove -y trae 2>/dev/null || true
rm -f /opt/on-demand-icons/trae.png
echo "[INFO] trae 已卸载"
