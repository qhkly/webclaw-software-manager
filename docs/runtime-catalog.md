# runtime-catalog.json

WebClaw 容器通过 `webclaw-catalog-update` 从固定地址拉取本仓库 `main` 分支上的
`runtime-catalog.json`：

```
https://raw.githubusercontent.com/qhkly/webclaw-software-manager/main/runtime-catalog.json
```

catalog 让第三方软件升级**不需要重打 Docker 镜像**：镜像里 root 所有的
`/opt/on-demand-apps/<id>.json` 决定「怎么装、允许从哪下载」（root policy），
catalog 只提供高频变化的事实——版本号和每个架构 artifact 的 URL + SHA256。

## 格式（schema v1）

```json
{
  "schema_version": 1,
  "generated_at": "2026-09-25T01:55:38Z",
  "apps": {
    "cc-switch": {
      "version": "3.20.4",
      "released_at": "2026-09-22T14:42:23Z",
      "artifacts": {
        "amd64": { "url": "https://...", "sha256": "<64 位小写十六进制>" },
        "arm64": { "url": "https://...", "sha256": "..." }
      }
    }
  }
}
```

- 严格校验：`runtime-catalog.schema.json`（JSON Schema 2020-12）与
  `tools/lib/runtime-catalog.mjs` 的 `validateCatalog()`（等价，并额外校验真实日期与 URL 解析）。
- 任何多余字段都会被拒绝；`install_method`、`install_script`、`shell`、`package`、
  `binary`、`path`、`target` 等 root policy 字段出现在任意层级都会被明确拒绝。
- URL 必须是 `https://`，不能带用户名密码。Docker 侧还会再要求 URL 落在该 app 本地 manifest 允许的来源内。
- `apps` 可以只覆盖部分应用；apt 类和用户 npm CLI 不进入 catalog。
- `generated_at` 只在 `apps` 真正变化时更新（避免每天空提交）。判断缓存是否陈旧请用容器侧
  `catalog-info` 的 `last_success_at`，不要用 `generated_at`。

## 数据来源

`tools/runtime-catalog.sources.json` 列出自动发现来源，只收录能**可靠**确定最新版本和两个架构 artifact 的应用：

| 类型 | 说明 | 当前应用 |
|---|---|---|
| `github-release` | `releases/latest` 的 tag + 按模板精确匹配资产名 | cc-switch、dbeaver、dockyard、opentypeless、ghostty |
| `json-version-api` | 明确的 JSON 版本接口 + URL 模板 | webclaw-launcher |
| `jetbrains-release` | JetBrains 官方 `data.services.jetbrains.com/products/releases?code=<CODE>&latest=true&type=release`，按 `linux` / `linuxARM64` 取下载地址；SHA256 直接读取官方 `checksumLink`（`.sha256` 文件，核对文件名），**不下载** tar.gz 本体。Docker broker 安装时仍会下载 artifact 并按这里的 SHA256 校验 | intellij（IIC）、pycharm（PCP） |
| `cursor-api` | Cursor 官方 `api2.cursor.sh/updates/api/download/stable/{linux-x64,linux-arm64}/cursor`，两个平台版本必须一致，资产名须含同一版本号 | cursor |

`jetbrains-release` / `cursor-api` 必须声明 `url_prefixes`，解析出的每个下载地址（及校验和地址）都必须落在其中，
取值与镜像内 `/opt/on-demand-apps/<id>.json` 的 `catalog_url_prefixes` 一致（JetBrains：`https://download.jetbrains.com/`、
`https://download-cdn.jetbrains.com/`；Cursor：`https://downloads.cursor.com/`），越界的地址根本不会被下载。

说明：
- JetBrains 产品代码与 Docker 侧 `jetbrains_code` 保持一致。IntelliJ 的 `IIC`（Community）上游已停在 2025.3，
  如需跟进统一版 IntelliJ（`IIU`），需要 Docker 侧 policy 与这里同时改。
- bundled 清单里 Cursor 的旧版本接口 `api2.cursor.sh/updates/latest` 已返回 404；Docker 侧固定的 `3.2` golden 轨道
  也远落后于 stable。catalog 改用同源的官方 stable 下载接口，它直接返回 `downloads.cursor.com` 上带版本号的 AppImage。

暂不收录（保持镜像内现状，不猜版本）：Trae、Eclipse、WeChat（没有可靠的版本接口）、
Obsidian（`releases/latest` 目前只有 Android 包，没有 AppImage）、Android Studio（版本接口是镜像内本地脚本）。

## 更新工具

```bash
node tools/update-runtime-catalog.mjs            # 查询最新版本；只有版本/URL 变化才下载并计算 SHA256；原子写回
node tools/update-runtime-catalog.mjs --dry-run  # 不写文件，把结果打印到 stdout
node tools/update-runtime-catalog.mjs --check    # 同上；有待更新内容时退出码 1
node tools/update-runtime-catalog.mjs --validate # 离线校验 catalog 与 sources
node --test tools/test/runtime-catalog.test.mjs  # 单元测试（不联网）
```

退出码：`0` 正常，`1` `--check` 发现变化，`2` 部分 app 失败（这些 app 保留上一版条目，其它照常更新），`3` 致命错误（不写文件）。

安全细节：
- 下载时手动跟随重定向（最多 8 跳），每一跳都必须是 https、不带用户名密码，循环直接拒绝。
- `GITHUB_TOKEN` 只附加在发往 `api.github.com` 的元数据请求上；artifact 下载及其重定向从不携带 Authorization。
- 没有官方校验和的来源（GitHub、webclaw-launcher、Cursor）在版本变化时流式下载计算 SHA256，不落盘；有官方校验和的（JetBrains）只读校验和文件。
- 写文件用同目录临时文件 + fsync + rename。
- 工具放在 `tools/` 而不是 `scripts/`：`scripts/` 会被 `webclaw-scripts-updater` 整体同步进容器的 `/opt/install-scripts/`。

## GitHub Actions

`.github/workflows/runtime-catalog.yml`（独立于 `release.yml`）：

- 每天 03:17 UTC 运行 + 支持手动触发；改动 catalog/tools 的 PR 上只跑校验与测试。
- 有变化时：上传候选文件为 workflow artifact，并把改动 force-push 到 `automation/runtime-catalog` 分支、开/更新 PR。
  合并 PR 后容器下次拉取即生效。**不会**触发 Docker 镜像构建。
- 如果仓库不允许 Actions 创建 PR，job summary 会给出下一步（开启
  *Settings → Actions → General → Allow GitHub Actions to create and approve pull requests*，或手动用 artifact 提 PR）。
- 只使用内置 `github.token`，不需要额外的 PAT。

## Detached 签名（预留，需一次性配置）

Docker 侧：`/etc/webclaw/runtime-catalog.pub` 存在时签名变为强制，容器会下载
`runtime-catalog.json.sig` 并执行 `openssl pkeyutl -verify -pubin -inkey ... -rawin`（Ed25519，对文件原始字节签名）。

一次性配置（在离线/可信机器上）：

```bash
openssl genpkey -algorithm ed25519 -out runtime-catalog-signing.pem   # 私钥：绝不提交
openssl pkey -in runtime-catalog-signing.pem -pubout -out runtime-catalog.pub

# 私钥放进 GitHub secret（仓库 Settings → Secrets and variables → Actions）
gh secret set RUNTIME_CATALOG_SIGNING_KEY < runtime-catalog-signing.pem

# 先给当前 catalog 补一份签名并提交，再把 runtime-catalog.pub 交给 webclaw-docker
# 安装到镜像的 /etc/webclaw/runtime-catalog.pub（root:root 0644）
openssl pkeyutl -sign -inkey runtime-catalog-signing.pem -rawin \
  -in runtime-catalog.json -out runtime-catalog.json.sig
```

顺序很重要：**先**让 `main` 上有匹配的 `.sig`，**再**发布带公钥的镜像，否则新镜像会拒绝 catalog。
配置 secret 后，workflow 每次 catalog 变化都会重新生成 `.sig` 并放进同一个 PR；
如果仓库里已有 `.sig` 而 secret 缺失，workflow 会失败而不是产出签名不匹配的 PR。
