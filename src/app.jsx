// app.jsx — WebClaw 软件商店主应用

const TWEAK_DEFAULTS = {
  accent: '#0A6CFF',
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
const withCatalogOrder = (item, order) => ({ ...item, _order: order });

const GROUP_META = {
  ai: { label: 'AI 编程助手', sub: '终端、代理与智能开发工具', icon: 'terminal' },
  dev: { label: '开发工具', sub: '编辑器、IDE 与工程工具', icon: 'fileText' },
  social: { label: '社交通讯', sub: '桌面沟通与协作应用', icon: 'store' },
  media: { label: '音视频工具', sub: '录制、剪辑与媒体处理', icon: 'scan' },
  graphics: { label: '图形设计', sub: '图片编辑、截图与视觉工具', icon: 'settings' },
  network: { label: '网络工具', sub: '网络分析、安全与连接工具', icon: 'refresh' },
  office: { label: '办公效率', sub: '文档、笔记与生产力工具', icon: 'fileText' },
};

function itemGlyph(item) {
  const words = String(item.name || '?').replace(/[^a-zA-Z0-9\u4e00-\u9fa5 ]/g, ' ').trim().split(/\s+/);
  if (!words.length) return '?';
  const ascii = words.filter(w => /[a-zA-Z0-9]/.test(w));
  if (ascii.length >= 2) return (ascii[0][0] + ascii[1][0]).toUpperCase();
  return (ascii[0] || words[0]).slice(0, 2).toUpperCase();
}

function mapState(state) {
  if (state === 'not_installed') return 'installable';
  if (state === 'up_to_date') return 'uptodate';
  if (state === 'upgradable') return 'upgradable';
  return 'checking';
}

function nowTime() {
  return new Date().toLocaleTimeString('en-GB', { hour12: false });
}

function ClawMark({ s = 22 }) {
  return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="none">
      <path d="M7 5.5c2.5 2 3.4 5 3 9.5" stroke="#fff" strokeWidth="2" strokeLinecap="round"/>
      <path d="M12 4.5c2.4 2.6 3 6 2 10.5" stroke="#fff" strokeWidth="2" strokeLinecap="round" opacity=".85"/>
      <path d="M17 5.5c1.9 2.6 2 5.8.6 9.6" stroke="#fff" strokeWidth="2" strokeLinecap="round" opacity=".7"/>
      <circle cx="12" cy="18.5" r="1.5" fill="#fff"/>
    </svg>
  );
}

function MiniIcon({ name, size = 14, stroke = 1.6 }) {
  return <Icon name={name} size={size} stroke={stroke}/>;
}

function StoreIcon({ item, size }) {
  const [imgErr, setImgErr] = React.useState(false);
  const style = size ? { width: size, height: size } : undefined;
  return (
    <div className="app-icon" style={style}>
      {item.iconUrl && !imgErr
        ? <img className="app-icon-img" src={item.iconUrl} alt="" onError={() => setImgErr(true)}/>
        : <span className="mono-glyph">{itemGlyph(item)}</span>}
    </div>
  );
}

function StoreButton({ item, onInstall, onUpgrade, onRecheck }) {
  if (item.state === 'not_installed') {
    return <button className="pill-btn" onClick={() => onInstall(item)}>获取</button>;
  }
  if (item.state === 'upgradable') {
    return <button className="pill-btn" onClick={() => onUpgrade(item)}>更新</button>;
  }
  if (item.state === 'up_to_date') {
    return <button className="pill-btn open" disabled>最新</button>;
  }
  return <button className="pill-btn disabled" onClick={() => onRecheck(item.id)}>检测</button>;
}

function StoreCard({ item, selected, onSelect, onInstall, onUpgrade, onRecheck }) {
  const state = mapState(item.state);
  let meta;
  if (state === 'checking') {
    meta = <span className="dots-loading">{item.error ? '检测失败' : '检测中'}</span>;
  } else if (state === 'upgradable') {
    meta = (
      <>
        <span className="ver">{item.installed_version || '已安装'}</span>
        <span className="ver-arrow">→</span>
        <span className="up">{item.latest_version || '最新版'}</span>
        <span className="dot-sep"/>
        {item.category}
      </>
    );
  } else if (state === 'installable') {
    meta = <>{item.latest_version || '最新版'}<span className="dot-sep"/>远端源</>;
  } else {
    meta = <><span className="ver">{item.installed_version || item.latest_version || '已安装'}</span><span className="dot-sep"/>{item.category}</>;
  }

  return (
    <div className={`store-card ${selected ? 'selected' : ''}`}>
      <StoreIcon item={item}/>
      <div className="sc-body">
        <div className="sc-name">{item.name}</div>
        <div className="sc-desc">{item.desc || item.group || item.category}</div>
        <div className="sc-meta">{meta}</div>
      </div>
      <div className="sc-action">
        <button className={`card-checkbox ${selected ? 'on' : ''}`} onClick={() => onSelect(item.id)} title={selected ? '取消选中' : '加入批量'}/>
        <StoreButton item={item} onInstall={onInstall} onUpgrade={onUpgrade} onRecheck={onRecheck}/>
      </div>
    </div>
  );
}

function Hero({ item, onInstall, onUpgrade, onRecheck }) {
  if (!item) return null;
  const action = (() => {
    if (item.state === 'not_installed') return <button className="hero-btn" onClick={() => onInstall(item)}><MiniIcon name="download" size={15}/> 免费获取</button>;
    if (item.state === 'upgradable') return <button className="hero-btn" onClick={() => onUpgrade(item)}><MiniIcon name="download" size={15}/> 更新到 {item.latest_version || '最新版'}</button>;
    if (item.state === 'up_to_date') return <button className="hero-btn" disabled><MiniIcon name="check" size={15}/> 已是最新</button>;
    return <button className="hero-btn" onClick={() => onRecheck(item.id)}><MiniIcon name="refresh" size={15}/> 重新检测</button>;
  })();
  return (
    <div className="hero">
      <div className="hero-watermark">{itemGlyph(item)}</div>
      <div className="hero-eyebrow">本周精选 · {GROUP_META[item.group]?.label || item.category}</div>
      <div className="hero-title">{item.name}</div>
      <div className="hero-pitch">{item.desc || '来自 WebClaw 软件源的精选工具，可按需安装和更新。'}</div>
      {action}
    </div>
  );
}

function Shelf({ title, sub, items, more, onMore, selected, onSelect, onInstall, onUpgrade, onRecheck }) {
  if (!items.length) return null;
  return (
    <div className="shelf">
      <div className="shelf-head">
        <span className="shelf-title">{title}</span>
        {sub && <span className="shelf-sub">{sub}</span>}
        {more && <span className="shelf-more" onClick={onMore}>{more} <MiniIcon name="chevronDown" size={12}/></span>}
      </div>
      <div className="shelf-grid">
        {items.map(item => (
          <StoreCard
            key={item.id}
            item={item}
            selected={selected.has(item.id)}
            onSelect={onSelect}
            onInstall={onInstall}
            onUpgrade={onUpgrade}
            onRecheck={onRecheck}
          />
        ))}
      </div>
    </div>
  );
}

function App() {
  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);
  const [platform, setPlatform] = React.useState(null);
  const [manifestSource, setManifestSource] = React.useState('');
  const [items, setItems] = React.useState([]);
  const [section, setSection] = React.useState('featured');
  const [query, setQuery] = React.useState('');
  const [selected, setSelected] = React.useState(new Set());
  const [modal, setModal] = React.useState(null);
  const [actionState, setActionState] = React.useState(null);
  const [log, setLog] = React.useState(window.INITIAL_LOG);
  const [logOpen, setLogOpen] = React.useState(false);
  const [scanning, setScanning] = React.useState(false);
  const [scriptsState, setScriptsState] = React.useState(null);
  const [lastScan, setLastScan] = React.useState('');
  const [inited, setInited] = React.useState(false);

  const addLog = React.useCallback((msg, tone = 'sys') => {
    setLog(L => [...L, { ts: nowTime(), msg, tone }].slice(-80));
  }, []);

  const doScan = React.useCallback(async (platformKey) => {
    if (!tauriInvoke || !platformKey) return;
    setScanning(true);
    addLog(`扫描 ${platformKey} 平台软件清单...`, 'sys');
    let unlisten = null;
    try {
      const catalog = await tauriInvoke('get_platform_catalog', { platform: platformKey });
      setItems(catalog.map((item, index) => withIconUrl(withCatalogOrder(item, index))));
      if (tauriListen) {
        unlisten = await tauriListen('catalog-item-update', ev => {
          const updated = ev.payload;
          if (!updated?.id) return;
          setItems(prev => prev.map(i =>
            i.id === updated.id ? withIconUrl({ ...i, ...updated, icon: i.icon, _order: i._order }) : i
          ));
        });
      }
      const checked = await tauriInvoke('check_latest', { platform: platformKey });
      setItems(prev => {
        const checkedMap = Object.fromEntries(checked.map(i => [i.id, i]));
        return prev.map(i => withIconUrl({ ...i, ...(checkedMap[i.id] || {}), icon: i.icon, _order: i._order }));
      });
      const ts = nowTime();
      setLastScan(ts);
      const instCount = checked.filter(i => i.state !== 'not_installed').length;
      const upgCount = checked.filter(i => i.state === 'upgradable').length;
      addLog(`扫描完成：共 ${catalog.length} 项，已安装 ${instCount} 项，可升级 ${upgCount} 项`, 'ok');
    } catch (e) {
      addLog(`扫描失败：${stringifyError(e)}`, 'err');
    } finally {
      if (unlisten) unlisten();
      setScanning(false);
    }
  }, [addLog]);

  React.useEffect(() => {
    if (!tauriInvoke) {
      setInited(true);
      return;
    }
    async function init() {
      try {
        const info = await tauriInvoke('detect_platform');
        setPlatform(info);
        addLog(`平台：${info.label}（${info.os} / ${info.arch}）`, 'sys');
        const src = await tauriInvoke('refresh_manifest');
        setManifestSource(src);
        addLog(`软件清单来源：${{ remote: '远端', cache: '缓存', bundled: '内置' }[src] || src}`, 'sys');
        if (info.in_container) {
          setScriptsState('updating');
          tauriInvoke('refresh_scripts').then(r => {
            if (r === 'updated') {
              setScriptsState('updated');
              addLog('安装脚本已从远端更新', 'ok');
              setTimeout(() => setScriptsState(null), 3000);
            } else if (r && r.startsWith('warn:')) {
              setScriptsState('warn');
              addLog(`安装脚本更新失败：${r.slice(5)}`, 'err');
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

  React.useEffect(() => {
    document.documentElement.setAttribute('data-theme', t.dark ? 'dark' : 'light');
    document.documentElement.style.setProperty('--accent', t.accent);
    document.documentElement.style.setProperty('--accent-soft', tint(t.accent, t.dark));
  }, [t.dark, t.accent]);

  const counts = React.useMemo(() => ({
    total: items.length,
    not_installed: items.filter(i => i.state === 'not_installed').length,
    upgradable: items.filter(i => i.state === 'upgradable').length,
    up_to_date: items.filter(i => i.state === 'up_to_date').length,
    unknown: items.filter(i => i.state === 'unknown' || i.state === 'error').length,
  }), [items]);

  const groups = React.useMemo(() => {
    const seen = [];
    for (const item of items) {
      if (item.group && !seen.includes(item.group)) seen.push(item.group);
    }
    return seen.map(key => ({
      key,
      label: GROUP_META[key]?.label || key,
      sub: GROUP_META[key]?.sub || `${key} 分组软件`,
      icon: GROUP_META[key]?.icon || 'store',
    }));
  }, [items]);

  const q = query.trim().toLowerCase();
  const sortedItems = React.useMemo(() => {
    const stateRank = { not_installed: 0, upgradable: 1, unknown: 2, error: 2, up_to_date: 3 };
    return items.slice().sort((a, b) => {
      if (scanning) return (a._order ?? 9999) - (b._order ?? 9999) || a.name.localeCompare(b.name);
      return ((stateRank[a.state] ?? 4) - (stateRank[b.state] ?? 4)) || (a._order ?? 9999) - (b._order ?? 9999) || a.name.localeCompare(b.name);
    });
  }, [items, scanning]);

  const searchResults = React.useMemo(() => {
    if (!q) return null;
    return sortedItems.filter(i =>
      `${i.name} ${i.category} ${i.group} ${i.desc} ${i.installed_version || ''} ${i.latest_version || ''}`.toLowerCase().includes(q)
    );
  }, [q, sortedItems]);

  const upgradable = sortedItems.filter(i => i.state === 'upgradable');
  const installed = sortedItems.filter(i => i.state !== 'not_installed');
  const selectedUpgradable = [...selected].filter(id => items.find(i => i.id === id && i.state === 'upgradable')).length;
  const featured = upgradable[0] || sortedItems.find(i => i.state === 'not_installed') || sortedItems[0];

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

  const sectionTitle = q ? '搜索结果'
    : section === 'featured' ? '精选'
    : section === 'updates' ? '需要更新'
    : section === 'installed' ? '已安装'
    : (groups.find(g => g.key === section)?.label || '软件商店');

  const commonShelfProps = { selected, onSelect, onInstall, onUpgrade, onRecheck };
  let content;
  if (!inited) {
    content = <div className="empty"><div className="ico spin-ico"><MiniIcon name="refresh" size={32}/></div><div className="t">正在初始化...</div><div className="s">正在读取平台与软件清单</div></div>;
  } else if (q) {
    content = searchResults.length
      ? <div className="shelf-grid">{searchResults.map(item => <StoreCard key={item.id} item={item} {...commonShelfProps}/>)}</div>
      : <div className="empty"><div className="ico"><MiniIcon name="search" size={30}/></div><div className="t">没有找到「{query}」</div><div className="s">换个关键词试试，或浏览左侧分类</div></div>;
  } else if (section === 'featured') {
    content = (
      <>
        <Hero item={featured} onInstall={onInstall} onUpgrade={onUpgrade} onRecheck={onRecheck}/>
        <Shelf title="需要更新" sub={upgradable.length ? `${upgradable.length} 个工具有新版本` : '所有工具已是最新'} items={upgradable} more="查看全部" onMore={() => setSection('updates')} {...commonShelfProps}/>
        {groups.map(g => (
          <Shelf key={g.key} title={g.label} sub={g.sub} items={sortedItems.filter(i => i.group === g.key)} more="查看全部" onMore={() => setSection(g.key)} {...commonShelfProps}/>
        ))}
      </>
    );
  } else if (section === 'updates') {
    content = upgradable.length ? (
      <>
        <div className="bulk-row">
          <span className="bt"><b>{upgradable.length}</b> 个工具有可用更新</span>
          <button className="bulk-pill" onClick={() => setModal({ batch: upgradable, action: 'upgrade' })}>全部更新</button>
        </div>
        <div className="shelf-grid">{upgradable.map(item => <StoreCard key={item.id} item={item} {...commonShelfProps}/>)}</div>
      </>
    ) : <div className="empty"><div className="ico"><MiniIcon name="check" size={32}/></div><div className="t">全部都是最新的</div><div className="s">所有已安装工具都已更新到最新版本</div></div>;
  } else if (section === 'installed') {
    content = installed.length
      ? <div className="shelf-grid">{installed.map(item => <StoreCard key={item.id} item={item} {...commonShelfProps}/>)}</div>
      : <div className="empty"><div className="ico"><MiniIcon name="store" size={32}/></div><div className="t">还没有已安装项目</div><div className="s">从精选或分类中获取第一个工具</div></div>;
  } else {
    const g = groups.find(x => x.key === section);
    const groupItems = sortedItems.filter(i => i.group === section);
    content = (
      <>
        <div className="list-head"><span className="list-sub">{g?.sub} · {groupItems.length} 个工具</span></div>
        <div className="shelf-grid">{groupItems.map(item => <StoreCard key={item.id} item={item} {...commonShelfProps}/>)}</div>
      </>
    );
  }

  const sourceText = { remote: '远端已连接', cache: '使用缓存清单', bundled: '使用内置清单' }[manifestSource] || '软件源就绪';
  const latest = log[log.length - 1];

  return (
    <>
      <div className="desktop">
        <div className="window">
          <div className="body">
            <aside className="sidebar">
              <div className="sb-brand">
                <div className="sb-logo"><ClawMark s={17}/></div>
                <span className="sb-brand-name">WebClaw</span>
                <span className="sb-brand-tag">商店</span>
              </div>
              <div className="sb-search">
                <span className="search-symbol"><MiniIcon name="search" size={14}/></span>
                <input placeholder="搜索工具..." value={query} onChange={e => setQuery(e.target.value)}/>
                {query && <span className="clear" onClick={() => setQuery('')}><MiniIcon name="x" size={12}/></span>}
              </div>
              <div className="sb-scroll">
                <div className="sb-section-label">商店</div>
                <div className={`nav-item ${section === 'featured' && !q ? 'active' : ''}`} onClick={() => { setSection('featured'); setQuery(''); }}>
                  <span className="ni-ico"><MiniIcon name="store" size={16}/></span><span className="ni-label">精选</span>
                </div>
                {groups.map(g => (
                  <div key={g.key} className={`nav-item ${section === g.key && !q ? 'active' : ''}`} onClick={() => { setSection(g.key); setQuery(''); }}>
                    <span className="ni-ico"><MiniIcon name={g.icon} size={16}/></span><span className="ni-label">{g.label}</span>
                  </div>
                ))}
                <div className="sb-section-label">资源库</div>
                <div className={`nav-item ${section === 'updates' && !q ? 'active' : ''}`} onClick={() => { setSection('updates'); setQuery(''); }}>
                  <span className="ni-ico"><MiniIcon name="download" size={16}/></span><span className="ni-label">需要更新</span>
                  {counts.upgradable > 0 && <span className="nav-badge">{counts.upgradable}</span>}
                </div>
                <div className={`nav-item ${section === 'installed' && !q ? 'active' : ''}`} onClick={() => { setSection('installed'); setQuery(''); }}>
                  <span className="ni-ico"><MiniIcon name="check" size={16}/></span><span className="ni-label">已安装</span>
                </div>
              </div>
              <div className="sb-foot">
                <div className="source-pill">
                  <span className={`sp-dot ${scriptsState === 'warn' ? 'warn' : ''}`}/>
                  <div className="sp-text">
                    <div className="a">{scriptsState === 'updating' ? '脚本更新中' : scriptsState === 'updated' ? '脚本已更新' : sourceText}</div>
                    <div className="b">{platform?.label || '检测平台中'} · {counts.total} 项</div>
                  </div>
                </div>
              </div>
            </aside>

            <main className="main">
              <div className="main-top">
                <span className="main-title">{sectionTitle}</span>
                <div className="main-top-spacer"/>
                {selectedUpgradable > 0 && <button className="scan-btn" onClick={onBatchUpgrade}>{selectedUpgradable} 项更新</button>}
                <button className="scan-btn" onClick={onRecheck} disabled={scanning || !platform?.key}>
                  <span className={scanning ? 'spin-ico' : ''}><MiniIcon name={scanning ? 'refresh' : 'scan'} size={14}/></span>
                  {scanning ? '检查中...' : '检查更新'}
                </button>
                <button className="icon-btn" title="切换主题" onClick={() => setTweak('dark', !t.dark)}>
                  <MiniIcon name={t.dark ? 'sun' : 'moon'} size={16}/>
                </button>
                <button className="icon-btn" title="设置" onClick={() => window.postMessage({ type: '__activate_edit_mode' }, '*')}>
                  <MiniIcon name="settings" size={16}/>
                </button>
              </div>
              {scanning && <div className="scan-progress"><div className="bar"/></div>}
              <div className="main-scroll">{content}</div>
            </main>
          </div>

          <div className={`statusbar ${logOpen ? 'open' : ''}`}>
            <div className="statusbar-row" onClick={() => setLogOpen(o => !o)}>
              <span className="status-ico"><MiniIcon name="terminal" size={13}/></span>
              <span className="status-label">活动</span>
              <span className="status-count">{log.length}</span>
              <span className="status-latest">{latest && <><span className="ts">[{latest.ts}]</span> {latest.msg}</>}</span>
              <span className="status-toggle">上次检查 {lastScan || '未扫描'}<span className="chev"><MiniIcon name="chevronUp" size={13}/></span></span>
            </div>
            <div className="log-panel">
              <div className="log-scroll">
                {log.slice().reverse().map((entry, i) => (
                  <div key={i} className={`log-line ${entry.tone || 'sys'}`}>
                    <span className="ts">{entry.ts}</span>
                    <span className="ico"><MiniIcon name={entry.tone === 'ok' ? 'check' : entry.tone === 'err' ? 'alert' : entry.tone === 'up' ? 'download' : 'terminal'} size={12}/></span>
                    <span className="msg">{entry.msg}</span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      </div>

      <TweaksPanel>
        <TweakSection label="主题"/>
        <TweakColor label="主色" value={t.accent}
                    options={['#0A6CFF', '#D4623A', '#3F6E91', '#5C8A4A']}
                    onChange={(v) => setTweak('accent', v)}/>
        <TweakToggle label="深色模式" value={t.dark} onChange={(v) => setTweak('dark', v)}/>
      </TweaksPanel>

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
    </>
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

function tint(hex, dark) {
  const [r, g, b] = hexToRgb(hex);
  if (dark) return `rgba(${r},${g},${b},0.18)`;
  return `rgba(${r},${g},${b},0.1)`;
}

ReactDOM.createRoot(document.getElementById('root')).render(<App/>);
