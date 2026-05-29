#!/usr/bin/env bash
set -euo pipefail

# 读取预装清单并依次安装每个 app
# 用法：WEBCLAW_DOCKER_BUILD=1 bash preinstall.sh
# 环境变量：
#   PREINSTALL_APPS_JSON  预装清单路径，默认 /opt/preinstall-apps.json
#   INSTALL_SCRIPTS_DIR   安装脚本目录，默认 /opt/install-scripts

PREINSTALL_JSON="${PREINSTALL_APPS_JSON:-/opt/preinstall-apps.json}"
SCRIPTS_DIR="${INSTALL_SCRIPTS_DIR:-/opt/install-scripts}"

if [ ! -f "$PREINSTALL_JSON" ]; then
    echo "[WARN] 预装清单不存在: $PREINSTALL_JSON，跳过预装"
    exit 0
fi

echo "[INFO] 读取预装清单: $PREINSTALL_JSON"
APPS=$(python3 -c "
import json, sys
data = json.load(open('$PREINSTALL_JSON'))
for app in data.get('preinstall', []):
    print(app)
")

if [ -z "$APPS" ]; then
    echo "[INFO] 预装清单为空，无需安装"
    exit 0
fi

SUCCESS=0
FAILED=0

for APP_ID in $APPS; do
    SCRIPT="${SCRIPTS_DIR}/install-${APP_ID}.sh"
    if [ -x "$SCRIPT" ]; then
        echo "[INFO] ===== 预装 ${APP_ID} ====="
        if WEBCLAW_DOCKER_BUILD=1 bash "$SCRIPT"; then
            echo "[INFO] ${APP_ID} 预装成功"
            SUCCESS=$((SUCCESS + 1))
        else
            echo "[WARN] ${APP_ID} 安装失败，跳过继续"
            FAILED=$((FAILED + 1))
        fi
    else
        echo "[WARN] 未找到安装脚本: $SCRIPT，跳过 ${APP_ID}"
        FAILED=$((FAILED + 1))
    fi
done

echo "[INFO] 预装完成：成功 ${SUCCESS} 个，失败/跳过 ${FAILED} 个"
