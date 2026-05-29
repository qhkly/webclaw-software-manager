#!/usr/bin/env bash
set -euo pipefail
dpkg -r opencode 2>/dev/null || apt-get remove -y opencode 2>/dev/null || true
rm -f /usr/local/bin/opencode
echo "[INFO] opencode 已卸载"
