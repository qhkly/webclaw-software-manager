#!/usr/bin/env bash
set -euo pipefail
dpkg -r cc-switch 2>/dev/null || apt-get remove -y cc-switch 2>/dev/null || true
rm -f /opt/on-demand-icons/cc-switch.png
echo "[INFO] cc-switch 已卸载"
