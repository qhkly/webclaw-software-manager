// node --test tools/test/runtime-catalog.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import {
  updateCatalog, validateCatalog, validateSources, sha256OfUrl, serializeCatalog,
} from '../lib/runtime-catalog.mjs';
import { writeAtomic } from '../update-runtime-catalog.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const sha = (s) => createHash('sha256').update(s).digest('hex');
const NOW = () => new Date('2026-09-25T00:00:00Z');

const SOURCES = {
  apps: {
    alpha: { type: 'github-release', repo: 'o/alpha', assets: { amd64: 'alpha_{version_no_v}_amd64.deb', arm64: 'alpha_{version_no_v}_arm64.deb' } },
    beta: {
      type: 'json-version-api',
      version_url: 'https://example.com/latest.json',
      released_at_field: 'publishedAt',
      artifacts: { amd64: 'https://dl.example.com/v{version}/beta-x64.zip', arm64: 'https://dl.example.com/v{version}/beta-arm64.zip' },
    },
  },
};

/**
 * 假网络：记录每次请求（含 headers）；routes 值可以是 JSON 对象、字符串 body、
 * {status} 或 {redirect: '<Location>', status?: 302}。
 */
function fakeFetch(routes) {
  const calls = [];
  const impl = async (url, init = {}) => {
    calls.push({ url, headers: { ...(init.headers || {}) }, redirect: init.redirect });
    const r = routes[url];
    if (r === undefined) return { ok: false, status: 404, url };
    if (r && r.redirect) {
      return { ok: false, status: r.status || 302, url, headers: { get: (k) => (k.toLowerCase() === 'location' ? r.redirect : null) } };
    }
    if (r && r.status) return { ok: false, status: r.status, url };
    if (typeof r === 'string') return { ok: true, status: 200, url, body: [Buffer.from(r)], text: async () => r };
    return { ok: true, status: 200, url, json: async () => r };
  };
  impl.calls = calls;
  impl.downloads = () => calls.map((c) => c.url).filter((u) => /\.(deb|zip|tar\.gz|AppImage)(\?|$)/.test(u));
  return impl;
}

function ghRelease(tag, names) {
  return {
    tag_name: tag,
    published_at: '2026-09-20T10:00:00Z',
    assets: names.map((n) => ({ name: n, browser_download_url: `https://github.com/o/alpha/releases/download/${tag}/${n}` })),
  };
}

function routesFor({ alpha = '1.2.0', beta = '0.9.1' } = {}) {
  const gh = `https://github.com/o/alpha/releases/download/v${alpha}/`;
  return {
    'https://api.github.com/repos/o/alpha/releases/latest': ghRelease(`v${alpha}`, [`alpha_${alpha}_amd64.deb`, `alpha_${alpha}_arm64.deb`]),
    [`${gh}alpha_${alpha}_amd64.deb`]: `alpha-amd64-${alpha}`,
    [`${gh}alpha_${alpha}_arm64.deb`]: `alpha-arm64-${alpha}`,
    'https://example.com/latest.json': { version: beta, publishedAt: '2026-09-21T00:00:00.000Z' },
    [`https://dl.example.com/v${beta}/beta-x64.zip`]: `beta-x64-${beta}`,
    [`https://dl.example.com/v${beta}/beta-arm64.zip`]: `beta-arm64-${beta}`,
  };
}

const validCatalog = () => ({
  schema_version: 1,
  generated_at: '2026-09-25T00:00:00Z',
  apps: {
    alpha: {
      version: '1.2.0',
      released_at: '2026-09-20T10:00:00Z',
      artifacts: {
        amd64: { url: 'https://github.com/o/alpha/releases/download/v1.2.0/a.deb', sha256: 'a'.repeat(64) },
        arm64: { url: 'https://github.com/o/alpha/releases/download/v1.2.0/b.deb', sha256: 'b'.repeat(64) },
      },
    },
  },
});

test('schema: 合法 catalog 与仓库里的文件都通过', () => {
  assert.deepEqual(validateCatalog(validCatalog()), []);
  const repo = JSON.parse(readFileSync(join(ROOT, 'runtime-catalog.json'), 'utf8'));
  assert.deepEqual(validateCatalog(repo), []);
  const sources = JSON.parse(readFileSync(join(ROOT, 'tools', 'runtime-catalog.sources.json'), 'utf8'));
  assert.deepEqual(validateSources(sources), []);
  assert.deepEqual(validateSources(SOURCES), []);
});

test('schema: 拒绝 root policy 字段', () => {
  for (const [where, key, value] of [
    ['app', 'install_method', 'apt'],
    ['app', 'install_script', '/opt/x.sh'],
    ['app', 'package', 'code'],
    ['app', 'binary', '/usr/bin/x'],
    ['app', 'shell', 'curl | sh'],
    ['artifact', 'path', '/opt/x'],
    ['artifact', 'target', '/usr/local/bin'],
    ['root', 'install_method', 'apt'],
  ]) {
    const c = validCatalog();
    const obj = where === 'app' ? c.apps.alpha : where === 'artifact' ? c.apps.alpha.artifacts.amd64 : c;
    obj[key] = value;
    const errors = validateCatalog(c);
    assert.ok(errors.some((e) => e.includes(key) && e.includes('root policy')), `${key}: ${errors}`);
  }
});

test('schema: 拒绝坏 sha / http URL / 未知字段 / 坏时间 / 坏 id', () => {
  const cases = [
    (c) => { c.apps.alpha.artifacts.amd64.sha256 = 'A'.repeat(64); },
    (c) => { c.apps.alpha.artifacts.amd64.sha256 = 'a'.repeat(63); },
    (c) => { c.apps.alpha.artifacts.amd64.url = 'http://github.com/x.deb'; },
    (c) => { c.apps.alpha.artifacts.amd64.url = 'file:///etc/passwd'; },
    (c) => { c.apps.alpha.artifacts.amd64.url = 'https://user:pw@github.com/x.deb'; },
    (c) => { c.apps.alpha.artifacts.riscv64 = c.apps.alpha.artifacts.amd64; },
    (c) => { c.apps.alpha.artifacts = {}; },
    (c) => { c.apps.alpha.extra = 1; },
    (c) => { c.apps.alpha.version = '1.0; rm -rf /'; },
    (c) => { c.apps.alpha.released_at = 'yesterday'; },
    (c) => { c.generated_at = '2026-13-45T00:00:00Z'; },
    (c) => { c.schema_version = 2; },
    (c) => { c.apps['../evil'] = c.apps.alpha; },
    (c) => { c.apps.Alpha = c.apps.alpha; },
  ];
  for (const mutate of cases) {
    const c = validCatalog();
    mutate(c);
    assert.notDeepEqual(validateCatalog(c), [], mutate.toString());
  }
});

test('sources: 拒绝非 https 与非法模板', () => {
  assert.notDeepEqual(validateSources({ apps: { x: { type: 'json-version-api', version_url: 'http://a/b', artifacts: { amd64: 'https://a/{version}' } } } }), []);
  assert.notDeepEqual(validateSources({ apps: { x: { type: 'json-version-api', version_url: 'https://a/b', artifacts: { amd64: 'https://a/{arch}' } } } }), []);
  assert.notDeepEqual(validateSources({ apps: { x: { type: 'github-release', repo: 'a/b', assets: { amd64: '../x' } } } }), []);
  assert.notDeepEqual(validateSources({ apps: { x: { type: 'custom-script', script: '/x.sh' } } }), []);
});

test('updater: 首次生成会下载并计算 checksum', async () => {
  const f = fakeFetch(routesFor());
  const r = await updateCatalog({ prev: null, sources: SOURCES, fetchImpl: f, now: NOW });
  assert.deepEqual(r.failures, []);
  assert.deepEqual(r.changed, ['alpha', 'beta']);
  assert.equal(r.downloads, 4);
  assert.equal(r.catalog.apps.alpha.version, '1.2.0');
  assert.equal(r.catalog.apps.alpha.artifacts.amd64.sha256, sha('alpha-amd64-1.2.0'));
  assert.equal(r.catalog.apps.beta.artifacts.arm64.sha256, sha('beta-arm64-0.9.1'));
  assert.equal(r.catalog.apps.beta.released_at, '2026-09-21T00:00:00.000Z');
  assert.equal(r.catalog.generated_at, '2026-09-25T00:00:00Z');
  assert.deepEqual(validateCatalog(r.catalog), []);
});

test('updater: 版本不变时不重复下载，generated_at 不变', async () => {
  const first = await updateCatalog({ prev: null, sources: SOURCES, fetchImpl: fakeFetch(routesFor()), now: NOW });
  const f = fakeFetch(routesFor());
  const later = () => new Date('2026-09-26T00:00:00Z');
  const r = await updateCatalog({ prev: first.catalog, sources: SOURCES, fetchImpl: f, now: later });
  assert.equal(r.downloads, 0);
  assert.deepEqual(f.downloads(), []);
  assert.deepEqual(r.changed, []);
  assert.equal(serializeCatalog(r.catalog), serializeCatalog(first.catalog));
});

test('updater: 版本变化只重新下载变化的 app', async () => {
  const first = await updateCatalog({ prev: null, sources: SOURCES, fetchImpl: fakeFetch(routesFor()), now: NOW });
  const f = fakeFetch(routesFor({ alpha: '1.3.0' }));
  const later = () => new Date('2026-09-26T00:00:00Z');
  const r = await updateCatalog({ prev: first.catalog, sources: SOURCES, fetchImpl: f, now: later });
  assert.deepEqual(r.changed, ['alpha']);
  assert.equal(f.downloads().length, 2);
  assert.ok(f.downloads().every((u) => u.includes('alpha_1.3.0')));
  assert.equal(r.catalog.apps.alpha.version, '1.3.0');
  assert.equal(r.catalog.apps.alpha.artifacts.arm64.sha256, sha('alpha-arm64-1.3.0'));
  assert.deepEqual(r.catalog.apps.beta, first.catalog.apps.beta);
  assert.equal(r.catalog.generated_at, '2026-09-26T00:00:00Z');
});

test('updater: 单 app 失败保留上一版 entry，其它 app 照常更新', async () => {
  const first = await updateCatalog({ prev: null, sources: SOURCES, fetchImpl: fakeFetch(routesFor()), now: NOW });
  const routes = routesFor({ alpha: '1.3.0', beta: '1.0.0' });
  delete routes['https://github.com/o/alpha/releases/download/v1.3.0/alpha_1.3.0_arm64.deb']; // 下载 404
  const r = await updateCatalog({ prev: first.catalog, sources: SOURCES, fetchImpl: fakeFetch(routes), now: NOW });
  assert.deepEqual(r.failures.map((f) => f.id), ['alpha']);
  assert.deepEqual(r.catalog.apps.alpha, first.catalog.apps.alpha);
  assert.equal(r.catalog.apps.beta.version, '1.0.0');
  assert.deepEqual(validateCatalog(r.catalog), []);
});

test('updater: 缺少某个架构的 release 资产算失败；首次失败时不收录', async () => {
  const routes = routesFor();
  routes['https://api.github.com/repos/o/alpha/releases/latest'] = ghRelease('v1.2.0', ['alpha_1.2.0_amd64.deb']);
  const r = await updateCatalog({ prev: null, sources: SOURCES, fetchImpl: fakeFetch(routes), now: NOW });
  assert.deepEqual(r.failures.map((f) => f.id), ['alpha']);
  assert.ok(!('alpha' in r.catalog.apps));
  assert.ok('beta' in r.catalog.apps);
});

test('updater: 版本 API 失败或返回非法版本都保留旧 entry', async () => {
  const first = await updateCatalog({ prev: null, sources: SOURCES, fetchImpl: fakeFetch(routesFor()), now: NOW });
  for (const bad of [{ status: 500 }, { version: '$(id)' }, { nothing: true }]) {
    const routes = routesFor();
    routes['https://example.com/latest.json'] = bad;
    const r = await updateCatalog({ prev: first.catalog, sources: SOURCES, fetchImpl: fakeFetch(routes), now: NOW });
    assert.deepEqual(r.failures.map((f) => f.id), ['beta']);
    assert.deepEqual(r.catalog.apps.beta, first.catalog.apps.beta);
  }
});

test('updater: 被移出 sources 的 app 从 catalog 删除', async () => {
  const first = await updateCatalog({ prev: null, sources: SOURCES, fetchImpl: fakeFetch(routesFor()), now: NOW });
  const r = await updateCatalog({ prev: first.catalog, sources: { apps: { beta: SOURCES.apps.beta } }, fetchImpl: fakeFetch(routesFor()), now: NOW });
  assert.deepEqual(r.removed, ['alpha']);
  assert.deepEqual(Object.keys(r.catalog.apps), ['beta']);
});

test('sha256OfUrl: 拒绝 http、空文件；从不自动跟随重定向', async () => {
  await assert.rejects(sha256OfUrl('http://x/a.deb', { fetchImpl: fakeFetch({}) }), /非 https/);
  await assert.rejects(sha256OfUrl('https://x/a.deb', { fetchImpl: fakeFetch({ 'https://x/a.deb': '' }) }), /空文件/);
  const f = fakeFetch({ 'https://x/a.deb': 'abc' });
  assert.equal(await sha256OfUrl('https://x/a.deb', { fetchImpl: f }), sha('abc'));
  assert.ok(f.calls.every((c) => c.redirect === 'manual'));
});

test('sha256OfUrl: HTTPS 重定向链正常，中间任意一跳降级到 HTTP 都拒绝', async () => {
  const ok = fakeFetch({
    'https://github.com/o/a/releases/download/v1/a.deb': { redirect: 'https://objects.example.com/a.deb?sig=1', status: 302 },
    'https://objects.example.com/a.deb?sig=1': { redirect: '/final/a.deb', status: 307 },
    'https://objects.example.com/final/a.deb': 'payload',
  });
  assert.equal(await sha256OfUrl('https://github.com/o/a/releases/download/v1/a.deb', { fetchImpl: ok }), sha('payload'));
  assert.equal(ok.calls.length, 3);

  // HTTPS → HTTP → HTTPS：最终地址是 https 也必须拒绝，且 http 那一跳根本不会被请求
  const downgrade = fakeFetch({
    'https://x/a.deb': { redirect: 'http://mirror.example.com/a.deb' },
    'http://mirror.example.com/a.deb': { redirect: 'https://x/final.deb' },
    'https://x/final.deb': 'payload',
  });
  await assert.rejects(sha256OfUrl('https://x/a.deb', { fetchImpl: downgrade }), /非 https/);
  assert.deepEqual(downgrade.calls.map((c) => c.url), ['https://x/a.deb']);

  // 带用户名密码的 Location
  await assert.rejects(
    sha256OfUrl('https://x/a.deb', { fetchImpl: fakeFetch({ 'https://x/a.deb': { redirect: 'https://u:p@y/a.deb' } }) }),
    /非 https/,
  );
  // 缺 Location
  await assert.rejects(
    sha256OfUrl('https://x/a.deb', { fetchImpl: fakeFetch({ 'https://x/a.deb': { redirect: '' , status: 301 } }) }),
  );
});

test('sha256OfUrl: 重定向循环与超过上限都拒绝', async () => {
  const loop = fakeFetch({
    'https://x/a': { redirect: 'https://x/b' },
    'https://x/b': { redirect: 'https://x/a' },
  });
  await assert.rejects(sha256OfUrl('https://x/a', { fetchImpl: loop }), /循环/);

  const chain = {};
  for (let i = 0; i < 20; i++) chain[`https://x/${i}`] = { redirect: `https://x/${i + 1}` };
  chain['https://x/20'] = 'payload';
  await assert.rejects(sha256OfUrl('https://x/0', { fetchImpl: fakeFetch(chain), maxRedirects: 8 }), /超过 8 跳/);
  // 恰好在上限内的链可以成功
  const short = {};
  for (let i = 0; i < 8; i++) short[`https://x/${i}`] = { redirect: `https://x/${i + 1}` };
  short['https://x/8'] = 'payload';
  assert.equal(await sha256OfUrl('https://x/0', { fetchImpl: fakeFetch(short), maxRedirects: 8 }), sha('payload'));
});

test('GitHub token 只发给 api.github.com，artifact 下载及其重定向从不携带 Authorization', async () => {
  const routes = routesFor();
  const gh = 'https://github.com/o/alpha/releases/download/v1.2.0/';
  // 让 artifact 经 CDN 重定向；再让 API 自身重定向到别的主机，token 也不能跟过去
  routes[`${gh}alpha_1.2.0_amd64.deb`] = { redirect: 'https://objects.githubusercontent.com/amd64.deb' };
  routes['https://objects.githubusercontent.com/amd64.deb'] = 'alpha-amd64-1.2.0';
  const f = fakeFetch(routes);
  const r = await updateCatalog({ prev: null, sources: SOURCES, fetchImpl: f, githubToken: 'SECRET', now: NOW });
  assert.deepEqual(r.failures, []);
  for (const c of f.calls) {
    const isApi = new URL(c.url).hostname === 'api.github.com';
    assert.equal(Boolean(c.headers.Authorization), isApi, c.url);
  }
  assert.ok(f.calls.some((c) => c.headers.Authorization === 'Bearer SECRET'));

  const moved = fakeFetch({
    'https://api.github.com/repos/o/alpha/releases/latest': { redirect: 'https://evil.example.com/latest', status: 301 },
    'https://evil.example.com/latest': ghRelease('v1.2.0', []),
  });
  await updateCatalog({ prev: null, sources: { apps: { alpha: SOURCES.apps.alpha } }, fetchImpl: moved, githubToken: 'SECRET', now: NOW });
  const evilCall = moved.calls.find((c) => c.url.startsWith('https://evil.example.com'));
  assert.ok(evilCall && !evilCall.headers.Authorization);
});

// ─── JetBrains / Cursor 官方 API adapter ─────────────────────────────

const JB_PREFIXES = ['https://download.jetbrains.com/', 'https://download-cdn.jetbrains.com/'];
const JB_SOURCES = { apps: { intellij: { type: 'jetbrains-release', code: 'IIC', url_prefixes: JB_PREFIXES } } };
const JB_API = 'https://data.services.jetbrains.com/products/releases?code=IIC&latest=true&type=release';

/**
 * JetBrains 假 API。artifact 本体路由故意存在：用来证明 updater 从不请求它们。
 * opts: arm=false 去掉 linuxARM64；noChecksum 去掉 x64 的 checksumLink；checksumBody 覆盖 x64 .sha256 内容；
 *       checksumHost 让 checksumLink 指向别的主机；linkHost 让下载地址指向别的主机。
 */
function jbRoutes(version = '2025.3', {
  arm = true, noChecksum = false, checksumBody, checksumHost, linkHost = 'https://download.jetbrains.com',
} = {}) {
  const x64 = `${linkHost}/idea/idea-${version}.tar.gz`;
  const a64 = `${linkHost}/idea/idea-${version}-aarch64.tar.gz`;
  const x64Sum = `${checksumHost ?? linkHost}/idea/idea-${version}.tar.gz.sha256`;
  const a64Sum = `${linkHost}/idea/idea-${version}-aarch64.tar.gz.sha256`;
  const downloads = { linux: noChecksum ? { link: x64 } : { link: x64, checksumLink: x64Sum } };
  if (arm) downloads.linuxARM64 = { link: a64, checksumLink: a64Sum };
  return {
    [JB_API]: { IIC: [{ version, build: '253.1', date: '2025-12-08', downloads }] },
    [x64]: `idea-x64-${version}`,
    [a64]: `idea-arm-${version}`,
    [x64Sum]: checksumBody ?? `${sha(`idea-x64-${version}`)} *idea-${version}.tar.gz\n`,
    [a64Sum]: `${sha(`idea-arm-${version}`)} *idea-${version}-aarch64.tar.gz\n`,
  };
}

const CURSOR_SOURCES = {
  apps: {
    cursor: {
      type: 'cursor-api',
      endpoint: 'https://api2.cursor.sh/updates/api/download/stable/{platform}/cursor',
      platforms: { amd64: 'linux-x64', arm64: 'linux-arm64' },
      url_prefixes: ['https://downloads.cursor.com/'],
    },
  },
};
const CURSOR_API = (p) => `https://api2.cursor.sh/updates/api/download/stable/${p}/cursor`;

function cursorRoutes(version = '3.22.7', { armVersion = version, armUrl, x64Url } = {}) {
  const x64 = x64Url ?? `https://downloads.cursor.com/production/abc/linux/x64/Cursor-${version}-x86_64.AppImage`;
  const a64 = armUrl ?? `https://downloads.cursor.com/production/abc/linux/arm64/Cursor-${armVersion}-aarch64.AppImage`;
  return {
    [CURSOR_API('linux-x64')]: { version, downloadUrl: x64, commitSha: 'abc' },
    [CURSOR_API('linux-arm64')]: armUrl === null ? { version: armVersion } : { version: armVersion, downloadUrl: a64 },
    [x64]: `cursor-x64-${version}`,
    [a64]: `cursor-arm-${armVersion}`,
  };
}

test('jetbrains: 解析版本/发布日期，按 linux / linuxARM64 选资产，SHA256 直接取官方 checksumLink', async () => {
  const f = fakeFetch(jbRoutes('2025.3'));
  const r = await updateCatalog({ prev: null, sources: JB_SOURCES, fetchImpl: f, now: NOW });
  assert.deepEqual(r.failures, []);
  const e = r.catalog.apps.intellij;
  assert.equal(e.version, '2025.3');
  assert.equal(e.released_at, '2025-12-08T00:00:00Z');
  assert.equal(e.artifacts.amd64.url, 'https://download.jetbrains.com/idea/idea-2025.3.tar.gz');
  assert.equal(e.artifacts.arm64.url, 'https://download.jetbrains.com/idea/idea-2025.3-aarch64.tar.gz');
  assert.equal(e.artifacts.amd64.sha256, sha('idea-x64-2025.3'));
  assert.equal(e.artifacts.arm64.sha256, sha('idea-arm-2025.3'));
  assert.deepEqual(validateCatalog(r.catalog), []);
  // 只请求 releases API + 两个 .sha256，从不请求 tar.gz 本体
  assert.equal(r.downloads, 0);
  assert.deepEqual(f.calls.map((c) => c.url).sort(), [
    JB_API,
    'https://download.jetbrains.com/idea/idea-2025.3-aarch64.tar.gz.sha256',
    'https://download.jetbrains.com/idea/idea-2025.3.tar.gz.sha256',
  ].sort());
  assert.deepEqual(f.downloads(), []);

  // 版本不变：只查 API，连 .sha256 都不取
  const again = fakeFetch(jbRoutes('2025.3'));
  const r2 = await updateCatalog({ prev: r.catalog, sources: JB_SOURCES, fetchImpl: again, now: NOW });
  assert.equal(r2.downloads, 0);
  assert.deepEqual(again.calls.map((c) => c.url), [JB_API]);

  // 版本变化：换成新版本的官方校验和，仍不下载本体
  const bump = fakeFetch(jbRoutes('2026.1'));
  const r3 = await updateCatalog({ prev: r.catalog, sources: JB_SOURCES, fetchImpl: bump, now: NOW });
  assert.deepEqual(r3.changed, ['intellij']);
  assert.equal(r3.catalog.apps.intellij.artifacts.arm64.sha256, sha('idea-arm-2026.1'));
  assert.deepEqual(bump.downloads(), []);
});

test('jetbrains: 缺资产 / checksum 缺失、非法、文件名不符、越界 → 失败并保留上一版，且不下载本体', async () => {
  const first = await updateCatalog({ prev: null, sources: JB_SOURCES, fetchImpl: fakeFetch(jbRoutes('2025.3')), now: NOW });
  const cases = {
    'no arm64': jbRoutes('2026.1', { arm: false }),
    'checksumLink missing': jbRoutes('2026.1', { noChecksum: true }),
    'checksum not hex': jbRoutes('2026.1', { checksumBody: 'not-a-checksum idea-2026.1.tar.gz\n' }),
    'checksum short': jbRoutes('2026.1', { checksumBody: `${'a'.repeat(63)} *idea-2026.1.tar.gz\n` }),
    'checksum for other file': jbRoutes('2026.1', { checksumBody: `${'a'.repeat(64)} *idea-2025.3.tar.gz\n` }),
    'checksum 404': (() => { const r = jbRoutes('2026.1'); delete r['https://download.jetbrains.com/idea/idea-2026.1.tar.gz.sha256']; return r; })(),
    'checksumLink foreign host': jbRoutes('2026.1', { checksumHost: 'https://evil.example.com' }),
    'download link foreign host': jbRoutes('2026.1', { linkHost: 'https://evil.example.com' }),
  };
  for (const [name, routes] of Object.entries(cases)) {
    const f = fakeFetch(routes);
    const r = await updateCatalog({ prev: first.catalog, sources: JB_SOURCES, fetchImpl: f, now: NOW });
    assert.deepEqual(r.failures.map((x) => x.id), ['intellij'], name);
    assert.deepEqual(r.catalog.apps.intellij, first.catalog.apps.intellij, name);
    assert.deepEqual(f.downloads(), [], `${name}: 不应下载 artifact 本体`);
    assert.ok(!f.calls.some((c) => c.url.startsWith('https://evil.example.com')), `${name}: 不应请求越界地址`);
  }
});

test('cursor: 每个平台查官方 API，版本一致且 downloadUrl 在 downloads.cursor.com 下', async () => {
  const f = fakeFetch(cursorRoutes('3.22.7'));
  const r = await updateCatalog({ prev: null, sources: CURSOR_SOURCES, fetchImpl: f, now: NOW });
  assert.deepEqual(r.failures, []);
  const e = r.catalog.apps.cursor;
  assert.equal(e.version, '3.22.7');
  assert.equal(e.released_at, undefined);
  assert.match(e.artifacts.amd64.url, /^https:\/\/downloads\.cursor\.com\/.*Cursor-3\.22\.7-x86_64\.AppImage$/);
  assert.match(e.artifacts.arm64.url, /^https:\/\/downloads\.cursor\.com\/.*Cursor-3\.22\.7-aarch64\.AppImage$/);
  assert.equal(e.artifacts.arm64.sha256, sha('cursor-arm-3.22.7'));

  const again = fakeFetch(cursorRoutes('3.22.7'));
  const r2 = await updateCatalog({ prev: r.catalog, sources: CURSOR_SOURCES, fetchImpl: again, now: NOW });
  assert.equal(r2.downloads, 0);
  assert.deepEqual(again.downloads(), []);

  // 版本变化：只下载新版本的两个资产
  const bump = fakeFetch(cursorRoutes('3.23.0'));
  const r3 = await updateCatalog({ prev: r.catalog, sources: CURSOR_SOURCES, fetchImpl: bump, now: NOW });
  assert.deepEqual(r3.changed, ['cursor']);
  assert.equal(bump.downloads().length, 2);
  assert.equal(r3.catalog.apps.cursor.version, '3.23.0');
});

test('cursor: 缺 arm64 下载 / 架构版本不一致 / 越出前缀 / 资产名与版本不符 → 失败并保留上一版', async () => {
  const first = await updateCatalog({ prev: null, sources: CURSOR_SOURCES, fetchImpl: fakeFetch(cursorRoutes('3.22.7')), now: NOW });
  const cases = {
    'no arm64 downloadUrl': cursorRoutes('3.23.0', { armUrl: null }),
    'arch version mismatch': cursorRoutes('3.23.0', { armVersion: '3.22.9' }),
    'foreign host': cursorRoutes('3.23.0', { x64Url: 'https://cursor.evil.example.com/Cursor-3.23.0-x86_64.AppImage' }),
    'host prefix trick': cursorRoutes('3.23.0', { x64Url: 'https://downloads.cursor.com.evil.example/Cursor-3.23.0-x86_64.AppImage' }),
    'name/version mismatch': cursorRoutes('3.23.0', { x64Url: 'https://downloads.cursor.com/production/x/linux/x64/Cursor-3.1.0-x86_64.AppImage' }),
  };
  for (const [name, routes] of Object.entries(cases)) {
    const f = fakeFetch(routes);
    const r = await updateCatalog({ prev: first.catalog, sources: CURSOR_SOURCES, fetchImpl: f, now: NOW });
    assert.deepEqual(r.failures.map((x) => x.id), ['cursor'], name);
    assert.deepEqual(r.catalog.apps.cursor, first.catalog.apps.cursor, name);
    assert.deepEqual(f.downloads(), [], `${name}: 不应下载任何资产`);
  }
});

test('单个 adapter 失败不影响其它 app（混合来源）', async () => {
  const sources = { apps: { ...SOURCES.apps, ...JB_SOURCES.apps, ...CURSOR_SOURCES.apps } };
  const routes = { ...routesFor(), ...jbRoutes('2025.3'), ...cursorRoutes('3.22.7') };
  const first = await updateCatalog({ prev: null, sources, fetchImpl: fakeFetch(routes), now: NOW });
  assert.deepEqual(first.failures, []);
  const broken = { ...routesFor({ alpha: '1.3.0' }), ...jbRoutes('2026.1', { arm: false }), ...cursorRoutes('3.22.7') };
  const r = await updateCatalog({ prev: first.catalog, sources, fetchImpl: fakeFetch(broken), now: NOW });
  assert.deepEqual(r.failures.map((x) => x.id), ['intellij']);
  assert.deepEqual(r.catalog.apps.intellij, first.catalog.apps.intellij);
  assert.equal(r.catalog.apps.alpha.version, '1.3.0');
  assert.deepEqual(r.catalog.apps.cursor, first.catalog.apps.cursor);
});

test('sources: 新 adapter 的配置校验', () => {
  const ok = (src) => validateSources({ apps: { x: src } });
  assert.deepEqual(ok(JB_SOURCES.apps.intellij), []);
  assert.deepEqual(ok(CURSOR_SOURCES.apps.cursor), []);
  for (const bad of [
    { ...JB_SOURCES.apps.intellij, code: 'iic' },
    { ...JB_SOURCES.apps.intellij, code: 'IIC&x=1' },
    { ...JB_SOURCES.apps.intellij, url_prefixes: [] },
    { ...JB_SOURCES.apps.intellij, url_prefixes: ['https://download.jetbrains.com'] },
    { ...JB_SOURCES.apps.intellij, url_prefixes: ['http://download.jetbrains.com/'] },
    { ...JB_SOURCES.apps.intellij, install_method: 'direct_download' },
    { ...CURSOR_SOURCES.apps.cursor, endpoint: 'https://api2.cursor.sh/updates/{arch}' },
    { ...CURSOR_SOURCES.apps.cursor, endpoint: 'http://api2.cursor.sh/{platform}' },
    { ...CURSOR_SOURCES.apps.cursor, platforms: { riscv64: 'linux-riscv' } },
    { ...CURSOR_SOURCES.apps.cursor, platforms: { amd64: '../x' } },
  ]) {
    assert.notDeepEqual(ok(bad), [], JSON.stringify(bad));
  }
});

test('writeAtomic: 替换文件且不留临时文件', () => {
  const dir = mkdtempSync(join(tmpdir(), 'catalog-'));
  const path = join(dir, 'runtime-catalog.json');
  writeFileSync(path, 'old');
  writeAtomic(path, 'new');
  assert.equal(readFileSync(path, 'utf8'), 'new');
  assert.deepEqual(readdirSync(dir), ['runtime-catalog.json']);
});

test('CLI --validate: 离线校验通过；坏 catalog 退出码 3', () => {
  const cli = join(ROOT, 'tools', 'update-runtime-catalog.mjs');
  const ok = spawnSync(process.execPath, [cli, '--validate'], { encoding: 'utf8' });
  assert.equal(ok.status, 0, ok.stderr);
  const dir = mkdtempSync(join(tmpdir(), 'catalog-'));
  const bad = validCatalog();
  bad.apps.alpha.install_script = '/opt/x.sh';
  writeFileSync(join(dir, 'c.json'), JSON.stringify(bad));
  const r = spawnSync(process.execPath, [cli, '--validate', '--catalog', join(dir, 'c.json')], { encoding: 'utf8' });
  assert.equal(r.status, 3);
  assert.match(r.stderr, /install_script/);
});
