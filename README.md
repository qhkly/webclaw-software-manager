# webclaw-software-manager

WebClaw 跨平台软件商店。基于 Tauri 2 + React（无构建工具），同一个二进制在容器、macOS、Windows 上展示各自平台的可安装软件，支持一键安装与升级。

## 功能概览

- **平台自适应**：自动检测运行环境（Docker 容器 / macOS / Windows），只展示当前平台支持的软件
- **软件商店视图**：全量展示可安装软件，状态分为「可安装 / 可升级 / 已最新 / 待检测」
- **实时安装进度**：通过 Tauri 事件流（`software-progress`）将命令输出逐行推送到前端
- **清单热更新**：启动时从远端拉取 `software-manifest.json`，失败自动降级到本地缓存或内置版本
- **批量升级**：勾选多个可升级项，一键批量操作
- **主题与布局**：内置亮/暗色模式切换、三档卡片密度、自定义主色

## 架构

```
软件清单 (software-manifest.json)
  └─ 远端 GitHub Raw → 本地缓存 → 内置资源（三级降级）

Tauri 后端 (Rust)
  ├─ detect_platform        检测平台（container/macos/windows）
  ├─ refresh_manifest       拉取/缓存清单，返回来源
  ├─ get_platform_catalog   返回当前平台全量软件（初始 state: not_installed）
  ├─ check_latest           并发检测已安装版本 + 最新版本
  ├─ install_software       执行安装，流式推送 software-progress 事件
  └─ upgrade_software       执行升级，流式推送 software-progress 事件

前端 (React / 无 bundler)
  ├─ src/index.html         入口，Babel 浏览器编译
  ├─ src/app.jsx            主应用逻辑
  ├─ src/components.jsx     UI 组件（Header / Stats / Card / ActionModal 等）
  ├─ src/tweaks-panel.jsx   主题调节面板
  └─ src/data.jsx           启动日志初始化
```

## 平台键（Platform Key）

| 运行环境 | 键名 | 检测方式 |
|---|---|---|
| Docker 容器 | `container` | `/.dockerenv` 文件存在 |
| macOS | `macos` | `cfg!(target_os = "macos")` |
| Windows | `windows` | `cfg!(target_os = "windows")` |
| 其他 Linux | `linux` | 默认 |

## 软件清单格式

```json
{
  "_remote_url": "https://raw.githubusercontent.com/qhkly/webclaw-software-manager/main/software-manifest.json",
  "software": [
    {
      "id": "claude-code",
      "name": "Claude Code",
      "category": "AI 工具",
      "group": "Anthropic",
      "risk": "low",
      "desc": "Anthropic 官方 CLI 编程助手",
      "platforms": {
        "container": {
          "detect":  { "type": "npm-global", "pkg": "@anthropic-ai/claude-code" },
          "latest":  { "type": "npm-registry", "pkg": "@anthropic-ai/claude-code" },
          "install": { "type": "npm-global", "pkg": "@anthropic-ai/claude-code" }
        },
        "macos": { "...": "同结构" },
        "windows": { "...": "同结构" }
      }
    }
  ]
}
```

ActionSpec 支持的类型：`NpmGlobal` / `NpmRegistry` / `Apt` / `AptPolicy` / `CustomScript` / `Shell` / `Static` / `Dpkg`

## 开发

```bash
# 需要 Rust（rustup，非 Homebrew）+ Node 18+

npm install
npm run tauri dev      # 热重载开发模式（首次 Rust 编译约 2 分钟）
npm run tauri build    # 生产构建（生成 .dmg / .deb / .msi）
```

## 与 webclaw-docker 集成

`webclaw-docker` 将本仓库作为 git submodule 引入，构建时把 `scripts/` 目录复制到容器内：

```dockerfile
COPY webclaw-software-manager/scripts/ /opt/install-scripts/
```

容器内通过 noVNC 桌面启动 `webclaw-software-manager` GUI 应用，需要 sudo NOPASSWD 白名单（`apt-get`、`npm`、`dpkg`、`bash`）。

## 相关项目

| 项目 | 说明 |
|---|---|
| [webclaw-docker](https://github.com/qhkly/webclaw-docker) | 容器镜像，内嵌本应用 |
| [webclaw-launcher-tauri](https://github.com/jiayq007/webcode-launcher-tauri) | 桌面启动器，管理容器实例 |
| webclaw-upgrader | 容器内 AI 核心服务升级（独立维护，不在本仓库） |
