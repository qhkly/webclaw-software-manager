// components.jsx — UI building blocks

const Icon = ({ name, size = 16, stroke = 1.6 }) => {
  const paths = {
    claw: <g><path d="M5 4c0 5 3 8 7 8s7-3 7-8" /><path d="M9 12v4" /><path d="M12 13v5" /><path d="M15 12v4" /></g>,
    search: <g><circle cx="11" cy="11" r="6"/><path d="m20 20-3.5-3.5"/></g>,
    scan: <g><path d="M3 7V5a2 2 0 0 1 2-2h2"/><path d="M17 3h2a2 2 0 0 1 2 2v2"/><path d="M21 17v2a2 2 0 0 1-2 2h-2"/><path d="M7 21H5a2 2 0 0 1-2-2v-2"/><path d="M3 12h18"/></g>,
    settings: <g><circle cx="12" cy="12" r="3"/><path d="M19 12a7 7 0 0 0-.1-1.2l2-1.5-2-3.5-2.4.9a7 7 0 0 0-2-1.2L14 3h-4l-.5 2.5a7 7 0 0 0-2 1.2L5.1 5.8l-2 3.5 2 1.5A7 7 0 0 0 5 12a7 7 0 0 0 .1 1.2l-2 1.5 2 3.5 2.4-.9a7 7 0 0 0 2 1.2L10 21h4l.5-2.5a7 7 0 0 0 2-1.2l2.4.9 2-3.5-2-1.5A7 7 0 0 0 19 12Z"/></g>,
    sun: <g><circle cx="12" cy="12" r="4"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4"/></g>,
    moon: <path d="M21 13A9 9 0 1 1 11 3a7 7 0 0 0 10 10Z"/>,
    refresh: <g><path d="M3 12a9 9 0 0 1 15-6.7L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-15 6.7L3 16"/><path d="M3 21v-5h5"/></g>,
    check: <path d="m5 12 5 5L20 7"/>,
    x: <path d="M6 6l12 12M18 6 6 18"/>,
    chevronDown: <path d="m6 9 6 6 6-6"/>,
    chevronUp: <path d="m6 15 6-6 6 6"/>,
    upgrade: <g><path d="M12 19V5"/><path d="m5 12 7-7 7 7"/></g>,
    download: <g><path d="M12 5v14"/><path d="m5 12 7 7 7-7"/><path d="M3 19h18"/></g>,
    alert: <g><path d="M12 9v4M12 17h.01"/><path d="M10.3 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.7 3.86a2 2 0 0 0-3.4 0Z"/></g>,
    info: <g><circle cx="12" cy="12" r="9"/><path d="M12 8h.01M11 12h1v4h1"/></g>,
    terminal: <g><path d="M4 17l5-5-5-5"/><path d="M11 19h9"/></g>,
    clock: <g><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></g>,
    store: <g><path d="M3 9l1-3h16l1 3"/><path d="M3 9h18v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V9Z"/><path d="M9 22V12"/><path d="M15 22V12"/><path d="M3 22h18"/></g>,
    fileText: <g><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/><path d="M8 13h8M8 17h8M8 9h2"/></g>,
  };
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24"
         fill="none" stroke="currentColor" strokeWidth={stroke} strokeLinecap="round" strokeLinejoin="round">
      {paths[name]}
    </svg>
  );
};

const ClawLogo = ({ size = 34 }) => (
  <>
    <img className="hdr-logo-img logo-light" src="assets/claw-black.png" width={size} height={size} alt="" />
    <img className="hdr-logo-img logo-dark" src="assets/claw-white.png" width={size} height={size} alt="" />
  </>
);

const MANIFEST_SOURCE_LABELS = { remote: '远端', cache: '缓存', bundled: '内置' };

const SCRIPTS_STATE_CFG = {
  updating: { cls: 'scripts-updating', icon: 'refresh', iconCls: 'spin', label: '脚本更新中' },
  updated:  { cls: 'scripts-ok',       icon: 'check',   iconCls: '',     label: '脚本已更新' },
  warn:     { cls: 'scripts-warn',     icon: 'alert',   iconCls: '',     label: '脚本更新失败' },
};

const Header = ({ platformLabel, manifestSource, counts, lastScan, onScan, scanning, dark, onToggleDark, onOpenTweaks, scriptsState }) => (
  <header className="hdr">
    <div className="hdr-logo"><ClawLogo size={24}/></div>
    <div className="hdr-title">
      <h1>WebClaw 软件商店</h1>
      <small>{platformLabel} · {counts.total} 项</small>
    </div>
    {manifestSource && (
      <div className="hdr-badge">
        <span className={`source-badge source-${manifestSource}`}>
          {MANIFEST_SOURCE_LABELS[manifestSource] || manifestSource}
        </span>
      </div>
    )}
    {scriptsState && SCRIPTS_STATE_CFG[scriptsState] && (() => {
      const cfg = SCRIPTS_STATE_CFG[scriptsState];
      return (
        <div className="hdr-badge">
          <span className={`scripts-badge ${cfg.cls}`}>
            <span className={cfg.iconCls}><Icon name={cfg.icon} size={12}/></span>
            {cfg.label}
          </span>
        </div>
      );
    })()}
    <div className="hdr-spacer"/>
    <div className="hdr-meta">
      <span>上次扫描</span>
      <strong>{lastScan || '未扫描'}</strong>
    </div>
    <button className="btn btn-ghost btn-icon" aria-label="设置" onClick={onOpenTweaks}><Icon name="settings" size={15}/></button>
    <button className="theme-toggle" onClick={onToggleDark} aria-label="切换深浅模式">
      <Icon name={dark ? 'sun' : 'moon'} size={14}/>
    </button>
    <button className={`btn ${scanning ? '' : 'btn-primary'}`} onClick={onScan} disabled={scanning}>
      {scanning
        ? <><Icon name="refresh" size={14} stroke={2}/> 扫描中...</>
        : <><Icon name="scan" size={14} stroke={2}/> 扫描</>}
    </button>
  </header>
);

const Stats = ({ counts, active, onPick }) => {
  const tiles = [
    { key: 'all',          n: counts.total,         lbl: '全部',   cls: 'c-ink'    },
    { key: 'not_installed',n: counts.not_installed,  lbl: '可安装', cls: 'c-info'   },
    { key: 'upgradable',   n: counts.upgradable,     lbl: '可升级', cls: 'c-accent' },
    { key: 'up_to_date',   n: counts.up_to_date,     lbl: '已最新', cls: 'c-ok'     },
    { key: 'unknown',      n: counts.unknown,        lbl: '待检测', cls: 'c-warn'   },
  ];
  return (
    <div className="stats">
      {tiles.map(tile => (
        <div key={tile.key} className={`stat ${active === tile.key ? 'active' : ''}`} onClick={() => onPick(tile.key)}>
          <div className={`stat-num ${tile.cls}`}>{tile.n}</div>
          <div className="stat-lbl">{tile.lbl}</div>
        </div>
      ))}
    </div>
  );
};

const Toolbar = ({ filter, onFilter, counts, q, onQ, sort, onSort }) => {
  const chips = [
    { k: 'all',          l: '全部',   n: counts.total         },
    { k: 'not_installed',l: '可安装', n: counts.not_installed  },
    { k: 'upgradable',   l: '可升级', n: counts.upgradable     },
    { k: 'up_to_date',   l: '已最新', n: counts.up_to_date     },
    { k: 'unknown',      l: '待检测', n: counts.unknown        },
  ];
  return (
    <div className="toolbar">
      <div className="chips" role="tablist">
        {chips.map(c => (
          <button key={c.k} className={`chip ${filter === c.k ? 'active' : ''}`} onClick={() => onFilter(c.k)}>
            {c.l}<span className="chip-count">{c.n}</span>
          </button>
        ))}
      </div>
      <div className="search">
        <span className="search-icon"><Icon name="search" size={14}/></span>
        <input value={q} onChange={(e) => onQ(e.target.value)} placeholder="搜索名称 / 分类 / 版本..." />
      </div>
      <div className="spacer-x"/>
      <div className="sort">
        排序
        <select value={sort} onChange={(e) => onSort(e.target.value)}>
          <option value="state">状态优先</option>
          <option value="name">名称 A-Z</option>
          <option value="category">分类</option>
          <option value="group">分组</option>
        </select>
      </div>
    </div>
  );
};

const STATE_DOT  = { upgradable: 'up', up_to_date: 'ok', unknown: 'unk', not_installed: 'off', error: 'fatal' };
const STATE_TAGS = {
  upgradable:    <span className="tag tag-up">可升级</span>,
  up_to_date:    <span className="tag tag-ok">已最新</span>,
  not_installed: <span className="tag tag-info">可安装</span>,
  unknown:       <span className="tag tag-warn">待检测</span>,
  error:         <span className="tag tag-danger">错误</span>,
};

const Card = ({ item, selected, onSelect, onInstall, onUpgrade, onRecheck, dense }) => {
  const [imgErr, setImgErr] = React.useState(false);
  const dotCls = STATE_DOT[item.state] || 'unk';
  const cardCls = ['card',
    item.state === 'upgradable' ? 'upgradable' : '',
    item.state === 'not_installed' ? 'not-installed' : '',
    selected ? 'selected' : '',
  ].filter(Boolean).join(' ');

  const actionButton = (() => {
    switch (item.state) {
      case 'not_installed':
        return <button className="btn btn-sm btn-primary" onClick={() => onInstall(item)}><Icon name="download" size={12}/> 安装</button>;
      case 'upgradable':
        return <button className="btn btn-sm btn-accent" onClick={() => onUpgrade(item)}><Icon name="upgrade" size={12}/> 升级</button>;
      case 'up_to_date':
        return <span className="badge-latest"><Icon name="check" size={11} stroke={2.5}/> 最新</span>;
      default:
        return <button className="btn btn-sm" onClick={() => onRecheck(item.id)}><Icon name="refresh" size={12}/> 检测</button>;
    }
  })();

  return (
    <div className={cardCls}>
      <div className="card-layout">
        <div className="card-icon-wrap">
          {item.iconUrl && !imgErr
            ? <img className="card-icon" src={item.iconUrl} alt="" onError={() => setImgErr(true)}/>
            : <div className="card-icon-fallback">{item.name?.[0]?.toUpperCase() || '?'}</div>
          }
          <span className={`dot ${dotCls}`} aria-hidden/>
        </div>
        <div className="card-body">
          <div className="card-head">
            <div className="card-title-wrap">
              <div className="card-title">{item.name}</div>
              <div className="card-sub">{item.desc || item.category}</div>
            </div>
            <div className="card-tags">
              <span className="tag tag-type">{item.category}</span>
              {STATE_TAGS[item.state]}
            </div>
          </div>
          <div className="versions">
            <span className={`ver current ${item.installed_version ? '' : 'unknown'}`}>
              {item.installed_version ?? (item.state === 'not_installed' ? '未安装' : '未检测')}
            </span>
            {item.state !== 'not_installed' && (
              <>
                <span className="ver-arrow">→</span>
                {item.latest_version
                  ? <span className={`ver ${item.state === 'upgradable' ? 'latest' : 'equal'}`}>{item.latest_version}</span>
                  : <span className="ver unknown">?</span>}
              </>
            )}
          </div>
          <div className="card-foot">
            <button className={`card-checkbox ${selected ? 'on' : ''}`} onClick={() => onSelect(item.id)} title={selected ? '取消选中' : '加入批量'}>
              {selected && <Icon name="check" size={11} stroke={3}/>}
            </button>
            {actionButton}
            {item.state !== 'not_installed' && (
              <button className="btn btn-sm" onClick={() => onRecheck(item.id)}>
                <Icon name="refresh" size={12}/>
              </button>
            )}
            <div className="spacer"/>
            {item.error && (
              <button className="btn-icon" title={item.error}><Icon name="alert" size={13}/></button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};

const BatchBar = ({ count, onClear, onUpgrade, onRecheck }) => (
  <div className="batch">
    <span className="batch-info">已选中 <b>{count}</b> 项可升级</span>
    <div style={{ flex: 1 }}/>
    <button className="btn btn-sm" onClick={onRecheck} style={{ background: 'transparent', color: 'var(--bg)', borderColor: 'rgba(255,255,255,.2)' }}>
      <Icon name="refresh" size={12}/> 重新检测
    </button>
    <button className="btn btn-sm btn-accent" onClick={onUpgrade}><Icon name="upgrade" size={12}/> 批量升级</button>
    <button className="btn btn-sm" onClick={onClear} style={{ background: 'transparent', color: 'var(--bg)', borderColor: 'rgba(255,255,255,.2)' }}>取消</button>
  </div>
);

const LogBar = ({ entries, open, onToggle }) => {
  const last = entries[entries.length - 1];
  return (
    <div className={`logbar ${open ? 'open' : ''}`}>
      <div className="logbar-head" onClick={onToggle}>
        <Icon name="terminal" size={13}/>
        <span className="label">操作日志</span>
        <span className="badge">{entries.length}</span>
        {!open && last && <span className="latest">[{last.ts}] {last.msg}</span>}
        <div style={{ flex: 1 }}/>
        <span style={{ fontSize: 11, color: 'var(--ink-3)' }}>{open ? '收起' : '展开'}</span>
        <Icon name={open ? 'chevronDown' : 'chevronUp'} size={14}/>
      </div>
      <div className="logbar-body">
        {entries.map((e, i) => (
          <div key={i} className="logline">
            <span className="log-ts">[{e.ts}]</span>
            <span className={`log-msg ${e.tone || ''}`}>{e.msg}</span>
          </div>
        ))}
      </div>
    </div>
  );
};

const ACTION_LABELS = { install: '安装', upgrade: '升级' };

const ActionModal = ({ item, batch, action, actionState, onClose, onConfirm }) => {
  const targets = batch || [item];
  const phase = actionState?.phase || 'confirm';
  const stage = String(actionState?.stage || '').toUpperCase();
  const percent = Math.max(0, Math.min(100, Number(actionState?.percent || 0)));
  const doneOk = phase === 'done' && stage !== 'ERROR';
  const label = ACTION_LABELS[action] || action;

  return (
    <div className="modal-backdrop" onClick={phase === 'running' ? undefined : onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>
            {batch ? `批量${label} · ${batch.length} 项` : `${label} ${item.name}`}
          </h3>
          <button className="btn-icon btn-ghost" onClick={onClose} disabled={phase === 'running'}><Icon name="x" size={14}/></button>
        </div>

        {phase === 'confirm' && (
          <>
            <div className="modal-body">
              <div>
                <div className="modal-label">即将执行</div>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                  {targets.map(t => (
                    <code key={t.id} className="codeblock">
                      <span style={{ color: 'var(--ink-3)' }}># </span>
                      {label} {t.id}
                      {action === 'upgrade' && t.installed_version && (
                        <span style={{ color: 'var(--ink-3)' }}> ({t.installed_version} → {t.latest_version ?? '最新版'})</span>
                      )}
                    </code>
                  ))}
                </div>
              </div>
            </div>
            <div className="modal-foot">
              <div className="spacer"/>
              <button className="btn btn-sm" onClick={onClose}>取消</button>
              <button className="btn btn-sm btn-accent" onClick={() => onConfirm(targets, action)}>开始{label}</button>
            </div>
          </>
        )}

        {phase === 'running' && (
          <>
            <div className="modal-body">
              <div className="callout callout-info">
                <span className="callout-icon"><Icon name="refresh" size={16}/></span>
                <div>正在{label} · {stage}</div>
              </div>
              <div className="progress"><div style={{ width: `${percent}%` }}/></div>
              <pre className="upgrade-output">{(actionState?.lines || []).join('\n') || '等待输出...'}</pre>
            </div>
            <div className="modal-foot">
              <div className="spacer"/>
              <button className="btn btn-sm" disabled>{label}中...</button>
            </div>
          </>
        )}

        {phase === 'done' && (
          <>
            <div className="modal-body">
              <div className="callout" style={{ background: doneOk ? 'var(--ok-tint)' : 'var(--danger-tint)', color: doneOk ? 'var(--ok)' : 'var(--danger)' }}>
                <span className="callout-icon"><Icon name={doneOk ? 'check' : 'alert'} size={16} stroke={2.4}/></span>
                <div><strong>{doneOk ? `${label}完成。` : `${label}失败。`}</strong><br/>{targets.length} 项任务已结束。</div>
              </div>
              <pre className="upgrade-output">{(actionState?.lines || []).join('\n') || '无输出'}</pre>
            </div>
            <div className="modal-foot">
              <div className="spacer"/>
              <button className="btn btn-sm btn-accent" onClick={onClose}>知道了</button>
            </div>
          </>
        )}
      </div>
    </div>
  );
};

const LogModal = ({ name, content, onClose }) => (
  <div className="modal-backdrop" onClick={onClose}>
    <div className="modal log-modal" onClick={(e) => e.stopPropagation()}>
      <div className="modal-head">
        <h3>{name} · 日志</h3>
        <button className="btn-icon btn-ghost" onClick={onClose}><Icon name="x" size={14}/></button>
      </div>
      <div className="modal-body">
        <pre className="log-pre">{content || '无日志输出'}</pre>
      </div>
    </div>
  </div>
);

Object.assign(window, {
  Icon, ClawLogo, Header, Stats, Toolbar,
  Card, BatchBar, LogBar, ActionModal, LogModal,
});
