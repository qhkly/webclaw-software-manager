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
  ├─ uninstall_software     卸载（仅 broker 条目；broker 可能返回不支持）
  ├─ broker_runtime_status  容器 broker API 版本 + runtime-catalog 状态
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

ActionSpec 支持的类型：`NpmGlobal` / `NpmRegistry` / `Apt` / `AptPolicy` / `CustomScript` / `Shell` / `Static` / `Dpkg` / `Binary` / `GithubReleaseLatest` / `Broker`

### 容器后端

| 后端 | 如何得到 | 适用 |
|---|---|---|
| broker | 客户端把白名单内 app 的旧 `apt` / `custom-script` 写法映射为 `webclaw-app-admin` 高层 API | 需要 root 且 broker 能端到端 install/upgrade 的系统/桌面软件（apt、.deb、AppImage/归档/cursor_api，以及声明了 upgrade_by_reinstall 的 qq/telegram/discord，共 24 个） |
| user-node | `npm-global` / `npm-registry`（旧的 Claude/OpenCode root 脚本写法也会被映射过来） | Claude Code、Codex、OpenCode 等用户 NVM CLI，经 `/usr/local/bin/webclaw-user-node-run`，不用 root |
| legacy | `custom-script`（sudoers 逐个放行的固定脚本） | 暂时没有端到端 broker 支持的软件 |

**发布的 `software-manifest.json` 保持旧格式**（不含 `"type": "broker"`），老版本客户端照常读取；
新客户端在 `src-tauri/src/commands/manifest.rs` 里按硬编码白名单 `BROKER_APPS` 做 normalize：
只有 id 在白名单中、且 action 与历史内置写法完全一致时才映射为 broker，远程清单里任意别的
custom-script / apt / shell 都原样保留，绝不因此获得 broker 权限。`ActionSpec::Broker`
（`{"type":"broker","app_id":"vscode","min_api_version":2}`）作为内部能力保留，客户端同样要求
`app_id == id` 且在白名单内，但当前公开清单不使用它——等老客户端淘汰后再考虑。

**容器安装策略只信任随客户端发布的 bundled 清单。** 远程/缓存清单可以更新名称、描述、分组、商品链接等元数据，
但 `platforms.container`（含 shell / custom-script 等可执行 action）每次加载时都替换为 bundled 里同 id 的版本；
bundled 里没有的远程新 app 在容器平台不提供任何后端；bundled 读不到时容器条目全部移除（fail closed）。macOS / Windows 暂保持原样。

broker v2 镜像上，新客户端不再调用 `refresh_scripts`（即不再 sudo 运行 `webclaw-scripts-updater` 从远程 main 覆盖
`/opt/install-scripts`），legacy 脚本以镜像内版本为准；只有没有 v2 broker 的旧镜像才继续走旧的脚本刷新。
apt 类 broker 软件的最新版本由客户端只读执行 `apt-cache policy <包名>` 获得（包名来自硬编码白名单），升级仍走 broker。

broker 调用固定为 `sudo -n -- /usr/local/bin/webclaw-app-admin <status|install|upgrade|uninstall> <app_id>`（argv 直接执行，不经 shell；
sudoers 只放行这一个入口），启动时先探测 `api-version`。镜像里没有 broker 或 API 版本低于 2 时，界面显示
「WebClaw 镜像运行时过旧，需要一次性升级镜像」，不会退回到宽泛 sudo。

`runtime-catalog.json`（版本 + 各架构下载地址与 SHA256）见 [docs/runtime-catalog.md](docs/runtime-catalog.md)。

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

容器内通过 noVNC 桌面启动 `webclaw-software-manager` GUI 应用。root 操作只经过 sudoers 精确放行的受控入口：
broker `/usr/local/bin/webclaw-app-admin`（api v2）与逐个列出的 legacy 安装脚本；用户 CLI 走 `webclaw-user-node-run`，不需要 sudo。

## 相关项目

| 项目 | 说明 |
|---|---|
| [webclaw-docker](https://github.com/qhkly/webclaw-docker) | 容器镜像，内嵌本应用 |
| [webclaw-launcher-tauri](https://github.com/jiayq007/webcode-launcher-tauri) | 桌面启动器，管理容器实例 |
| webclaw-upgrader | 容器内 AI 核心服务升级（独立维护，不在本仓库） |
