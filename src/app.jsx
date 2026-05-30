// app.jsx — WebClaw 软件商店 主应用

const TWEAK_DEFAULTS = {
  accent: '#D4623A',
  density: 'regular',
  dark: false,
};

const tauriInvoke = (...args) => window.__TAURI__?.core?.invoke(...args);
const tauriListen = (...args) => window.__TAURI__?.event?.listen(...args);

const toIconUrl = (path) => {
  if (!path) return null;
  if (path.startsWith('data:')) return path;
  try { return window.__TAURI__.core.convertFileSrc(path); }
  catch { return null; }
};
const withIconUrl = (item) => ({ ...item, iconUrl: toIconUrl(item.icon) });

function App() {
  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);
  const [platform, setPlatform] = React.useState(null);
  const [manifestSource, setManifestSource] = React.useState('');
  const [items, setItems] = React.useState([]);
  const [filter, setFilter] = React.useState('all');
  const [q, setQ] = React.useState('');
  const [sort, setSort] = React.useState('state');
  const [selected, setSelected] = React.useState(new Set());
  const [modal, setModal] = React.useState(null);
  const [actionState, setActionState] = React.useState(null);
  const [log, setLog] = React.useState(window.INITIAL_LOG);
  const [logOpen, setLogOpen] = React.useState(false);
  const [scanning, setScanning] = React.useState(false);
  const [scriptsState, setScriptsState] = React.useState(null); // null|'updating'|'updated'|'warn'
  const [lastScan, setLastScan] = React.useState('');
  const [inited, setInited] = React.useState(false);

  const addLog = React.useCallback((msg, tone = 'dim') => {
    const ts = new Date().toLocaleTimeString('en-GB', { hour12: false });
    setLog(L => [...L, { ts, msg, tone }]);
  }, []);

  const doScan = React.useCallback(async (platformKey) => {
    if (!tauriInvoke || !platformKey) return;
    setScanning(true);
    addLog(`扫描 ${platformKey} 平台软件清单...`, 'dim');
    try {
      const catalog = await tauriInvoke('get_platform_catalog', { platform: platformKey });
      setItems(catalog.map(withIconUrl));
      const checked = await tauriInvoke('check_latest', { platform: platformKey });
      const checkedMap = Object.fromEntries(checked.map(i => [i.id, i]));
      setItems(catalog.map(i => withIconUrl(checkedMap[i.id] || i)));
      const ts = new Date().toLocaleTimeString('en-GB', { hour12: false });
      setLastScan(ts);
      const instCount = checked.filter(i => i.state !== 'not_installed').length;
      const upgCount = checked.filter(i => i.state === 'upgradable').length;
      addLog(`扫描完成：共 ${catalog.length} 项，已安装 ${instCount} 项，可升级 ${upgCount} 项`, 'ok');
    } catch (e) {
      addLog(`扫描失败：${stringifyError(e)}`, 'err');
    } finally {
      setScanning(false);
    }
  }, [addLog]);

  React.useEffect(() => {
    if (!tauriInvoke) return;
    async function init() {
      try {
        const info = await tauriInvoke('detect_platform');
        setPlatform(info);
        addLog(`平台：${info.label}（${info.os} / ${info.arch}）`, 'dim');
        const src = await tauriInvoke('refresh_manifest');
        setManifestSource(src);
        addLog(`软件清单来源：${{ remote: '远端', cache: '缓存', bundled: '内置' }[src] || src}`, 'dim');
        if (info.in_container) {
          setScriptsState('updating');
          tauriInvoke('refresh_scripts').then(r => {
            if (r === 'updated') {
              setScriptsState('updated');
              addLog('安装脚本已从远端更新', 'ok');
              setTimeout(() => setScriptsState(null), 3000);
            } else if (r && r.startsWith('warn:')) {
              setScriptsState('warn');
              addLog(`安装脚本更新失败：${r.slice(5)}`, 'dim');
            } else {
              setScriptsState(null);
            }
          }).catch(() => setScriptsState('warn'));
        }
        await doScan(info.key);
      } catch (e) {
        addLog(`初始化失败：${stringifyError(e)}`, 'err');
      } finally {
        setInited(true);
      }
    }
    init();
  }, [addLog, doScan]);

  const onRescan = React.useCallback(() => {
    if (platform?.key) doScan(platform.key);
  }, [platform, doScan]);

  const counts = React.useMemo(() => ({
    total: items.length,
    not_installed: items.filter(i => i.state === 'not_installed').length,
    upgradable: items.filter(i => i.state === 'upgradable').length,
    up_to_date: items.filter(i => i.state === 'up_to_date').length,
    unknown: items.filter(i => i.state === 'unknown').length,
  }), [items]);

  const visible = React.useMemo(() => {
    let v = items.slice();
    if (filter === 'not_installed') v = v.filter(i => i.state === 'not_installed');
    else if (filter === 'upgradable') v = v.filter(i => i.state === 'upgradable');
    else if (filter === 'up_to_date') v = v.filter(i => i.state === 'up_to_date');
    else if (filter === 'unknown') v = v.filter(i => i.state === 'unknown');

    if (q.trim()) {
      const k = q.trim().toLowerCase();
      v = v.filter(i =>
        i.name.toLowerCase().includes(k) ||
        i.category.toLowerCase().includes(k) ||
        i.group.toLowerCase().includes(k) ||
        i.desc.toLowerCase().includes(k) ||
        (i.installed_version || '').toLowerCase().includes(k) ||
        (i.latest_version || '').toLowerCase().includes(k)
      );
    }

    const stateRank = { not_installed: 0, upgradable: 1, unknown: 2, up_to_date: 3 };
    if (sort === 'state') v.sort((a, b) => ((stateRank[a.state] ?? 4) - (stateRank[b.state] ?? 4)) || a.name.localeCompare(b.name));
    else if (sort === 'name') v.sort((a, b) => a.name.localeCompare(b.name));
    else if (sort === 'category') v.sort((a, b) => a.category.localeCompare(b.category) || a.name.localeCompare(b.name));
    else if (sort === 'group') v.sort((a, b) => a.group.localeCompare(b.group) || a.name.localeCompare(b.name));
    return v;
  }, [items, filter, q, sort]);

  const onInstall = (item) => {
    setActionState(null);
    setModal({ item, action: 'install' });
  };

  const onUpgrade = (item) => {
    setActionState(null);
    setModal({ item, action: 'upgrade' });
  };

  const onRecheck = React.useCallback(() => {
    if (platform?.key) doScan(platform.key);
  }, [platform, doScan]);

  const onSelect = (id) => {
    setSelected(prev => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  };

  const onBatchUpgrade = () => {
    if (!selected.size) return;
    const batch = items.filter(i => selected.has(i.id) && i.state === 'upgradable');
    if (!batch.length) return;
    setActionState(null);
    setModal({ batch, action: 'upgrade' });
  };

  const onConfirmAction = async (targets, action) => {
    if (!tauriInvoke || !tauriListen || !platform) {
      addLog('Tauri IPC 不可用', 'err');
      return;
    }
    const command = action === 'install' ? 'install_software' : 'upgrade_software';
    setActionState({ phase: 'running', percent: 0, lines: [], stage: 'STARTING' });

    const unlisten = await tauriListen('software-progress', ev => {
      const { stage, percent, line } = ev.payload || {};
      setActionState(prev => ({
        ...prev,
        stage: String(stage || prev?.stage || '').toUpperCase(),
        percent: percent != null ? percent : (prev?.percent ?? 0),
        lines: line ? [...(prev?.lines || []), line] : (prev?.lines || []),
        phase: ['done', 'error'].includes(String(stage).toLowerCase()) ? 'done' : 'running',
      }));
    });

    const actionLabel = action === 'install' ? '安装' : '升级';
    let lastError = null;
    try {
      for (const target of targets) {
        addLog(`开始${actionLabel} ${target.name}...`, 'up');
        await tauriInvoke(command, { id: target.id, platform: platform.key });
        addLog(`${target.name} ${actionLabel}完成`, 'ok');
        setItems(prev => prev.map(i =>
          i.id === target.id
            ? { ...i, state: 'up_to_date', installed_version: i.latest_version ?? i.installed_version }
            : i
        ));
      }
      setSelected(new Set());
      setActionState(prev => ({ ...prev, phase: 'done', stage: 'DONE', percent: 100 }));
    } catch (e) {
      lastError = stringifyError(e);
      addLog(`${actionLabel}失败：${lastError}`, 'err');
      setActionState(prev => ({
        ...prev,
        phase: 'done',
        stage: 'ERROR',
        lines: [...(prev?.lines || []), lastError],
      }));
    } finally {
      unlisten();
    }
  };

  React.useEffect(() => {
    document.documentElement.setAttribute('data-theme', t.dark ? 'dark' : 'light');
    document.documentElement.style.setProperty('--accent', t.accent);
    document.documentElement.style.setProperty('--accent-2', shade(t.accent, -18));
    document.documentElement.style.setProperty('--accent-tint', tint(t.accent, t.dark));
  }, [t.dark, t.accent]);

  const platformLabel = platform?.label || '软件商店';
  const upgradable = items.filter(i => i.state === 'upgradable');

  return (
    <div className="app">
      <div className="sticky-header">
        <Header
          platformLabel={platformLabel}
          manifestSource={manifestSource}
          counts={counts}
          lastScan={lastScan}
          scanning={scanning || !inited}
          onScan={onRescan}
          dark={t.dark}
          onToggleDark={() => setTweak('dark', !t.dark)}
          onOpenTweaks={() => window.postMessage({ type: '__activate_edit_mode' }, '*')}
          scriptsState={scriptsState}
        />

        <Stats counts={counts} active={filter} onPick={(k) => setFilter(k === filter ? 'all' : k)}/>
        <Toolbar filter={filter} onFilter={setFilter} counts={counts} q={q} onQ={setQ} sort={sort} onSort={setSort}/>
      </div>

      {selected.size > 0 && upgradable.some(i => selected.has(i.id)) && (
        <BatchBar
          count={[...selected].filter(id => items.find(i => i.id === id && i.state === 'upgradable')).length}
          onClear={() => setSelected(new Set())}
          onUpgrade={onBatchUpgrade}
          onRecheck={onRecheck}
        />
      )}

      <div className={`grid density-${t.density}`}>
        {visible.map(i => (
          <Card
            key={i.id}
            item={i}
            selected={selected.has(i.id)}
            onSelect={onSelect}
            onInstall={onInstall}
            onUpgrade={onUpgrade}
            onRecheck={onRecheck}
            dense={t.density === 'compact'}
          />
        ))}
        {visible.length === 0 && inited && (
          <div className="empty-panel">没有匹配的软件项，试试调整筛选或搜索词</div>
        )}
        {!inited && (
          <div className="empty-panel">正在初始化...</div>
        )}
      </div>

      <LogBar entries={log} open={logOpen} onToggle={() => setLogOpen(o => !o)}/>

      {modal && (
        <ActionModal
          item={modal.item}
          batch={modal.batch}
          action={modal.action}
          actionState={actionState}
          onClose={() => { setModal(null); setActionState(null); }}
          onConfirm={onConfirmAction}
        />
      )}

      <TweaksPanel>
        <TweakSection label="主题"/>
        <TweakColor label="主色" value={t.accent}
                    options={['#D4623A', '#3F6E91', '#5C8A4A', '#7A5AE0']}
                    onChange={(v) => setTweak('accent', v)}/>
        <TweakToggle label="深色模式" value={t.dark} onChange={(v) => setTweak('dark', v)}/>
        <TweakSection label="布局"/>
        <TweakRadio label="卡片密度" value={t.density}
                    options={['compact', 'regular', 'comfy']}
                    onChange={(v) => setTweak('density', v)}/>
      </TweaksPanel>
    </div>
  );
}

function stringifyError(e) {
  if (typeof e === 'string') return e;
  if (e?.message) return e.message;
  try { return JSON.stringify(e); } catch (_) { return String(e); }
}

function hexToRgb(h) {
  const m = h.replace('#', '').match(/.{2}/g);
  return m ? m.map(x => parseInt(x, 16)) : [0, 0, 0];
}
function rgbToHex(r, g, b) {
  return '#' + [r, g, b].map(v => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, '0')).join('');
}
function shade(hex, pct) {
  const [r, g, b] = hexToRgb(hex);
  const f = pct / 100;
  return rgbToHex(r + (f < 0 ? r * f : (255 - r) * f),
                  g + (f < 0 ? g * f : (255 - g) * f),
                  b + (f < 0 ? b * f : (255 - b) * f));
}
function tint(hex, dark) {
  const [r, g, b] = hexToRgb(hex);
  if (dark) return `rgb(${Math.round(r * 0.35)},${Math.round(g * 0.35)},${Math.round(b * 0.35)})`;
  return `rgb(${Math.round(r + (255 - r) * 0.78)},${Math.round(g + (255 - g) * 0.78)},${Math.round(b + (255 - b) * 0.78)})`;
}

ReactDOM.createRoot(document.getElementById('root')).render(<App/>);
