/* ============ SuperTask 全量原型 · 逻辑 / 路由 / 交互 ============ */
'use strict';

/* ---------- 图标 ---------- */
const ICONS = {
  run:'<path d="M4 6h16M4 12h16M4 18h10"/>',
  logs:'<path d="M4 5h16M4 10h10M4 15h13M4 20h7"/>',
  config:'<path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M18 18h2"/><circle cx="15" cy="6" r="2"/><circle cx="9" cy="12" r="2"/><circle cx="15" cy="18" r="2"/>',
  templates:'<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/>',
  env:'<rect x="4" y="4" width="16" height="16" rx="2"/><path d="M9 9h6v6H9zM4 9h5M15 9h5M4 15h5M15 15h5M9 4v5M15 4v5M9 15v5M15 15v5"/>',
  git:'<circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="9" r="2.5"/><path d="M6 8.5v7M6 6h9a3 3 0 013 3v0"/>',
  docker:'<rect x="3" y="8" width="18" height="11" rx="2"/><path d="M7 8V5h3l1 3M3 13h3M9 13h3M15 13h3"/>',
  gateway:'<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2"/>',
  cloud:'<path d="M7 18a4 4 0 01-.5-7.97A6 6 0 0118 9.5a3.5 3.5 0 01-.5 8.5z"/>',
  ai:'<path d="M12 3l1.8 4.6L18 9l-4.2 1.4L12 15l-1.8-4.6L6 9l4.2-1.4zM5 15l.9 2.3L8 18l-2.1.7L5 21l-.9-2.3L2 18l2.1-.7zM19 14l.7 1.8L21 16.5l-1.3.7L19 19l-.7-1.8L17 16.5l1.3-.7z"/>',
  settings:'<circle cx="12" cy="12" r="3"/><path d="M19 12a7 7 0 00-.1-1l2-1.5-2-3.5-2.4 1a7 7 0 00-1.7-1l-.4-2.5h-4l-.4 2.5a7 7 0 00-1.7 1l-2.4-1-2 3.5 2 1.5a7 7 0 000 2l-2 1.5 2 3.5 2.4-1a7 7 0 001.7 1l.4 2.5h4l.4-2.5a7 7 0 001.7-1l2.4 1 2-3.5-2-1.5a7 7 0 00.1-1z"/>',
  play:'<path d="M7 4l13 8-13 8z" fill="currentColor" stroke="none"/>',
  stop:'<rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none"/>',
  restart:'<path d="M3 12a9 9 0 109-9 9 9 0 00-6.3 2.6L3 8"/><path d="M3 3v5h5"/>',
  terminal:'<path d="M5 7l4 4-4 4M12 16h7"/>',
  folder:'<path d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2z"/>',
  search:'<circle cx="11" cy="11" r="7"/><path d="M21 21l-4-4"/>',
  chip:'<path d="M6 3v3M18 3v3M6 18v3M18 18v3M3 6h3M3 18h3M18 6h3M18 18h3M8 8h8v8H8z"/>',
  cpu:'<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3"/>',
  box:'<path d="M21 8l-9-5-9 5 9 5 9-5zM3 8v8l9 5 9-5V8M12 13v8"/>',
  net:'<circle cx="12" cy="12" r="2.5"/><path d="M12 3v7M12 14v7M3 12h7M14 12h7"/>',
  spark:'<path d="M12 3l1.8 4.6L18 9l-4.2 1.4L12 15l-1.8-4.6L6 9l4.2-1.4z"/>',
  gear:'<circle cx="12" cy="12" r="3"/><path d="M19 12a7 7 0 00-.1-1l2-1.5-2-3.5-2.4 1a7 7 0 00-1.7-1l-.4-2.5h-4l-.4 2.5a7 7 0 00-1.7 1l-2.4-1-2 3.5 2 1.5a7 7 0 000 2l-2 1.5 2 3.5 2.4-1a7 7 0 001.7 1l.4 2.5h4l.4-2.5a7 7 0 001.7-1l2.4 1 2-3.5-2-1.5a7 7 0 00.1-1z"/>',
  close:'<line x1="6" y1="6" x2="18" y2="18"/><line x1="18" y1="6" x2="6" y2="18"/>',
  plus:'<path d="M12 5v14M5 12h14"/>',
  trash:'<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>',
  warn:'<path d="M12 9v4M12 17h.01"/><path d="M10.3 3.9L2 18a2 2 0 001.7 3h16.6A2 2 0 0022 18L13.7 3.9a2 2 0 00-3.4 0z"/>',
  check:'<path d="M5 13l4 4L19 7"/>',
  chev:'<path d="M6 9l6 6 6-6"/>',
  pulse:'<path d="M3 12h4l3-7 4 14 3-7h4"/>',
  key:'<circle cx="8" cy="8" r="4"/><path d="M11 11l9 9M17 17l2-2M14 14l2-2"/>',
  refresh:'<path d="M3 12a9 9 0 109-9 9 9 0 00-6.3 2.6L3 8"/><path d="M3 3v5h5"/>',
  layers:'<path d="M12 3l9 5-9 5-9-5zM3 13l9 5 9-5"/>',
  branch:'<circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="9" r="2.5"/><path d="M6 8.5v7M6 6h9a3 3 0 013 3v0"/>',
  download:'<path d="M12 3v12M7 10l5 5 5-5M5 21h14"/>',
  book:'<path d="M4 5a2 2 0 012-2h12v16H6a2 2 0 00-2 2zM4 21a2 2 0 012-2h14"/>',
  info:'<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>'
};
function ic(name, cls){ return `<svg class="icon ${cls||''}" viewBox="0 0 24 24" aria-hidden="true">${ICONS[name]||''}</svg>`; }

/* ---------- 功能注册表（壳上禁用 if，全部读这里） ---------- */
const FEATURES = [
  { id:'run',       path:'/run',       label:'运行',   icon:'run',       status:'live', since:'1.0' },
  { id:'logs',      path:'/logs',      label:'日志',   icon:'logs',      status:'live', since:'1.0' },
  { id:'config',    path:'/config',    label:'配置',   icon:'config',    status:'live', since:'1.0' },
  { sep:true },
  { id:'templates', path:'/templates', label:'模板',   icon:'templates', status:'soon', since:'1.1' },
  { id:'env',       path:'/env',       label:'环境',   icon:'env',       status:'live', since:'1.0', note:'探测 live · 安装升级 1.2' },
  { id:'git',       path:'/git',       label:'Git',    icon:'git',       status:'soon', since:'1.1' },
  { id:'docker',    path:'/docker',    label:'容器',   icon:'docker',    status:'soon', since:'1.3' },
  { id:'gateway',   path:'/gateway',   label:'网关',   icon:'gateway',   status:'soon', since:'1.6' },
  { id:'cloud',     path:'/cloud',     label:'云',     icon:'cloud',     status:'soon', since:'2.0' },
  { id:'ai',        path:'/ai',        label:'AI',     icon:'ai',        status:'soon', since:'2.1' },
  { id:'settings',  path:'/settings',  label:'设置',   icon:'settings',  status:'live', since:'1.0' }
];

/* ---------- 模拟数据 ---------- */
const STATE_NAMES = { stopped:'已停止', starting:'启动中', running:'运行中', unhealthy:'不健康', stopping:'停止中', exited:'已退出' };
const now = Date.now();
const services = [
  { id:'gateway', kind:'spring-boot', module:'gateway', port:8080, stack:'Spring Boot 3.2 · JDK 17', state:'running', pid:18220, startedAt:now-2*3600*1000-14*60000, grace:45, health:{ ok:true, at:'16:40:49', ms:46, type:'http' }, env:{ SERVER_PORT:'8080' }, depends:[] },
  { id:'user-api', kind:'spring-boot', module:'user-service', port:8081, stack:'Spring Boot 3.2 · JDK 17', state:'running', pid:18231, startedAt:now-2*3600*1000-10*60000, grace:45, health:{ ok:true, at:'16:40:50', ms:38, type:'http' }, env:{ SERVER_PORT:'8081' }, depends:[] },
  { id:'web', kind:'node', dir:'web', pm:'pnpm', script:'dev', port:5173, stack:'Node 20 · pnpm', state:'running', pid:19002, startedAt:now-2*3600*1000-2*60000, grace:15, health:{ ok:true, at:'16:40:51', ms:12, type:'tcp' }, env:{ PORT:'5173' }, depends:['user-api','gateway'] }
];
const scripts = [ { id:'bootstrap', desc:'安装依赖', cmds:['mvn -q -DskipTests install','pnpm --dir web install'], state:'stopped' } ];
const appState = { route:'/run', selected:null, drawerTab:'logs', collapsed:false, yamlText:sampleYaml() };

function sampleYaml(){
  return `version: 1
name: mall
root: .
env:
  SPRING_PROFILES_ACTIVE: local
services:
  gateway:
    kind: spring-boot
    module: gateway
    port: 8080
    grace_secs: 45
    health:
      type: http
      http: http://127.0.0.1:8080/actuator/health
      interval_secs: 2
      timeout_secs: 2
  user-api:
    kind: spring-boot
    module: user-service
    port: 8081
    grace_secs: 45
    health: { type: http, http: http://127.0.0.1:8081/actuator/health }
  web:
    kind: node
    dir: web
    package_manager: pnpm
    script: dev
    port: 5173
    depends_on: [user-api, gateway]
scripts:
  bootstrap:
    desc: 安装依赖
    cmds: [mvn -q -DskipTests install, pnpm --dir web install]
    timeout_secs: 1800
`;
}

/* ---------- 工具 ---------- */
const $ = (s,r=document)=>r.querySelector(s);
const $$ = (s,r=document)=>[...r.querySelectorAll(s)];
function esc(s){ return String(s).replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c])); }
function kindIcon(kind){ return ic(kind==='node'?'terminal':'layers'); }
/* (legacy helper, reserved for future use) */
function fmtDur(ms){ const s=Math.floor(ms/1000); const h=Math.floor(s/3600), m=Math.floor(s%3600/60); return (h?h+'h ':'')+m+'m'; }
function logFor(svc){
  const t = svc.id==='web' ? ['VITE ready in 412 ms','➜  Local:   http://localhost:5173/','➜  press h + enter to show help','[hmr] page reload src/App.tsx','GET / 200 12ms'] :
    ['[INFO] Starting GatewayApplication v1.0','[INFO] Tomcat started on port(s): 8080 (http)','[INFO] Started GatewayApplication in 6.142s','[INFO] Mapped actuator endpoints under /actuator','o.s.b.w.embedded.tomcat.TomcatWebServer  : Started'];
  return t.map((l,i)=>`<span class="ln"><span class="ts">16:${40+i}:${10+i*7}</span> <span class="${i%4===3?'st-err':'st-out'}">${esc(l)}</span></span>`).join('');
}

/* ---------- 渲染：侧栏 / 状态栏 ---------- */
function renderNav(){
  const nav = $('#nav');
  nav.innerHTML = FEATURES.map(f=>{
    if(f.sep) return `<div class="nav-sep"></div>`;
    const tip = f.status==='soon' ? `<span class="nav-ver tip">将在 ${f.since} 提供</span>` : '';
    const ver = f.status==='soon' ? `<span class="nav-ver">即将 ${f.since}</span>` : '';
    return `<div class="nav-item ${f.status} ${appState.route===f.path?'active':''}" data-route="${f.path}" data-tip="${f.status==='soon'?('将在 '+f.since+' 提供'):''}" role="button" tabindex="0">
      <span class="icn">${ic(f.icon)}</span><span class="nm">${f.label}</span>${ver}${tip}</div>`;
  }).join('');
}
function renderStatus(){
  const running = services.filter(s=>s.state==='running').length;
  const sb = $('#statusbar');
  sb.innerHTML = `
    <span class="probe"><span class="pdot"></span>JDK 17</span>
    <span class="probe"><span class="pdot"></span>Maven 3.9</span>
    <span class="probe"><span class="pdot"></span>Node 20</span>
    <span class="probe"><span class="pdot"></span>pnpm 9</span>
    <span class="spacer"></span>
    <span class="pulse"><span class="heartbeat"><i></i><i></i><i></i><i></i></span> 已同步</span>
    <span class="chip"><span class="dot running" style="width:.4rem;height:.4rem"></span>${running} running</span>
    <span class="kbd">⌘K</span> 命令面板
    <span class="ver" style="color:var(--t3)">v1.0.0</span>`;
}

/* ---------- 路由 ---------- */
function router(){
  const hash = location.hash.replace(/^#/,'') || '/run';
  appState.route = hash;
  renderNav();
  const phTtl=$('#phTtl'), phSub=$('#phSub');
  const f = FEATURES.find(x=>x.path===hash);
  const titles = { '/run':['运行','管理服务启停与健康'], '/logs':['日志','分服务实时输出'], '/config':['配置','表单与原始 YAML'], '/templates':['模板','即将 1.1'], '/env':['环境','工具链探测 · 安装升级 1.2'], '/git':['Git','即将 1.1'], '/docker':['容器','即将 1.3'], '/gateway':['网关','即将 1.6'], '/cloud':['云','即将 2.0'], '/ai':['AI','即将 2.1'], '/settings':['设置','外观 · 常规 · 关于'], '/welcome':['欢迎',''] };
  const t = titles[hash]||['',''];
  phTtl.textContent = t[0]; phSub.textContent = t[1];
  const view = $('#view');
  if(hash==='/run') view.innerHTML = renderRun();
  else if(hash==='/logs') view.innerHTML = renderLogs();
  else if(hash==='/config') view.innerHTML = renderConfig();
  else if(hash==='/env') view.innerHTML = renderEnv();
  else if(hash==='/settings') view.innerHTML = renderSettings();
  else if(hash==='/welcome') view.innerHTML = renderWelcome();
  else view.innerHTML = renderComingSoon(f);
  if(hash==='/run') bindRun();
  if(hash==='/config') bindConfig();
  // 路由后关闭抽屉（顶层导航不保留抽屉态）
  if(hash!=='/run'){ const d=$('#drawer'); if(d) d.classList.remove('open'); }
  view.scrollTop = 0;
}

/* ---------- 运行页 ---------- */
function renderRun(){
  const live = services;
  let body;
  if(live.length===0){
    body = `<div class="empty-state"><div class="big">${ic('config')}</div><div>当前工作区还没有服务</div><button class="btn primary" data-act="scan">扫描生成草稿</button></div>`;
  } else {
    body = `<div class="run-grid">${live.map(cardHTML).join('')}</div>`;
  }
  return `
    <div class="pagehead-inner" style="display:flex;gap:.6rem;margin-bottom:.9rem;flex-wrap:wrap">
      <button class="btn primary" data-act="start-all">${ic('play')} 启动全部</button>
      <button class="btn" data-act="stop-all">${ic('stop')} 停止全部</button>
      <button class="btn" data-act="open-dir">${ic('folder')} 打开目录</button>
      <button class="btn ghost" data-act="scan">${ic('refresh')} 重新扫描</button>
    </div>
    ${body}
    ${drawerHTML()}`;
}
function cardHTML(s){
  const st = s.state;
  const canStart = st==='stopped'||st==='exited';
  const canStop = st==='running'||st==='unhealthy'||st==='starting';
  const canRestart = st==='running'||st==='unhealthy'||st==='exited';
  const cmd = s.kind==='node'?`$ ${s.pm} ${s.port?'-p '+s.port:''}`:`$ mvn -pl ${s.module} -am spring-boot:run`;
  const time = s.startedAt?fmtDur(Date.now()-s.startedAt):'';
  return `<div class="card ${appState.selected===s.id?'selected':''}" data-act="select" data-svc="${s.id}" data-state="${st}">
    <div class="card-row">
      <span class="ch-name">${s.id}</span>
      <span class="ch-kind">${s.kind==='node'?'NODE':'SPRING'}</span>
      <span class="ch-cmd">${cmd}</span>
      <span class="ch-spacer"></span>
      <span class="ch-state ${st}"><span class="dot"></span>${STATE_NAMES[st]}</span>
      ${time?`<span class="ch-time">${time}</span>`:''}
      <span class="card-acts">
        <button class="go" data-act="start" data-svc="${s.id}" ${canStart?'':'disabled'} aria-label="启动" title="启动">${ic('play')}</button>
        <button class="stop" data-act="stop" data-svc="${s.id}" ${canStop?'':'disabled'} aria-label="停止" title="停止">${ic('stop')}</button>
        <button data-act="restart" data-svc="${s.id}" ${canRestart?'':'disabled'} aria-label="重启" title="重启">${ic('restart')}</button>
      </span>
    </div>
    <div class="card-row card-tags">
      <span class="tag mono">PORT <em>:${s.port}</em></span>
      <span class="tag">${s.stack||(s.kind==='node'?'Node 20 + pnpm':'JDK 17 + Maven 3.9')}</span>
      <span class="tag">L${1+s.depends.length} · ${s.depends.length?s.depends.join(' → ')+' → '+s.id:'无依赖'}</span>
      <span class="tag live">${s.health?.ok?(s.health.ms+'ms'):'— · —'}</span>
      <span class="tag">Job Object</span>
      <span class="spacer"></span>
      ${s.pid?`<span class="pid">PID <b>${s.pid}</b></span>`:''}
    </div>
  </div>`;
}
function drawerHTML(){
  const s = services.find(x=>x.id===appState.selected);
  if(!s) return `<div class="drawer" id="drawer"></div>`;
  const tabs = [
    { id:'logs', label:'日志', live:true },
    { id:'env', label:'环境', live:true },
    { id:'health', label:'健康', live:true },
    { id:'terminal', label:'终端', live:false },
    { id:'metrics', label:'指标', live:false },
    { id:'container', label:'容器', live:false },
    { id:'proxy', label:'代理', live:false }
  ];
  return `<div class="drawer open" id="drawer" role="dialog" aria-label="${s.id} 详情">
    <div class="drawer-head">
      <span class="dh-name">${s.id}</span>
      <span class="ch-state ${s.state}"><span class="dot"></span>${STATE_NAMES[s.state]}</span>
      <span class="dh-sub">${s.kind==='node'?'Node · '+s.pm:'Spring · '+s.module}</span>
      <button class="icon-btn close" data-act="close-drawer" aria-label="关闭">${ic('close')}</button>
    </div>
    <div class="tabs">
      ${tabs.map(t=>`<button class="tab ${appState.drawerTab===t.id?'active':''}" data-act="tab" data-tab="${t.id}" ${t.live?'':'disabled'} role="tab">${t.label}${t.live?'':ic('lock')}</button>`).join('')}
    </div>
    <div class="tab-body ${appState.drawerTab==='logs'?'active':''}" data-body="logs">${tabLogs(s)}</div>
    <div class="tab-body ${appState.drawerTab==='env'?'active':''}" data-body="env">${tabEnv(s)}</div>
    <div class="tab-body ${appState.drawerTab==='health'?'active':''}" data-body="health">${tabHealth(s)}</div>
    <div class="tab-body ${appState.drawerTab==='terminal'?'active':''}" data-body="terminal">${tabSoon('终端','1.5 PTY 终端')}</div>
    <div class="tab-body ${appState.drawerTab==='metrics'?'active':''}" data-body="metrics">${tabSoon('指标','1.2 CPU / 内存')}</div>
    <div class="tab-body ${appState.drawerTab==='container'?'active':''}" data-body="container">${tabSoon('容器','1.3 容器')}</div>
    <div class="tab-body ${appState.drawerTab==='proxy'?'active':''}" data-body="proxy">${tabSoon('代理','1.6 网关代理')}</div>
  </div>`;
}
function tabLogs(s){ return `<div class="logbar"><span class="chip">${ic('pulse')} 跟随底部</span><button class="btn sm" data-act="log-pause">${ic('stop')} 暂停</button><button class="btn sm" data-act="log-clear">${ic('trash')} 清屏</button><span class="spacer"></span><span class="chip">${s.id} · .supertask/logs/${s.id}.log</span></div><div class="logview">${logFor(s)}</div>`; }
function tabEnv(s){
  const rows = Object.entries(s.env).map(([k,v])=>`<div class="kv-row"><input value="${esc(k)}" aria-label="键"><input value="${esc(v)}" aria-label="值"><button class="icon-btn del" data-act="env-del" aria-label="删除">${ic('trash')}</button></div>`).join('');
  return `<div class="field"><label>端口（运行中保存不自动重启）</label><input type="number" value="${s.port}"></div>
    <div class="banner warn">${ic('warn')} 未重启：端口/环境变量已写入 YAML，需重启服务生效</div>
    <div class="field"><label>环境变量（系统 ⊂ 工作区 ⊂ 服务）</label><div class="kv">${rows}<button class="btn sm ghost" data-act="env-add">${ic('plus')} 添加</button></div></div>
    <button class="btn primary" data-act="env-save">${ic('check')} 保存并写回 YAML</button>`;
}
function tabHealth(s){
  const h=s.health||{};
  return `<div class="hp-grid">
    <div class="hp-card"><h4>探测配置</h4>
      <div class="hp-row"><span class="k">类型</span><span class="v">${h.type==='tcp'?'TCP':h.type==='http'?'HTTP':'无'}</span></div>
      <div class="hp-row"><span class="k">目标</span><span class="v">${s.kind==='node'?('127.0.0.1:'+s.port):('GET /actuator/health')}</span></div>
      <div class="hp-row"><span class="k">间隔 / 超时</span><span class="v">2s / 2s</span></div>
      <div class="hp-row"><span class="k">grace</span><span class="v">${s.grace}s</span></div>
    </div>
    <div class="hp-card"><h4>最近结果</h4>
      <div class="hp-row"><span class="k">状态</span><span class="v" style="color:var(--ok-deep)">${h.ok?'成功 · 200':'失败'}</span></div>
      <div class="hp-row"><span class="k">耗时</span><span class="v">${h.ms}ms</span></div>
      <div class="hp-row"><span class="k">时间</span><span class="v">${h.at||'—'}</span></div>
      <div class="hp-row"><span class="k">失败原因</span><span class="v">${h.ok?'—（无）':'依赖超时'}</span></div>
    </div></div>`;
}
function tabSoon(name,ver){ return `<div class="cs" style="min-height:40vh"><div class="cs-card"><div class="cs-ic">${ic('spark')}</div><h2>${name}</h2><p>该功能将在后续版本提供。</p><span class="cs-ver">即将 ${ver}</span></div></div>`; }

function bindRun(){
  // 抽屉内交互已在全局委托处理；这里无需额外绑定
}

/* ---------- 日志页 ---------- */
function renderLogs(){
  const sel = appState.selected || services[0].id;
  const side = services.map(s=>`<div class="si ${s.id===sel?'active':''}" data-act="select-log" data-svc="${s.id}">
    <span class="dot ${s.state}"></span><span>${s.id}</span><span class="chip" style="margin-left:auto">:${s.port}</span></div>`).join('');
  const s = services.find(x=>x.id===sel);
  return `<div class="logs-layout">
    <div class="logs-side"><div class="recent-h" style="padding:.4rem .6rem">服务</div>${side}
      <div class="recent-h" style="padding:.4rem .6rem">脚本</div>
      <div class="si" data-act="select-log" data-svc="__bootstrap"><span class="dot stopped"></span><span>bootstrap</span><span class="chip" style="margin-left:auto">脚本</span></div>
    </div>
    <div class="logs-main">
      <div class="loghead"><b>${s.id}</b><span class="ch-state ${s.state}"><span class="dot"></span>${STATE_NAMES[s.state]}</span>
        <span class="spacer" style="flex:1"></span><button class="btn sm" data-act="log-pause">${ic('stop')} 暂停</button><button class="btn sm" data-act="log-clear">${ic('trash')} 清屏</button></div>
      <div class="logview">${logFor(s)}</div>
    </div></div>`;
}

/* ---------- 配置页 ---------- */
function renderConfig(){
  const svcBlocks = services.map(s=>`
    <div class="svc-block">
      <div class="bh"><span class="id">${s.id}</span><span class="chip">${s.kind}</span><button class="icon-btn del" data-act="svc-del" aria-label="删除">${ic('trash')}</button></div>
      <div class="form-row">
        <div class="field"><label>${s.kind==='node'?'目录 dir':'模块 module'}</label><input value="${s.kind==='node'?s.dir:s.module}"></div>
        <div class="field"><label>端口 port</label><input type="number" value="${s.port}"></div>
      </div>
      <div class="field"><label>grace_secs</label><input type="number" value="${s.grace}"></div>
      <div class="field"><label>depends_on（多选）</label><div class="kv">${services.filter(o=>o.id!==s.id).map(o=>`<span class="chip" style="${s.depends.includes(o.id)?'background:var(--accent-tint);color:var(--accent-hover);border-color:#DCDFF6':''}">${o.id}</span>`).join('')}</div></div>
    </div>`).join('');
  return `<div class="config-wrap">
    <div style="display:flex;gap:.6rem;margin-bottom:1rem;align-items:center">
      <div class="seg" id="cfgSeg"><button class="active" data-cfg="form">表单</button><button data-cfg="yaml">原文</button></div>
      <span class="spacer" style="flex:1"></span>
      <button class="btn ghost" data-act="scan" ${services.length?'disabled':''}>${ic('refresh')} 重新扫描并合并</button>
    </div>
    <div id="cfgForm">${svcBlocks}
      <button class="btn" data-act="svc-add">${ic('plus')} 添加服务</button>
    </div>
    <div id="cfgYaml" style="display:none">
      <div class="banner warn">${ic('warn')} 表单保存会丢失注释与键顺序；改动复杂结构请用原文。原文保存原样写盘并解析，失败标行号。</div>
      <div class="yaml"><textarea spellcheck="false">${esc(appState.yamlText)}</textarea></div>
      <button class="btn primary" data-act="yaml-save" style="margin-top:.7rem">${ic('check')} 保存原文</button>
    </div>
  </div>`;
}
function bindConfig(){
  $$('#cfgSeg button').forEach(b=>b.addEventListener('click',()=>{
    $$('#cfgSeg button').forEach(x=>x.classList.remove('active')); b.classList.add('active');
    const yaml = b.dataset.cfg==='yaml';
    $('#cfgForm').style.display = yaml?'none':'block';
    $('#cfgYaml').style.display = yaml?'block':'none';
  }));
}

/* ---------- 环境页（探测 live + 安装 soon） ---------- */
function renderEnv(){
  return `<div class="config-wrap">
    <div class="set-group"><h3>工具链探测（live）</h3>
      ${[['JDK','17','Java(TM) SE 17.0.9'],['Maven','3.9','Apache Maven 3.9.5'],['Node','20','v20.11.0'],['pnpm','9','9.1.0']].map(([n,v,d])=>`
        <div class="set-row"><span class="row-icn">${ic('chip')}</span>
          <div><div class="lbl">${n}</div><div class="desc">${d}</div></div>
          <div class="ctrl"><span class="chip"><span class="dot running" style="width:.4rem;height:.4rem"></span>已安装 ${v}</span></div></div>`).join('')}
    </div>
    <div class="cs" style="min-height:34vh"><div class="cs-card"><div class="cs-ic">${ic('download')}</div>
      <h2>安装与升级</h2><p>1.2 将通过 mise / winget 一键安装、升级 JDK · Maven · Node，并切换版本。</p>
      <span class="cs-ver">即将 1.2</span></div></div>
  </div>`;
}

/* ---------- 设置页 ---------- */
function renderSettings(){
  const grp=(h,rows)=>`<div class="set-group"><h3>${h}</h3>${rows}</div>`;
  const row=(lbl,desc,ctrl)=>`<div class="set-row"><div><div class="lbl">${lbl}</div>${desc?`<div class="desc">${desc}</div>`:''}</div><div class="ctrl">${ctrl}</div></div>`;
  const tog=on=>`<button class="toggle ${on?'on':''}" data-act="toggle" aria-pressed="${on}"></button>`;
  return `<div class="settings-wrap">
    ${grp('常规', row('打开最后工作区',null,tog(true))+row('启动后自动进运行页',null,tog(true)))}
    ${grp('外观', row('亮 / 暗跟随系统',null,`<select style="padding:.35rem .5rem;border:1px solid var(--line);border-radius:var(--r-sm);background:var(--surface)"><option>跟随系统</option><option>浅色</option><option>深色（2.x）</option></select>`))}
    ${grp('工具链 <span class="soon-tag">即将 1.2</span>', row('一键安装 / 升级',null,`<span class="soon-tag">1.2</span>`))}
    ${grp('网络代理 <span class="soon-tag">即将 1.2</span>', row('HTTP / npm / Maven 镜像',null,`<span class="soon-tag">1.2</span>`))}
    ${grp('更新 <span class="soon-tag">即将 1.1</span>', row('当前版本','v1.0.0',`<span class="soon-tag">1.1 自动更新</span>`))}
    ${grp('账号 <span class="soon-tag">即将 2.0</span>', row('登录以同步工作区与模板',null,`<button class="btn sm" disabled>${ic('key')} 登录</button>`))}
    ${grp('关于', row('SuperTask','本机优先的多服务可视化工作台 · Tauri 2 + Rust + React',`<span class="chip">v1.0.0</span>`)+row('许可','MIT',`<a class="chip" href="#" onclick="return false">${ic('book')} 文档</a>`))}
  </div>`;
}

/* ---------- 欢迎页 ---------- */
function renderWelcome(){
  const recent = [{p:'D:/dev/mall',t:'2 小时前'},{p:'D:/dev/order-svc',t:'昨天'},{p:'D:/dev/blog',t:'3 天前'}];
  return `<div class="welcome"><div class="welcome-card">
    <div class="brand"><div class="logo">${ic('layers')}</div><div><h1>SuperTask</h1><p>本机优先的多服务可视化工作台</p></div></div>
    <div class="welcome-actions">
      <button class="btn primary" data-act="add-ws">${ic('plus')} 添加工作区</button>
      <button class="btn" data-act="scan-demo">${ic('refresh')} 扫描当前目录</button>
    </div>
    <div class="recent-h">最近工作区</div>
    <div class="recent-list">${recent.map(r=>`<div class="recent-item" data-act="open-recent" data-path="${r.p}">
      <div class="ws-mark" style="width:1.4rem;height:1.4rem;font-size:.7rem">M</div>
      <div class="meta"><div class="ws-name" style="font-size:.82rem">${r.p.split('/').pop()}</div><div class="rp">${r.p}</div></div>
      <span class="rt" style="margin-left:auto">${r.t}</span></div>`).join('')}</div>
  </div></div>`;
}

/* ---------- ComingSoon ---------- */
function renderComingSoon(f){
  const v = f?f.since:'1.x';
  const desc = { '/templates':'官方模板库：Spring 多模块 + Node 最少两套，一键生成工作区。',
    '/git':'git clone / pull 与分支、脏状态展示。', '/docker':'compose 起 Redis / MySQL sidecar，镜像 build / tag。',
    '/gateway':'nginx / apache / caddy 模板与本机校验，服务端口 → 反代路由可视化。',
    '/cloud':'账号登录、工作区 / 模板 / 密钥策略同步（密钥默认不同步）。',
    '/ai':'读取 README / 脚本生成 YAML 草稿；解释日志、改端口、补健康检查。' };
  return `<div class="cs"><div class="cs-card">
    <div class="cs-ic">${ic(f?f.icon:'spark')}</div>
    <h2>${f?f.label:'即将推出'}</h2>
    <p>${desc[appState.route]||'该功能将在后续版本提供。'}</p>
    <span class="cs-ver">即将 ${v}</span>
  </div></div>`;
}

/* ---------- 命令面板 ---------- */
let cpIndex=0, cpItems=[];
function openCP(){ $('#cp').classList.add('open'); $('#cpBack').classList.add('open'); $('#cpInput').value=''; $('#cpInput').focus(); renderCP(''); }
function closeCP(){ $('#cp').classList.remove('open'); $('#cpBack').classList.remove('open'); }
function renderCP(q){
  q=q.trim().toLowerCase();
  const navLive = FEATURES.filter(f=>!f.sep && (f.status==='live') && (!q||f.label.toLowerCase().includes(q)||f.id.includes(q)));
  const navSoon = FEATURES.filter(f=>!f.sep && f.status==='soon' && (!q||f.label.toLowerCase().includes(q)));
  const acts = [{label:'启动全部',icon:'play',act:'start-all'},{label:'停止全部',icon:'stop',act:'stop-all'},{label:'打开目录',icon:'folder',act:'open-dir'}]
    .filter(a=>!q||a.label.includes(q));
  cpItems=[];
  let html='';
  if(navLive.length){ html+='<div class="cp-group">导航</div>'; navLive.forEach(f=>{cpItems.push({route:f.path}); html+=`<div class="cp-item" data-idx="${cpItems.length-1}" data-route="${f.path}">${ic(f.icon)}<span class="nm">${f.label}</span><span class="hint">${f.path}</span></div>`;});}
  if(acts.length){ html+='<div class="cp-group">操作</div>'; acts.forEach(a=>{cpItems.push({act:a.act}); html+=`<div class="cp-item" data-idx="${cpItems.length-1}" data-act="${a.act}">${ic(a.icon)}<span class="nm">${a.label}</span></div>`;});}
  if(navSoon.length){ html+='<div class="cp-group">即将推出</div>'; navSoon.forEach(f=>{cpItems.push({soon:f.since}); html+=`<div class="cp-item" data-idx="${cpItems.length-1}" data-soon="${f.since}">${ic(f.icon)}<span class="nm">${f.label}</span><span class="hint">即将 ${f.since}</span></div>`;});}
  if(!html) html='<div class="cp-group">无匹配</div>';
  $('#cpList').innerHTML=html;
  cpIndex=0; highlightCP();
}
function highlightCP(){ $$('#cpList .cp-item').forEach((el,i)=>el.classList.toggle('active',i===cpIndex)); }
function cpExec(){
  const el = $$('#cpList .cp-item')[cpIndex]; if(!el) return;
  if(el.dataset.route){ location.hash=el.dataset.route; closeCP(); }
  else if(el.dataset.act){ closeCP(); doAction(el.dataset.act); }
  else if(el.dataset.soon){ closeCP(); toast('info',`「${el.querySelector('.nm').textContent}」将在 ${el.dataset.soon} 提供`); }
}

/* ---------- Toast ---------- */
function toast(type,msg,act){
  const t=document.createElement('div'); t.className='toast '+type;
  t.innerHTML=`${ic(type==='ok'?'check':type==='warn'?'warn':type==='err'?'warn':'info')}<span class="msg">${msg}</span>${act?`<button class="act">${act.t}</button>`:''}`;
  if(act) t.querySelector('.act').addEventListener('click',()=>{ act.fn(); t.remove(); });
  $('#toasts').appendChild(t);
  setTimeout(()=>{ t.style.opacity='0'; t.style.transform='translateX(12px)'; setTimeout(()=>t.remove(),220); }, 3200);
}

/* ---------- 状态机模拟 ---------- */
function findSvc(id){ return services.find(s=>s.id===id); }
function doAction(act, id){
  if(act==='start-all'){ services.filter(s=>s.state==='stopped'||s.state==='exited').forEach((s,i)=>setTimeout(()=>startOne(s.id),i*250)); toast('info','正在启动全部服务…'); }
  else if(act==='stop-all'){ [...services].reverse().forEach((s,i)=>setTimeout(()=>stopOne(s.id),i*200)); toast('info','正在停止全部服务…'); }
  else if(act==='start'){ startOne(id); }
  else if(act==='stop'){ stopOne(id); }
  else if(act==='restart'){ stopOne(id); setTimeout(()=>startOne(id),700); }
  else if(act==='open-dir'){ toast('info',`已用资源管理器打开 <b>D:/dev/mall</b>`); }
  else if(act==='scan'){ openScanWizard(); }
  else if(act==='add-ws'){ openScanWizard(); }
}
function startOne(id){
  const s=findSvc(id); if(!s) return; s.state='starting'; rerenderCurrent();
  setTimeout(()=>{ s.state='running'; s.pid=18000+Math.floor(Math.random()*900); s.startedAt=Date.now(); s.health={ok:true,at:nowTime(),ms:30+Math.floor(Math.random()*40),type:s.health?.type||'http'}; rerenderCurrent(); renderStatus(); toast('ok',`<b>${s.id}</b> 已启动`); }, 1100);
}
function stopOne(id){
  const s=findSvc(id); if(!s) return; if(s.state!=='running'&&s.state!=='unhealthy'&&s.state!=='starting') return;
  s.state='stopping'; rerenderCurrent();
  setTimeout(()=>{ s.state='stopped'; s.pid=null; rerenderCurrent(); renderStatus(); toast('info',`<b>${s.id}</b> 已停止`); }, 800);
}
function nowTime(){ const d=new Date(); return String(d.getHours()).padStart(2,'0')+':'+String(d.getMinutes()).padStart(2,'0')+':'+String(d.getSeconds()).padStart(2,'0'); }
function rerenderCurrent(){ if(appState.route==='/run'){ $('#view').innerHTML=renderRun(); } else if(appState.route==='/logs'){ $('#view').innerHTML=renderLogs(); } }

/* ---------- 扫描向导 ---------- */
function openScanWizard(){
  const found=[{id:'gateway',kind:'spring-boot',port:8080,sub:'modules/gateway · Spring Boot 3.2'},{id:'user-api',kind:'spring-boot',port:8081,sub:'modules/user-service · Spring Boot 3.2'},{id:'web',kind:'node',port:5173,sub:'web/ · pnpm · dev',warn:'依赖全部 Spring 服务'}];
  const items=found.map(f=>`<div class="scan-item"><div class="meta"><div class="id">${f.id}</div><div class="sub">${f.sub}</div>${f.warn?`<div class="warn">${ic('warn')} ${f.warn}</div>`:''}</div><span class="chip">:${f.port}</span></div>`).join('');
  $('#modal').innerHTML=`<div class="modal-head"><span class="row-icn">${ic('search')}</span><h3>扫描草稿预览</h3><button class="icon-btn close" id="mClose" aria-label="关闭">${ic('close')}</button></div>
    <div class="modal-body"><p style="color:var(--t2);font-size:.8rem;margin-bottom:.8rem">检测到 2 个 Spring Boot 模块 + 1 个 Node 包。确认后写入 <b>supertask.yaml</b>。</p>${items}
      <div class="banner warn" style="margin-top:.6rem">${ic('warn')} 扫描只覆盖根 pom 的 modules + 一层子目录 package.json。可在确认前调整。</div></div>
    <div class="modal-foot"><button class="btn ghost" id="mCancel">取消</button><span class="spacer"></span><button class="btn primary" id="mConfirm">${ic('check')} 确认写入并进入运行页</button></div>`;
  $('#modalBack').classList.add('open');
  $('#mClose').onclick=closeModal; $('#mCancel').onclick=closeModal;
  $('#mConfirm').onclick=()=>{ closeModal(); location.hash='/run'; toast('ok','草稿已写入 <b>supertask.yaml</b>'); };
}
function closeModal(){ $('#modalBack').classList.remove('open'); }

/* ---------- 全局事件委托 ---------- */
document.addEventListener('click',e=>{
  const t=e.target.closest('[data-act],[data-route],[data-tab],[data-svc]');
  if(!t) return;
  if(t.dataset.route){ location.hash=t.dataset.route; return; }
  if(t.dataset.act){
    const a=t.dataset.act, svc=t.dataset.svc;
    if(a==='select'){ appState.selected=svc; $('#view').innerHTML=renderRun(); return; }
    if(a==='close-drawer'){ appState.selected=null; $('#view').innerHTML=renderRun(); return; }
    if(a==='tab'){ appState.drawerTab=t.dataset.tab; const d=$('#drawer'); if(d) d.outerHTML=drawerHTML(); return; }
    if(a==='select-log'){ appState.selected=svc==='__bootstrap'?null:svc; $('#view').innerHTML=renderLogs(); return; }
    if(a==='open-recent'){ location.hash='/run'; toast('ok','已打开工作区'); return; }
    if(a==='scan-demo'){ openScanWizard(); return; }
    if(a==='log-clear'){ const v=t.closest('.tab-body,.logs-main')?.querySelector('.logview'); if(v) v.innerHTML='<span class="ln"><span class="ts">--:--:--</span> <span class="st-out">[已清屏 · 仅清内存视图，文件未动]</span></span>'; return; }
    if(a==='log-pause'){ toast('info','日志跟随已暂停'); return; }
    if(a==='env-save'){ toast('warn','已写入 YAML · 该服务需重启生效',{t:'重启',fn:()=>{ if(appState.selected) doAction('restart',appState.selected); }}); return; }
    if(a==='yaml-save'){ toast('ok','原文已保存'); return; }
    if(a==='toggle'){ t.classList.toggle('on'); t.setAttribute('aria-pressed',t.classList.contains('on')); return; }
    if(['start','stop','restart','start-all','stop-all','open-dir','scan','add-ws'].includes(a)){ doAction(a,svc); }
  }
});
// 侧栏折叠
$('#collapseBtn').addEventListener('click',()=>{ $('#sidebar').classList.toggle('collapsed'); });
// 工作区下拉（演示）
$('#wsBtn').addEventListener('click',()=>toast('info','工作区切换（占位）'));
// 账号
$('#acctBtn').addEventListener('click',()=>toast('info','账号登录将在 <b>2.0</b> 提供'));
// 搜索触发 → 命令面板
$('#searchTrigger').addEventListener('click',openCP);
$('#searchTrigger').addEventListener('keydown',e=>{ if(e.key==='Enter'||e.key===' ') {e.preventDefault();openCP();} });
// 命令面板
$('#cpBack').addEventListener('click',closeCP);
$('#cpInput').addEventListener('input',e=>{ renderCP(e.target.value); });
$('#cpList').addEventListener('click',e=>{ const it=e.target.closest('.cp-item'); if(!it) return; cpIndex=+it.dataset.idx; cpExec(); });
// 键盘
document.addEventListener('keydown',e=>{
  if((e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='k'){ e.preventDefault(); $('#cp').classList.contains('open')?closeCP():openCP(); return; }
  if(e.key==='Escape'){ closeCP(); closeModal(); const d=$('#drawer'); if(d&&appState.route==='/run'){ appState.selected=null; $('#view').innerHTML=renderRun(); } return; }
  if($('#cp').classList.contains('open')){
    if(e.key==='ArrowDown'){ e.preventDefault(); cpIndex=Math.min(cpIndex+1,cpItems.length-1); highlightCP(); }
    else if(e.key==='ArrowUp'){ e.preventDefault(); cpIndex=Math.max(cpIndex-1,0); highlightCP(); }
    else if(e.key==='Enter'){ e.preventDefault(); cpExec(); }
  }
});
window.addEventListener('hashchange',router);

/* ---------- 启动 ---------- */
renderNav(); renderStatus(); router();
