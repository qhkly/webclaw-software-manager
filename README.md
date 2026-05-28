# webclaw-upgrader

WebClaw 容器内的软件升级与进程管理工具。基于 Tauri 2 + 原生 HTML/JS（无前端框架），运行在 `webclaw-docker` 镜像内。

## 架构定位

参见 `../NORTHSTAR.md`：

- **Render 层**（容器内）+ **软件升级 Capability**
- `software-manifest.json` 是该子系统的 Capability Registry 实例
- 老 UI（卡片 + 进度条）和未来的 AI 调用都消费同一份 manifest

## 开发

```bash
npm install
npm run tauri dev   # 桌面开发模式
npm run tauri build # 生成 .deb / .dmg
```

需要 Rust（rustup）和 Node 18+。

## 与 webclaw-docker 集成

`webclaw-upgrader` 通过 .deb 嵌入 `webclaw-docker` 镜像，启动需 sudo NOPASSWD 白名单（apt/npm/supervisorctl/dpkg/tee/curl/tar/mv/chown）。详见 `Dockerfile.base` 的 `WEBCLAW_UPGRADER_VERSION` 层。
