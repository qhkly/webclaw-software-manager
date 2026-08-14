# webclaw-software-manager

WebClaw 跨平台软件商店。基于 Tauri 2 + React（无构建工具），同一个二进制在容器、macOS、Windows 上展示各自平台的可安装软件，支持一键安装与升级。

## 功能概览

- **平台自适应**：自动检测运行环境（Docker 容器 / macOS / Windows），只展示当前平台支持的软件
- **软件商店视图**：全量展示可安装软件，状态分为「可安装 / 可升级 / 已最新 / 待检测」
- **实时安装进度**：通过 Tauri 事件流（`software-progress`）将命令输出逐行推送到前端
- **清单热更新**：启动时从远端拉取 `software-manifest.json`，失败自动降级到本地缓存或内置版本
- **批量升级**：勾选多个可升级项，一键批量操作
- **账号与权益**：浏览器登录后，关联了 Store 商品的软件会显示「已拥有 / 未购买」
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
  ├─ upgrade_software       执行升级，流式推送 software-progress 事件
  ├─ auth_login/status/logout   浏览器登录、读取登录态、退出
  └─ refresh_entitlements   读 Store 权益（get_cached_entitlements 只读缓存）

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

`store_slug` 和 `official_url` 均为可选字段：

- `store_slug` 只保存 Store 商品标识，界面据此打开 `https://store.qhkly.com/products/{store_slug}`。
- `official_url` 只用于拥有独立官网的旗舰产品。
- 软件管理器不保存价格、套餐或支付状态。价格与购买归 Store 管理，账号权益归 Platform 管理。

例如 AI Studio 的清单项可以包含：

```json
{
  "id": "webcode-ai-studio",
  "store_slug": "webcode-ai-studio",
  "official_url": "https://ai-studio.qhkly.com"
}
```

ActionSpec 支持的类型：`NpmGlobal` / `NpmRegistry` / `Apt` / `AptPolicy` / `CustomScript` / `Shell` / `Static` / `Dpkg`

## 账号与权益

登录走 Platform 的签名票据回调，和 webclaw-launcher-tauri、webcode-ai-studio 是同一套流程
（服务端实现见 `webclaw-platform/lib/launcher-callback.ts`），Platform 侧无需为本项目做任何改动。

```
1. 本机随机端口起一次性 HTTP 监听
2. 浏览器打开 https://webclaw.qhkly.com/login?callback=http://127.0.0.1:<port>/auth-callback&state=<csrf>
3. Platform 带 auth_payload / auth_sig 跳回来
4. 本地验签（RSA-SHA256，公钥内置于 src-tauri/auth-callback-public.pem）
   并校验 iss / aud / state / exp
5. 拿 jti 换正式 token → 存到 auth.json（Unix 下 0600）
6. 带 Bearer token 请求 Store 的 GET /api/licenses 取权益
```

- **权益只来自 Store 一个域**。Store 内部会去 Platform 合并后台补发的权益，客户端不直接访问 Platform 的权益接口。
- **权益是提示，不是授权校验**。未购买的软件照样可以安装，卡片只是把主按钮文案改成「前往购买」。真正的授权校验应由各软件自己启动时完成。
- 权益永远先打网络、失败才回退缓存，并如实标注数据是旧的——和清单那边「缓存优先」的策略相反，因为陈旧的权益会让刚续费的用户看到「未购买」。
- 未登录时界面与接入前完全一致，不显示任何权益徽章。

本地开发可以用环境变量指向别的后端：

```bash
WEBCLAW_PLATFORM_URL=http://localhost:3001 WEBCLAW_STORE_URL=http://localhost:3000 npm run tauri dev
```

**已知限制**：精简版容器镜像没有桌面浏览器，这套回环回调登录用不了，点登录会提示打开浏览器失败。

存储位置（`dirs::data_local_dir()/webclaw-software-manager/`）：

| 文件 | 内容 |
|---|---|
| `auth.json` | 登录 token 和邮箱，Unix 下权限 0600 |
| `entitlements-cache.json` | 权益快照，带 `fetched_at`，仅在网络不通时使用 |
| `manifest-cache.json` | 软件清单缓存 |

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
