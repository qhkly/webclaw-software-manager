// runtime-catalog.json 的严格校验与更新逻辑（无第三方依赖，Node 20+）。
//
// catalog 只放高频变化的「事实数据」：版本、发布时间、每个架构的下载地址和 SHA256。
// 怎么装（install_method / 脚本 / 包名 / 目标路径）是 root policy，只能在镜像里
// root 所有的 /opt/on-demand-apps/<id>.json 里定义，这里一律拒绝。

import { createHash } from 'node:crypto';

export const SCHEMA_VERSION = 1;
export const ARCHES = ['amd64', 'arm64'];

// 出现在 catalog 任意层级都直接拒绝，给出明确原因（additionalProperties 也会拒，但信息不够清楚）。
export const FORBIDDEN_KEYS = [
  'install_method', 'install_script', 'uninstall_script', 'install_wrapper', 'postinstall',
  'shell', 'cmd', 'command', 'script', 'exec', 'args',
  'package', 'apt_package', 'binary', 'launch_binary', 'launch_script',
  'target', 'target_path', 'path', 'dest', 'install_dir',
];

const APP_ID_RE = /^[a-z0-9][a-z0-9._-]{0,63}$/;
const VERSION_RE = /^[A-Za-z0-9][A-Za-z0-9._+~-]{0,63}$/;
const SHA256_RE = /^[0-9a-f]{64}$/;
const RFC3339_RE = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/;

const isObject = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);

export function isRfc3339(v) {
  return typeof v === 'string' && RFC3339_RE.test(v) && !Number.isNaN(Date.parse(v));
}

export function isValidAppId(id) {
  return typeof id === 'string' && APP_ID_RE.test(id) && !id.includes('..');
}

export function isHttpsUrl(v) {
  if (typeof v !== 'string' || v.length > 2048 || /[\s"'`\\]/.test(v)) return false;
  try {
    const u = new URL(v);
    return u.protocol === 'https:' && !u.username && !u.password && !!u.hostname;
  } catch {
    return false;
  }
}

function findForbiddenKeys(value, where, errors) {
  if (Array.isArray(value)) {
    value.forEach((v, i) => findForbiddenKeys(v, `${where}[${i}]`, errors));
  } else if (isObject(value)) {
    for (const [k, v] of Object.entries(value)) {
      if (FORBIDDEN_KEYS.includes(k)) {
        errors.push(`${where}.${k}: root policy 字段不允许出现在 runtime-catalog 中`);
      }
      findForbiddenKeys(v, `${where}.${k}`, errors);
    }
  }
}

function checkKeys(obj, where, required, optional, errors) {
  for (const k of required) {
    if (!(k in obj)) errors.push(`${where}: 缺少字段 ${k}`);
  }
  for (const k of Object.keys(obj)) {
    if (!required.includes(k) && !optional.includes(k) && !FORBIDDEN_KEYS.includes(k)) {
      errors.push(`${where}: 未知字段 ${k}`);
    }
  }
}

/** 返回错误列表；空数组表示合法。与 runtime-catalog.schema.json 等价且更严格（RFC3339 真实日期、URL 解析）。 */
export function validateCatalog(catalog) {
  const errors = [];
  if (!isObject(catalog)) return ['catalog 必须是 JSON 对象'];
  findForbiddenKeys(catalog, '$', errors);
  checkKeys(catalog, '$', ['schema_version', 'generated_at', 'apps'], [], errors);
  if (catalog.schema_version !== SCHEMA_VERSION) {
    errors.push(`$.schema_version 必须是 ${SCHEMA_VERSION}`);
  }
  if (!isRfc3339(catalog.generated_at)) errors.push('$.generated_at 必须是 RFC3339 时间');
  if (!isObject(catalog.apps)) {
    errors.push('$.apps 必须是对象');
    return errors;
  }
  for (const [id, app] of Object.entries(catalog.apps)) {
    const w = `$.apps.${id}`;
    if (!isValidAppId(id)) errors.push(`${w}: 非法 app_id`);
    if (!isObject(app)) {
      errors.push(`${w} 必须是对象`);
      continue;
    }
    checkKeys(app, w, ['version', 'artifacts'], ['released_at'], errors);
    if (typeof app.version !== 'string' || !VERSION_RE.test(app.version)) {
      errors.push(`${w}.version 非法`);
    }
    if ('released_at' in app && !isRfc3339(app.released_at)) {
      errors.push(`${w}.released_at 必须是 RFC3339 时间`);
    }
    if (!isObject(app.artifacts) || Object.keys(app.artifacts).length === 0) {
      errors.push(`${w}.artifacts 必须是非空对象`);
      continue;
    }
    for (const [arch, art] of Object.entries(app.artifacts)) {
      const aw = `${w}.artifacts.${arch}`;
      if (!ARCHES.includes(arch)) errors.push(`${aw}: 只允许 ${ARCHES.join('/')}`);
      if (!isObject(art)) {
        errors.push(`${aw} 必须是对象`);
        continue;
      }
      checkKeys(art, aw, ['url', 'sha256'], [], errors);
      if (!isHttpsUrl(art.url)) errors.push(`${aw}.url 必须是 https:// 地址`);
      if (typeof art.sha256 !== 'string' || !SHA256_RE.test(art.sha256)) {
        errors.push(`${aw}.sha256 必须是 64 位小写十六进制`);
      }
    }
  }
  return errors;
}

// ─── 数据源配置（tools/runtime-catalog.sources.json）────────────────────
// 只支持能可靠自动发现的来源：GitHub Releases、明确的 JSON 版本接口 + URL 模板、
// JetBrains 官方 releases API、Cursor 官方下载 API。

// JetBrains releases API 里各架构的下载键（固定，不来自配置）
const JETBRAINS_DOWNLOAD_KEYS = { amd64: 'linux', arm64: 'linuxARM64' };

function checkUrlPrefixes(prefixes, where, errors) {
  if (!Array.isArray(prefixes) || !prefixes.length) {
    errors.push(`${where} 必须是非空数组`);
    return;
  }
  for (const pre of prefixes) {
    // 与 Docker 侧 catalog_url_prefixes 相同的形状：https://host/…，以 / 结尾防止 host 前缀拼接绕过
    if (!isHttpsUrl(pre) || !/^https:\/\/[a-z0-9]([a-z0-9.-]*[a-z0-9])?\/([^?#]*\/)?$/.test(pre)) {
      errors.push(`${where}: ${pre} 必须是以 / 结尾的 https:// 前缀`);
    }
  }
}

function assertUnderPrefixes(id, url, prefixes) {
  if (!isHttpsUrl(url) || !prefixes.some((pre) => url.startsWith(pre))) {
    throw new Error(`${id}: 地址不在允许的前缀内：${url}`);
  }
}

const TEMPLATE_RE = /\{(version|version_no_v)\}/g;

function checkTemplate(t, where, errors, { url }) {
  if (typeof t !== 'string' || !t) {
    errors.push(`${where} 必须是非空字符串`);
    return;
  }
  const leftover = t.replace(TEMPLATE_RE, '');
  if (/[{}]/.test(leftover)) errors.push(`${where}: 只允许 {version} / {version_no_v} 占位符`);
  if (url && !isHttpsUrl(t.replace(TEMPLATE_RE, '0'))) errors.push(`${where} 必须是 https:// 地址模板`);
  if (!url && /[/\\]/.test(t)) errors.push(`${where}: 资产名不能含路径分隔符`);
}

export function validateSources(sources) {
  const errors = [];
  if (!isObject(sources) || !isObject(sources.apps)) return ['sources.apps 必须是对象'];
  checkKeys(sources, '$', ['apps'], ['$comment'], errors);
  findForbiddenKeys(sources, '$', errors);
  for (const [id, src] of Object.entries(sources.apps)) {
    const w = `$.apps.${id}`;
    if (!isValidAppId(id)) errors.push(`${w}: 非法 app_id`);
    if (!isObject(src)) {
      errors.push(`${w} 必须是对象`);
      continue;
    }
    if (src.type === 'github-release') {
      checkKeys(src, w, ['type', 'repo', 'assets'], ['note'], errors);
      if (!/^[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+$/.test(src.repo || '')) errors.push(`${w}.repo 非法`);
      const assets = isObject(src.assets) ? src.assets : {};
      if (!Object.keys(assets).length) errors.push(`${w}.assets 不能为空`);
      for (const [arch, t] of Object.entries(assets)) {
        if (!ARCHES.includes(arch)) errors.push(`${w}.assets.${arch}: 未知架构`);
        checkTemplate(t, `${w}.assets.${arch}`, errors, { url: false });
      }
    } else if (src.type === 'json-version-api') {
      checkKeys(src, w, ['type', 'version_url', 'artifacts'], ['version_field', 'released_at_field', 'note'], errors);
      if (!isHttpsUrl(src.version_url)) errors.push(`${w}.version_url 必须是 https://`);
      const arts = isObject(src.artifacts) ? src.artifacts : {};
      if (!Object.keys(arts).length) errors.push(`${w}.artifacts 不能为空`);
      for (const [arch, t] of Object.entries(arts)) {
        if (!ARCHES.includes(arch)) errors.push(`${w}.artifacts.${arch}: 未知架构`);
        checkTemplate(t, `${w}.artifacts.${arch}`, errors, { url: true });
      }
    } else if (src.type === 'jetbrains-release') {
      checkKeys(src, w, ['type', 'code', 'url_prefixes'], ['note'], errors);
      if (!/^[A-Z]{2,8}$/.test(src.code || '')) errors.push(`${w}.code 必须是 JetBrains 产品代码（如 IIC）`);
      checkUrlPrefixes(src.url_prefixes, `${w}.url_prefixes`, errors);
    } else if (src.type === 'cursor-api') {
      checkKeys(src, w, ['type', 'endpoint', 'platforms', 'url_prefixes'], ['note'], errors);
      if (typeof src.endpoint !== 'string' || !src.endpoint.includes('{platform}')
          || !isHttpsUrl(src.endpoint.replace('{platform}', 'x')) || /[{}]/.test(src.endpoint.replace('{platform}', ''))) {
        errors.push(`${w}.endpoint 必须是只含 {platform} 占位符的 https:// 地址`);
      }
      const plats = isObject(src.platforms) ? src.platforms : {};
      if (!Object.keys(plats).length) errors.push(`${w}.platforms 不能为空`);
      for (const [arch, plat] of Object.entries(plats)) {
        if (!ARCHES.includes(arch)) errors.push(`${w}.platforms.${arch}: 未知架构`);
        if (typeof plat !== 'string' || !/^[a-z0-9-]{1,32}$/.test(plat)) errors.push(`${w}.platforms.${arch} 非法`);
      }
      checkUrlPrefixes(src.url_prefixes, `${w}.url_prefixes`, errors);
    } else {
      errors.push(`${w}.type 只支持 github-release / json-version-api / jetbrains-release / cursor-api`);
    }
  }
  return errors;
}

function fill(template, version) {
  return template
    .replaceAll('{version_no_v}', version.replace(/^v/, ''))
    .replaceAll('{version}', version);
}

function normalizeVersion(raw, where) {
  const version = String(raw ?? '').trim().replace(/^v/, '');
  if (!VERSION_RE.test(version)) throw new Error(`${where}: 拿不到合法版本号 ${JSON.stringify(raw)}`);
  return version;
}

const REDIRECT_STATUSES = new Set([301, 302, 303, 307, 308]);
export const MAX_REDIRECTS = 8;
const GITHUB_API_HOST = 'api.github.com';
const USER_AGENT = 'webclaw-runtime-catalog';

/**
 * 手动跟随重定向：每一跳都必须是 https、不带用户名密码，循环或超过 maxRedirects 拒绝。
 * Authorization 只在目标主机正好是 api.github.com 时附加（token 绝不会发到 artifact/CDN）。
 */
export async function fetchHttps(url, { fetchImpl = fetch, accept, githubToken, maxRedirects = MAX_REDIRECTS } = {}) {
  let current = url;
  const seen = new Set();
  for (let hop = 0; ; hop++) {
    if (!isHttpsUrl(current)) throw new Error(`拒绝非 https 地址（第 ${hop} 跳）：${current}`);
    if (seen.has(current)) throw new Error(`重定向循环：${current}`);
    seen.add(current);
    const headers = { 'User-Agent': USER_AGENT };
    if (accept) headers.Accept = accept;
    if (githubToken && new URL(current).hostname === GITHUB_API_HOST) headers.Authorization = `Bearer ${githubToken}`;
    const res = await fetchImpl(current, { redirect: 'manual', headers });
    if (!REDIRECT_STATUSES.has(res.status)) return res;
    if (hop >= maxRedirects) throw new Error(`重定向超过 ${maxRedirects} 跳：${url}`);
    const location = res.headers?.get?.('location');
    if (!location) throw new Error(`HTTP ${res.status} 重定向缺少 Location：${current}`);
    let next;
    try {
      next = new URL(location, current).toString();
    } catch {
      throw new Error(`无法解析重定向地址：${location}`);
    }
    current = next;
  }
}

async function fetchJson(fetchImpl, url, { githubToken, accept = 'application/json' } = {}) {
  const res = await fetchHttps(url, { fetchImpl, accept, githubToken });
  if (!res.ok) throw new Error(`GET ${url} → HTTP ${res.status}`);
  return res.json();
}

/**
 * 查询某个来源的最新版本。只查元数据，不下载 artifact。
 * @returns {{version: string, released_at?: string, urls: Record<string,string>}}
 */
export async function resolveLatest(id, src, { fetchImpl = fetch, githubToken } = {}) {
  if (src.type === 'github-release') {
    // token 只随 api.github.com 的元数据请求发送（见 fetchHttps），不会带到 artifact 下载上。
    const rel = await fetchJson(fetchImpl, `https://${GITHUB_API_HOST}/repos/${src.repo}/releases/latest`, {
      githubToken,
      accept: 'application/vnd.github+json',
    });
    const tag = String(rel.tag_name || '');
    const version = normalizeVersion(tag, id);
    const assets = Array.isArray(rel.assets) ? rel.assets : [];
    const urls = {};
    for (const [arch, template] of Object.entries(src.assets)) {
      const name = fill(template, tag);
      const asset = assets.find((a) => a.name === name);
      if (!asset) throw new Error(`${id}: release ${tag} 里没有 ${arch} 资产 ${name}`);
      urls[arch] = asset.browser_download_url;
    }
    return { version, released_at: isRfc3339(rel.published_at) ? rel.published_at : undefined, urls };
  }
  if (src.type === 'json-version-api') {
    const json = await fetchJson(fetchImpl, src.version_url);
    const version = normalizeVersion(json[src.version_field || 'version'], id);
    const released = src.released_at_field ? json[src.released_at_field] : undefined;
    const urls = {};
    for (const [arch, template] of Object.entries(src.artifacts)) urls[arch] = fill(template, version);
    return { version, released_at: isRfc3339(released) ? released : undefined, urls };
  }
  if (src.type === 'jetbrains-release') {
    // 官方 API：https://data.services.jetbrains.com/products/releases?code=<CODE>&latest=true&type=release
    const api = `https://data.services.jetbrains.com/products/releases?code=${encodeURIComponent(src.code)}&latest=true&type=release`;
    const json = await fetchJson(fetchImpl, api);
    const rel = Array.isArray(json?.[src.code]) ? json[src.code][0] : undefined;
    if (!isObject(rel)) throw new Error(`${id}: JetBrains API 没有 ${src.code} 的 release`);
    const version = normalizeVersion(rel.version, id);
    const urls = {};
    const checksums = {};
    for (const [arch, key] of Object.entries(JETBRAINS_DOWNLOAD_KEYS)) {
      const dl = rel.downloads?.[key];
      if (!isObject(dl) || typeof dl.link !== 'string') throw new Error(`${id}: ${version} 没有 ${arch}（${key}）下载`);
      assertUnderPrefixes(id, dl.link, src.url_prefixes);
      // 官方校验和是必需的：catalog 直接采用它，不为生成 catalog 下载几百 MB～1GB 的 tar.gz。
      // Docker broker 安装时仍会下载 artifact 并用这里的 SHA256 校验。
      if (typeof dl.checksumLink !== 'string') throw new Error(`${id}: ${version} 的 ${arch} 没有官方 checksumLink`);
      assertUnderPrefixes(id, dl.checksumLink, src.url_prefixes);
      urls[arch] = dl.link;
      checksums[arch] = dl.checksumLink;
    }
    const released = /^\d{4}-\d{2}-\d{2}$/.test(rel.date || '') ? `${rel.date}T00:00:00Z` : undefined;
    return { version, released_at: isRfc3339(released) ? released : undefined, urls, checksums };
  }
  if (src.type === 'cursor-api') {
    // 官方 API（与 Docker cursor_api 的 api_base 同源）：每个平台返回 {version, downloadUrl}
    let version;
    const urls = {};
    for (const [arch, platform] of Object.entries(src.platforms)) {
      const json = await fetchJson(fetchImpl, src.endpoint.replace('{platform}', platform));
      const v = normalizeVersion(json?.version, id);
      if (version && v !== version) throw new Error(`${id}: 各架构版本不一致（${version} / ${v}）`);
      version = v;
      const url = json?.downloadUrl;
      if (typeof url !== 'string') throw new Error(`${id}: ${platform} 没有 downloadUrl`);
      assertUnderPrefixes(id, url, src.url_prefixes);
      // 资产名必须带着同一个版本号，防止 API 返回与 version 不符的包
      if (!decodeURIComponent(new URL(url).pathname).split('/').pop().includes(v)) {
        throw new Error(`${id}: ${platform} 的下载地址与版本 ${v} 不符：${url}`);
      }
      urls[arch] = url;
    }
    return { version, urls };
  }
  throw new Error(`${id}: 不支持的来源类型 ${src.type}`);
}

/** 读取上游发布的 .sha256 文件（格式：`<64 hex> [*]<文件名>`），并核对文件名与 artifact 一致。 */
async function fetchPublishedSha256(checksumUrl, artifactUrl, fetchImpl) {
  const res = await fetchHttps(checksumUrl, { fetchImpl });
  if (!res.ok) throw new Error(`GET ${checksumUrl} → HTTP ${res.status}`);
  const text = (await res.text()).slice(0, 4096);
  const m = /^\s*([0-9a-fA-F]{64})(?:\s+\*?(\S+))?\s*$/.exec(text);
  if (!m) throw new Error(`${checksumUrl} 不是合法的 sha256 文件`);
  const expectedName = decodeURIComponent(new URL(artifactUrl).pathname).split('/').pop();
  if (m[2] && m[2] !== expectedName) throw new Error(`${checksumUrl} 对应的文件是 ${m[2]}，不是 ${expectedName}`);
  return m[1].toLowerCase();
}

/** 流式下载并计算 SHA256（不落盘）。重定向链的每一跳都必须是 https，且从不携带 Authorization。 */
export async function sha256OfUrl(url, { fetchImpl = fetch, maxBytes = 4 * 1024 ** 3, maxRedirects = MAX_REDIRECTS } = {}) {
  const res = await fetchHttps(url, { fetchImpl, maxRedirects });
  if (!res.ok) throw new Error(`下载 ${url} → HTTP ${res.status}`);
  if (!res.body) throw new Error(`下载 ${url} 没有响应体`);
  const hash = createHash('sha256');
  let size = 0;
  for await (const chunk of res.body) {
    size += chunk.length;
    if (size > maxBytes) throw new Error(`下载 ${url} 超过 ${maxBytes} 字节上限`);
    hash.update(chunk);
  }
  if (size === 0) throw new Error(`下载 ${url} 是空文件`);
  return hash.digest('hex');
}

function sameUrls(artifacts, urls) {
  const a = Object.keys(artifacts || {}).sort();
  const b = Object.keys(urls).sort();
  return a.length === b.length && a.every((k, i) => k === b[i] && artifacts[k].url === urls[k]);
}

/**
 * 计算新的 catalog。
 *  - 版本和 URL 都没变：原样沿用上一版的 url/sha256，不下载；
 *  - 有变化：下载各架构 artifact 计算 SHA256；
 *  - 某个 app 失败：保留它上一版（last-known-good），记入 failures，不影响其它 app；
 *  - generated_at 只在 apps 真的变化时更新，避免每天产生无意义的提交。
 */
export async function updateCatalog({ prev, sources, fetchImpl = fetch, githubToken, now = () => new Date(), log = () => {} }) {
  const prevApps = (prev && isObject(prev.apps)) ? prev.apps : {};
  const apps = {};
  const changed = [];
  const failures = [];
  let downloads = 0;

  for (const id of Object.keys(sources.apps).sort()) {
    const src = sources.apps[id];
    const prevEntry = prevApps[id];
    try {
      const latest = await resolveLatest(id, src, { fetchImpl, githubToken });
      if (prevEntry && prevEntry.version === latest.version && sameUrls(prevEntry.artifacts, latest.urls)) {
        apps[id] = prevEntry;
        log(`${id}: ${latest.version} 未变化`);
        continue;
      }
      const artifacts = {};
      for (const arch of Object.keys(latest.urls).sort()) {
        let sha256;
        if (latest.checksums?.[arch]) {
          // 上游发布了官方校验和（JetBrains）：直接采用，不下载 artifact 本体
          log(`${id}: 读取 ${arch} 官方校验和 ${latest.checksums[arch]}`);
          sha256 = await fetchPublishedSha256(latest.checksums[arch], latest.urls[arch], fetchImpl);
        } else {
          log(`${id}: 下载 ${arch} ${latest.urls[arch]}`);
          downloads += 1;
          sha256 = await sha256OfUrl(latest.urls[arch], { fetchImpl });
        }
        artifacts[arch] = { url: latest.urls[arch], sha256 };
      }
      const entry = { version: latest.version };
      if (latest.released_at) entry.released_at = latest.released_at;
      entry.artifacts = artifacts;
      const errors = validateCatalog({ schema_version: SCHEMA_VERSION, generated_at: now().toISOString(), apps: { [id]: entry } });
      if (errors.length) throw new Error(errors.join('; '));
      apps[id] = entry;
      changed.push(id);
      log(`${id}: ${prevEntry?.version ?? '（新）'} → ${latest.version}`);
    } catch (error) {
      failures.push({ id, error: error.message });
      if (prevEntry) apps[id] = prevEntry;
      log(`${id}: 失败，${prevEntry ? `保留上一版 ${prevEntry.version}` : '暂不收录'}：${error.message}`);
    }
  }

  const removed = Object.keys(prevApps).filter((id) => !(id in sources.apps));
  const dirty = changed.length > 0 || removed.length > 0 || !prev;
  const catalog = {
    schema_version: SCHEMA_VERSION,
    generated_at: dirty || !isRfc3339(prev?.generated_at) ? now().toISOString().replace(/\.\d{3}Z$/, 'Z') : prev.generated_at,
    apps,
  };
  const errors = validateCatalog(catalog);
  if (errors.length) throw new Error(`生成的 catalog 不合法：\n${errors.join('\n')}`);
  return { catalog, changed, removed, failures, downloads };
}

export function serializeCatalog(catalog) {
  return `${JSON.stringify(catalog, null, 2)}\n`;
}
