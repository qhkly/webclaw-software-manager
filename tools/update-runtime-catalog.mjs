#!/usr/bin/env node
// 更新 runtime-catalog.json。
//
//   node tools/update-runtime-catalog.mjs              查询最新版本，有变化才下载算 SHA256，原子写回
//   node tools/update-runtime-catalog.mjs --dry-run    同上但不写文件，打印结果
//   node tools/update-runtime-catalog.mjs --check      同 --dry-run；有待更新内容时退出码 1（CI 用）
//   node tools/update-runtime-catalog.mjs --validate   只做离线校验（catalog + sources），不联网
//   [--catalog <path>] [--sources <path>]
//
// 退出码：0 正常 / 1 --check 发现有变化 / 2 部分 app 失败（已保留 last-known-good）/ 3 致命错误
// GITHUB_TOKEN 若存在，只用于 api.github.com 的元数据查询（避免限流）。
// 在 GitHub Actions 里会把 changed / failed 写进 $GITHUB_OUTPUT。

import { appendFileSync, readFileSync, renameSync, writeFileSync, openSync, fsyncSync, closeSync, unlinkSync } from 'node:fs';
import { dirname, resolve, basename, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { serializeCatalog, updateCatalog, validateCatalog, validateSources } from './lib/runtime-catalog.mjs';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function parseArgs(argv) {
  const opts = {
    catalog: join(ROOT, 'runtime-catalog.json'),
    sources: join(ROOT, 'tools', 'runtime-catalog.sources.json'),
    mode: 'write',
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--dry-run') opts.mode = 'dry-run';
    else if (a === '--check') opts.mode = 'check';
    else if (a === '--validate') opts.mode = 'validate';
    else if (a === '--catalog') opts.catalog = resolve(argv[++i]);
    else if (a === '--sources') opts.sources = resolve(argv[++i]);
    else throw new Error(`未知参数：${a}`);
  }
  return opts;
}

function readJson(path, { optional = false } = {}) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch (error) {
    if (optional && error.code === 'ENOENT') return null;
    throw new Error(`读取 ${path} 失败：${error.message}`);
  }
}

/** 同目录临时文件 + fsync + rename：写到一半崩溃也不会留下半个 catalog。 */
export function writeAtomic(path, content) {
  const tmp = join(dirname(path), `.${basename(path)}.${process.pid}.tmp`);
  try {
    writeFileSync(tmp, content, { mode: 0o644 });
    const fd = openSync(tmp, 'r');
    try { fsyncSync(fd); } finally { closeSync(fd); }
    renameSync(tmp, path);
  } catch (error) {
    try { unlinkSync(tmp); } catch { /* 已经不存在 */ }
    throw error;
  }
}

function setOutput(key, value) {
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `${key}=${value}\n`);
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const log = (msg) => console.error(`[runtime-catalog] ${msg}`);

  const sources = readJson(opts.sources);
  const sourceErrors = validateSources(sources);
  if (sourceErrors.length) throw new Error(`sources 不合法：\n${sourceErrors.join('\n')}`);

  const prev = readJson(opts.catalog, { optional: true });
  if (prev) {
    const errors = validateCatalog(prev);
    if (errors.length) throw new Error(`现有 ${opts.catalog} 不合法：\n${errors.join('\n')}`);
  }
  if (opts.mode === 'validate') {
    if (!prev) throw new Error(`${opts.catalog} 不存在`);
    log(`校验通过：${Object.keys(prev.apps).length} 个 app，${Object.keys(sources.apps).length} 个来源`);
    return 0;
  }

  const result = await updateCatalog({ prev, sources, githubToken: process.env.GITHUB_TOKEN, log });
  const next = serializeCatalog(result.catalog);
  const dirty = !prev || next !== serializeCatalog(prev);

  log(`变化：${result.changed.join(', ') || '无'}；移除：${result.removed.join(', ') || '无'}；下载 ${result.downloads} 个 artifact`);
  for (const f of result.failures) log(`失败：${f.id}：${f.error}`);
  setOutput('changed', dirty ? 'true' : 'false');
  setOutput('failed', result.failures.map((f) => f.id).join(','));

  if (opts.mode === 'write') {
    if (dirty) {
      writeAtomic(opts.catalog, next);
      log(`已写入 ${opts.catalog}`);
    }
  } else {
    process.stdout.write(next);
  }
  if (result.failures.length) return 2;
  if (opts.mode === 'check' && dirty) return 1;
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then(
    (code) => process.exit(code),
    (error) => {
      console.error(`[runtime-catalog] 致命错误：${error.message}`);
      process.exit(3);
    },
  );
}
