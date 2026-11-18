// ModNest UI. Library of games -> a game page with Installed / Browse / Modpacks / Profiles.
// All real work happens in Rust; this file only draws screens and calls commands.
'use strict';

const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const listen = (ev, fn) => T.event.listen(ev, (e) => fn(e.payload));

const $ = (sel, el = document) => el.querySelector(sel);
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const fmt = (n) => (n >= 1e6 ? (n / 1e6).toFixed(1) + 'M' : n >= 1e3 ? (n / 1e3).toFixed(1) + 'k' : String(n ?? 0));
const ago = (iso) => {
  if (!iso) return '';
  const d = (Date.now() - new Date(iso)) / 86400000;
  return d < 1 ? 'today' : d < 2 ? 'yesterday' : d < 31 ? `${Math.floor(d)}d ago` : d < 365 ? `${Math.floor(d / 30)}mo ago` : `${Math.floor(d / 365)}y ago`;
};
const plural = (n, w) => `${n} ${w}${n === 1 ? '' : 's'}`;
const svg = (d, cls = '') => `<svg class="${cls}" viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="${d}"/></svg>`;
const I = {
  chev: svg('M7.4 8.6 12 13.2l4.6-4.6L18 10l-6 6-6-6z', 'chev'),
  caret: svg('M7 10l5 5 5-5z'),
  play: svg('M8 5v14l11-7z'),
  dl: svg('M5 20h14v-2H5zM19 9h-4V3H9v6H5l7 7z'),
  like: svg('M12 21.4 10.6 20C5.4 15.4 2 12.3 2 8.5 2 5.4 4.4 3 7.5 3c1.7 0 3.4.8 4.5 2.1C13.1 3.8 14.8 3 16.5 3 19.6 3 22 5.4 22 8.5c0 3.8-3.4 6.9-8.6 11.5z'),
  search: svg('M15.5 14h-.8l-.3-.3A6.5 6.5 0 1 0 14 15.5l.3.3v.8l5 5 1.5-1.5zm-6 0a4.5 4.5 0 1 1 0-9 4.5 4.5 0 0 1 0 9z'),
  grid: svg('M3 3h8v8H3zm10 0h8v8h-8zM3 13h8v8H3zm10 0h8v8h-8z'),
  list: svg('M3 5h18v3H3zm0 5.5h18v3H3zM3 16h18v3H3z'),
  library: svg('M4 6H2v14a2 2 0 0 0 2 2h14v-2H4zm16-4H8a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V4a2 2 0 0 0-2-2zm-1 9h-4v4h-2v-4H9V9h4V5h2v4h4z'),
  gear: svg('M19.4 13a7.6 7.6 0 0 0 0-2l2-1.6-2-3.4-2.4 1a7 7 0 0 0-1.7-1L15 3.5h-4l-.4 2.5a7 7 0 0 0-1.7 1l-2.4-1-2 3.4L6.6 11a7.6 7.6 0 0 0 0 2l-2 1.6 2 3.4 2.4-1a7 7 0 0 0 1.7 1l.4 2.5h4l.4-2.5a7 7 0 0 0 1.7-1l2.4 1 2-3.4zM12 15.5a3.5 3.5 0 1 1 0-7 3.5 3.5 0 0 1 0 7z'),
  layers: svg('M12 16.5 3.7 10 2 11.3l10 7.8 10-7.8-1.7-1.3zm0-2.6 8.3-6.4L22 6.2 12-1.6 2 6.2l1.6 1.3z'),
  check: svg('M9 16.2 4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4z', 'tick'),
};

const ui = {
  view: 'library', // library | game
  games: [],
  settings: { autoUpdateMods: true, autoInstallLoader: true },
  version: '',
  libScope: 'mine',
  libFilter: '',
  gameId: null,
  state: null,
  tab: 'installed', // installed | browse | modpacks | profiles
  instFilter: '',
  layout: (() => { try { return localStorage.getItem('modnest.layout') || 'list'; } catch { return 'list'; } })(),
  open: new Set(),
  browse: null,
  filters: {},
  busy: new Set(),
  autoDone: new Set(),
  menu: null,
};

// ---------- feedback ----------
document.addEventListener('error', (e) => { if (e.target.tagName === 'IMG') e.target.classList.add('broken'); }, true);

const toasts = new Map();
function toast(id, title, { text = '', kind = '', fraction = null, ttl = 0 } = {}) {
  let el = toasts.get(id);
  if (!el) { el = document.createElement('div'); $('#toasts').appendChild(el); toasts.set(id, el); }
  el.className = `toast ${kind}`;
  const bar = fraction === null ? '' : `<div class="bar ${fraction < 0 ? 'indeterminate' : ''}"><i style="width:${Math.max(0, fraction) * 100}%"></i></div>`;
  el.innerHTML = `<div class="toast-title">${esc(title)}</div>${text ? `<div class="muted small">${esc(text)}</div>` : ''}${bar}`;
  if (ttl) setTimeout(() => { el.remove(); toasts.delete(id); }, ttl);
}
const fail = (e) => toast('err' + Date.now(), 'Something went wrong', { text: String(e), kind: 'error', ttl: 10000 });

listen('progress', (p) => {
  const id = 'p:' + p.key;
  if (p.stage === 'done') toast(id, p.title, { text: 'Installed', kind: 'ok', ttl: 2200 });
  else if (p.stage === 'resolve') toast(id, 'Getting mod info…', { fraction: -1 });
  else toast(id, `${p.stage === 'install' ? 'Installing' : 'Downloading'} ${p.title}`, { fraction: p.stage === 'install' ? -1 : p.fraction });
});

function modal(html) {
  const dlg = $('#modal');
  $('#modal-body').innerHTML = html;
  dlg.showModal();
  return new Promise((resolve) => dlg.addEventListener('close', () => resolve(dlg.returnValue), { once: true }));
}
async function ask(title, label, value = '', ok = 'OK') {
  const r = await modal(`<header>${esc(title)}</header><section class="field"><label class="muted small">${label}</label>
    <input class="input" type="text" id="ask-input" value="${esc(value)}" autocomplete="off" /></section>
    <footer><button class="btn" value="">Cancel</button><button class="btn primary" value="ok" id="ask-ok">${esc(ok)}</button></footer>`);
  return r === 'ok' ? ($('#ask-input').value || '').trim() : null;
}
async function confirmBox(title, html, ok = 'OK') {
  return (await modal(`<header>${esc(title)}</header><section>${html}</section>
    <footer><button class="btn" value="">Cancel</button><button class="btn primary" value="ok">${esc(ok)}</button></footer>`)) === 'ok';
}
document.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && e.target.id === 'ask-input') { e.preventDefault(); $('#ask-ok').click(); }
  if (e.key === 'Escape' && ui.menu) closeMenu();
});
async function copy(text) {
  try { await navigator.clipboard.writeText(text); return true; } catch {
    const t = document.createElement('textarea'); t.value = text; document.body.appendChild(t); t.select();
    const ok = document.execCommand('copy'); t.remove(); return ok;
  }
}
const ico = (m, cls = 'ico') => (m.icon ? `<img class="${cls}" src="${esc(m.icon)}" alt="" loading="lazy" />` : `<div class="${cls} ${cls === 'thumb' ? 'letter' : ''}">${esc((m.title || '?').slice(0, 1).toUpperCase())}</div>`);

// =====================================================================
// Rail
// =====================================================================
function renderRail() {
  const mine = ui.games.filter((g) => g.installed && (g.source === 'own' || g.modCount));
  $('#rail').innerHTML = `
    <img class="rail-logo" src="logo.png" alt="ModNest" title="ModNest" />
    <div class="rail-sep"></div>
    <button class="rail-btn ${ui.view === 'library' ? 'active' : ''}" data-act="library" title="Library">${I.library}</button>
    <div class="rail-games">${mine.map((g) => `
      <button class="rail-btn ${ui.view === 'game' && ui.gameId === g.id ? 'active' : ''}" data-game="${esc(g.id)}" title="${esc(g.name)}">
        <img src="${esc(g.capsule || g.cover || '')}" alt="" />
      </button>`).join('')}</div>
    <button class="rail-btn" data-act="settings" title="Settings">${I.gear}</button>`;
}

// =====================================================================
// Library
// =====================================================================
function showLibrary() {
  ui.view = 'library';
  ui.state = null;
  renderRail();
  renderLibrary();
}

function renderLibrary() {
  const f = ui.libFilter.toLowerCase();
  let list = ui.games.filter((g) => !f || g.name.toLowerCase().includes(f));
  if (ui.libScope === 'mine') list = list.filter((g) => g.installed);
  const card = (g) => `
    <button class="cover-card ${g.installed ? '' : 'dim'}" data-game="${esc(g.id)}" title="${esc(g.name)}">
      <div class="fallback">${esc(g.name)}</div>
      <img src="${esc(g.capsule || g.cover || '')}" alt="" loading="lazy" style="position:relative" />
      <div class="shade"></div>
      ${g.modCount ? `<span class="pill blue badge">${plural(g.modCount, 'mod')}</span>` : ''}
      <div class="meta"><div class="name">${esc(g.name)}</div><div class="sub">${g.installed ? esc(g.loader) : 'Not installed'}</div></div>
    </button>`;
  const own = list.filter((g) => g.source === 'own');
  const ts = list.filter((g) => g.source !== 'own');
  $('#main').innerHTML = `
    <div class="lib-head">
      <div><h1>Library</h1><p>Pick a game to manage its mods.</p></div>
      <span style="flex:1"></span>
      <div class="seg">
        <button data-scope="mine" class="${ui.libScope === 'mine' ? 'active' : ''}">Installed</button>
        <button data-scope="all" class="${ui.libScope === 'all' ? 'active' : ''}">All supported</button>
      </div>
    </div>
    <div class="lib-tools"><label class="search">${I.search}<input class="input" type="search" id="lib-filter" placeholder="Search games" value="${esc(ui.libFilter)}" autocomplete="off" /></label></div>
    <div class="scroll">
      ${own.length ? `<div class="group-title">Your mods <span class="pill line">ModNest catalog</span></div><div class="cover-grid">${own.map(card).join('')}</div>` : ''}
      ${ts.length ? `<div class="group-title">Community mods <span class="pill line">from Thunderstore</span></div><div class="cover-grid">${ts.map(card).join('')}</div>` : ''}
      ${!list.length ? '<div class="empty">No games found. Try “All supported”.</div>' : ''}
      <div style="height:24px"></div>
    </div>`;
  const input = $('#lib-filter');
  input.addEventListener('input', () => { ui.libFilter = input.value; const p = input.selectionStart; renderLibrary(); const i = $('#lib-filter'); i.focus(); i.setSelectionRange(p, p); });
}

// =====================================================================
// Game page
// =====================================================================
async function openGame(id, tab = 'installed') {
  closeMenu();
  ui.view = 'game';
  ui.gameId = id;
  ui.state = null;
  ui.browse = null;
  ui.tab = tab;
  ui.open.clear();
  ui.instFilter = '';
  renderRail();
  $('#main').innerHTML = '<div class="center"><div class="spinner"></div></div>';
  try {
    let st = await invoke('game_state', { gameId: id });
    // Every game starts with a "Default" profile holding whatever is installed now.
    if (!st.profiles.length && st.game.installed) st = await invoke('save_profile', { gameId: id, name: 'Default' });
    if (ui.gameId !== id) return;
    ui.state = st;
    renderGame();
    autoMaintain(id);
  } catch (e) { fail(e); showLibrary(); }
}

// Loader for your own games + keep mods updated; once per game per session.
async function autoMaintain(id) {
  if (ui.autoDone.has(id) || !ui.state?.game.installed) return;
  ui.autoDone.add(id);
  try {
    const wantsLoader = ui.state.game.source === 'own' || ui.state.mods.length > 0;
    if (ui.settings.autoInstallLoader && wantsLoader && ui.state.baseMissing.length) setState(await invoke('ensure_base', { gameId: id }));
    const st = await invoke('check_updates', { gameId: id });
    setState(st);
    const outdated = st.mods.filter((m) => m.updateAvailable).map((m) => m.key);
    if (ui.settings.autoUpdateMods && outdated.length) {
      setState(await invoke('update_mods', { gameId: id, keys: outdated }));
      toast('upd' + id, `Updated ${plural(outdated.length, 'mod')}`, { kind: 'ok', ttl: 4000 });
    }
  } catch (e) { fail(e); }
}

function setState(st) {
  if (!st || st.game.id !== ui.gameId) return;
  ui.state = st;
  const g = ui.games.find((x) => x.id === st.game.id);
  if (g) Object.assign(g, st.game);
  if (ui.view === 'game') { renderRail(); renderGame(); }
}

function renderGame() {
  const { game: g, mods } = ui.state;
  const updates = mods.filter((m) => m.updateAvailable).length;
  const tab = (id, label, n, amber) => `<button class="tab ${ui.tab === id ? 'active' : ''}" data-tab="${id}">${label}${n != null ? ` <span class="n ${amber ? 'amber' : ''}">${n}</span>` : ''}</button>`;
  $('#main').innerHTML = `
    <section class="banner">
      <div class="art" style="background-image:url('${esc(g.hero || g.cover || '')}')"></div>
      <div class="banner-body">
        <img class="banner-cover" src="${esc(g.capsule || g.cover || '')}" alt="" />
        <div style="min-width:0">
          <h1>${esc(g.name)}</h1>
          <div class="chips">
            <span class="pill">${esc(g.loader)}</span>
            ${g.installed ? (ui.state.baseMissing.length ? '<span class="pill amber">Loader not set up</span>' : '<span class="pill green">Ready</span>') : '<span class="pill red">Game not found</span>'}
            ${updates ? `<span class="pill amber">${plural(updates, 'update')}</span>` : ''}
            ${ui.state.steamSwitchable ? `<span class="pill line" title="What happens when you start the game from Steam">${ui.state.steamVanilla ? 'Steam starts it without mods' : 'Steam starts it with mods'}</span>` : ''}
          </div>
        </div>
        <div class="banner-actions">
          <div style="position:relative">
            <button class="btn glass profile-btn" data-act="profile-menu" title="Switch profile">${I.layers.replace('<svg', '<svg width="15" height="15"')}<span>${esc(g.activeProfile || 'No profile')}</span>${I.caret.replace('<svg', '<svg width="16" height="16"')}</button>
            ${ui.menu === 'profile' ? profileMenu() : ''}
          </div>
          <div class="play-split">
            <button class="btn play" data-act="play" ${g.installed && g.appId ? '' : 'disabled'}>${I.play.replace('<svg', '<svg width="16" height="16"')} Play</button>
            <button class="btn play-more" data-act="play-menu" ${g.installed && g.appId ? '' : 'disabled'} title="More">${I.caret.replace('<svg', '<svg width="18" height="18"')}</button>
            ${ui.menu === 'play' ? `<div class="menu-pop">
              <button data-act="play-vanilla">${I.play.replace('<svg', '<svg width="14" height="14"')} Play without mods</button>
              ${ui.state.steamSwitchable ? `<hr/><div class="label">Starting from Steam</div>
              <button data-act="steam-mode" data-v="1">${ui.state.steamVanilla ? I.check : '<span style="width:14px"></span>'} Normal game (mods only via ModNest)</button>
              <button data-act="steam-mode" data-v="0">${ui.state.steamVanilla ? '<span style="width:14px"></span>' : I.check} With mods</button>` : ''}
              <hr/>
              <button data-act="file">Install mod from file…</button>
              <button data-act="open-folder">Open game folder</button>
              ${g.installed ? '' : '<button data-act="folder">Set game folder…</button>'}
            </div>` : ''}
          </div>
        </div>
      </div>
    </section>
    <nav class="tabs">
      ${tab('installed', 'Installed', mods.length, false)}
      ${tab('browse', 'Browse')}
      ${tab('modpacks', 'Modpacks')}
      ${tab('profiles', 'Profiles', ui.state.profiles.length)}
    </nav>
    <div id="tab" class="scroll" style="display:flex;flex-direction:column"></div>`;
  renderTab();
}

function profileMenu() {
  const { profiles } = ui.state;
  return `<div class="menu-pop">
    <div class="label">Profiles</div>
    ${profiles.map((p) => `<button data-act="switch-profile" data-name="${esc(p.name)}">${p.active ? I.check : '<span style="width:14px"></span>'}<span style="flex:1">${esc(p.name)}</span><span class="muted small">${p.count}</span></button>`).join('')}
    <hr/>
    <button data-act="new-profile">New empty profile…</button>
    <button data-act="import-code">Import a code…</button>
    <button data-tab="profiles">Manage profiles</button>
  </div>`;
}

function closeMenu() {
  if (!ui.menu) return;
  ui.menu = null;
  if (ui.view === 'game' && ui.state) renderGame();
}

function notes() {
  const { game: g, baseMissing, mods } = ui.state;
  const updates = mods.filter((m) => m.updateAvailable);
  if (!g.installed) return `<div class="note amber"><span>ModNest can't find ${esc(g.name)} in Steam. Installed somewhere else?</span><button class="btn sm" data-act="folder">Set game folder</button></div>`;
  if (baseMissing.length) return `<div class="note blue"><span>${esc(g.loader)} is set up automatically with your first mod.</span><button class="btn sm" data-act="base">Set it up now</button></div>`;
  if (updates.length) return `<div class="note amber"><span>${plural(updates.length, 'mod')} can be updated.</span><button class="btn sm primary" data-act="update-all">Update all</button></div>`;
  return '';
}

function renderTab() {
  const c = $('#tab');
  if (!c) return;
  if (ui.tab === 'installed') return renderInstalled(c);
  if (ui.tab === 'profiles') return renderProfiles(c);
  renderBrowse(c);
}

// ---------- Installed ----------
function renderInstalled(c) {
  const { mods, game: g } = ui.state;
  const f = ui.instFilter.toLowerCase();
  const list = mods.filter((m) => !f || m.title.toLowerCase().includes(f) || (m.author || '').toLowerCase().includes(f));
  const titleOf = (k) => (mods.find((m) => m.key === k) || {}).title || k.replace(/^\w+:/, '').replace(/^[^-]+-/, '').replace(/_/g, ' ');
  c.innerHTML = `
    <div class="toolbar">
      <label class="search">${I.search}<input class="input" type="search" id="inst-filter" placeholder="Filter installed mods" value="${esc(ui.instFilter)}" autocomplete="off" /></label>
      <button class="btn" data-act="check">Check for updates</button>
    </div>
    ${notes()}
    <div class="rows">${list.map((m) => `
      <div class="row ${m.enabled ? '' : 'off'} ${ui.open.has(m.key) ? 'open' : ''}">
        <div class="row-head" data-open="${esc(m.key)}">
          ${ico(m)}
          <div class="row-main">
            <div class="row-title"><span class="t">${esc(m.title)}</span>${m.author ? `<span class="a">by ${esc(m.author)}</span>` : ''}</div>
            <div class="row-desc">${m.required ? 'Mod loader · always installed' : m.auto ? 'Needed by another mod' : m.fromFile ? 'Installed from a file' : m.adopted ? 'Found in the game folder' : esc(/^\d/.test(m.version) ? 'Version ' + m.version : m.version)}</div>
          </div>
          <div class="row-tags">
            ${m.required ? '<span class="pill blue">Base</span>' : ''}
            ${m.auto ? '<span class="pill line">Dependency</span>' : ''}
            ${m.updateAvailable ? `<span class="pill amber">→ ${esc(m.latest)}</span>` : ''}
          </div>
          <label class="switch" title="${m.required ? 'Always on' : m.enabled ? 'On' : 'Off'}" data-stop>
            <input type="checkbox" data-enable="${esc(m.key)}" ${m.enabled ? 'checked' : ''} ${m.required || !g.installed ? 'disabled' : ''} /><span></span>
          </label>
          ${I.chev}
        </div>
        <div class="row-body">
          ${m.dependencies.length ? `<p class="deps">Needs ${esc(m.dependencies.map(titleOf).join(', '))}</p>` : ''}
          <div class="actions">
            ${m.updateAvailable ? `<button class="btn sm primary" data-act="update" data-key="${esc(m.key)}">Update to ${esc(m.latest)}</button>` : ''}
            ${m.adopted && !m.required ? `<button class="btn sm" data-act="update" data-key="${esc(m.key)}">Reinstall latest</button>` : ''}
            ${m.required ? '' : `<button class="btn sm danger" data-act="remove" data-key="${esc(m.key)}">Uninstall</button>`}
            ${m.key.startsWith('ts:') ? `<button class="btn sm ghost" data-act="link" data-url="https://thunderstore.io/package/${esc(m.key.slice(3).replace('-', '/'))}/">Mod page ↗</button>` : ''}
          </div>
        </div>
      </div>`).join('')}
      ${!list.length ? `<div class="empty">${mods.length ? 'Nothing matches that filter.' : 'No mods yet.'}<br/><br/><button class="btn primary" data-tab="browse">Browse mods</button></div>` : ''}
    </div>`;
  const input = $('#inst-filter');
  input.addEventListener('input', () => { ui.instFilter = input.value; const p = input.selectionStart; renderInstalled(c); const i = $('#inst-filter'); i.focus(); i.setSelectionRange(p, p); });
}

// ---------- Browse & Modpacks ----------
function browseFor(tab) {
  if (!ui.browse || ui.browse.tab !== tab) ui.browse = { tab, q: '', ordering: 'most-downloaded', category: '', page: 1, items: [], total: 0, hasMore: false, loading: false, loaded: false };
  return ui.browse;
}
async function loadFilters() {
  if (!ui.filters[ui.gameId]) {
    try { ui.filters[ui.gameId] = await invoke('filters', { gameId: ui.gameId }); } catch { ui.filters[ui.gameId] = { sections: [], categories: [] }; }
  }
  return ui.filters[ui.gameId];
}

function renderBrowse(c) {
  const b = browseFor(ui.tab);
  const packs = ui.tab === 'modpacks';
  const isTs = ui.state.game.source !== 'own';
  const cats = (ui.filters[ui.gameId] || {}).categories || [];
  const grid = ui.layout === 'grid';
  c.innerHTML = `
    <div class="toolbar">
      <label class="search">${I.search}<input class="input" type="search" id="b-q" placeholder="Search ${packs ? 'modpacks' : 'mods'}" value="${esc(b.q)}" autocomplete="off" /></label>
      ${isTs ? `<select class="input" id="b-sort">${[['most-downloaded', 'Most downloaded'], ['top-rated', 'Most liked'], ['last-updated', 'Recently updated'], ['newest', 'Newest']]
        .map(([v, l]) => `<option value="${v}" ${b.ordering === v ? 'selected' : ''}>${l}</option>`).join('')}</select>` : ''}
      ${cats.length && !packs ? `<select class="input" id="b-cat"><option value="">All categories</option>${cats.map((k) => `<option value="${esc(k.id)}" ${b.category === k.id ? 'selected' : ''}>${esc(k.name)}</option>`).join('')}</select>` : ''}
      <div class="view-toggle">
        <button data-layout="list" class="${grid ? '' : 'active'}" title="List">${I.list}</button>
        <button data-layout="grid" class="${grid ? 'active' : ''}" title="Grid">${I.grid}</button>
      </div>
      ${b.total ? `<span class="count">${fmt(b.total)} ${packs ? 'modpacks' : 'mods'}</span>` : ''}
    </div>
    ${packs ? `<div class="note blue"><span>A modpack installs as its own profile. Your current mods stay saved in “${esc(ui.state.game.activeProfile || 'Default')}”, one click away.</span></div>` : notes()}
    <div id="b-list">${grid ? `<div class="cards">${b.items.map((m) => browseCard(m, packs)).join('')}</div>` : `<div class="rows">${b.items.map((m) => browseRow(m, packs)).join('')}</div>`}</div>
    ${b.loading ? '<div class="more"><div class="spinner"></div></div>' : b.hasMore ? '<div class="more"><button class="btn" data-act="more">Load more</button></div>' : ''}
    ${!b.loading && b.loaded && !b.items.length ? `<div class="empty">No ${packs ? 'modpacks' : 'mods'} found.</div>` : ''}`;
  const input = $('#b-q');
  let t;
  input.addEventListener('input', () => { clearTimeout(t); t = setTimeout(() => { b.q = input.value.trim(); loadBrowse(true); }, 350); });
  $('#b-sort')?.addEventListener('change', (e) => { b.ordering = e.target.value; loadBrowse(true); });
  $('#b-cat')?.addEventListener('change', (e) => { b.category = e.target.value; loadBrowse(true); });
  if (!b.loaded && !b.loading) loadBrowse(true);
}

function getButton(m, packs, small = true) {
  const busy = ui.busy.has(m.key);
  const can = ui.state.game.installed && !busy;
  if (m.installed) return '<span class="pill green">Installed</span>';
  const label = busy ? '<span class="spinner sm"></span>' : packs ? 'Install as profile' : `${I.dl.replace('<svg', '<svg width="14" height="14"')} Get`;
  return `<button class="btn ${small ? 'sm' : ''} primary" data-act="${packs ? 'install-pack' : 'install'}" data-key="${esc(m.key)}" data-title="${esc(m.title)}" ${can ? '' : 'disabled'}>${label}</button>`;
}

function browseRow(m, packs) {
  return `
    <div class="row ${ui.open.has('b:' + m.key) ? 'open' : ''}">
      <div class="row-head" data-open="b:${esc(m.key)}">
        ${ico(m)}
        <div class="row-main">
          <div class="row-title"><span class="t">${esc(m.title)}</span><span class="a">by ${esc(m.author)}</span></div>
          <div class="row-desc">${esc(m.description)}</div>
        </div>
        <div class="row-tags">${m.required ? '<span class="pill blue">Base</span>' : ''}${m.deprecated ? '<span class="pill red">Deprecated</span>' : ''}</div>
        <div class="row-stats">
          ${m.downloads != null ? `<span title="Downloads">${I.dl} ${fmt(m.downloads)}</span>` : ''}
          ${m.rating != null ? `<span title="Likes">${I.like} ${fmt(m.rating)}</span>` : ''}
          ${m.updated ? `<span>${ago(m.updated)}</span>` : m.version ? `<span>v${esc(m.version)}</span>` : ''}
        </div>
        <div class="row-act" data-stop>${getButton(m, packs)}</div>
        ${I.chev}
      </div>
      <div class="row-body">
        <p>${esc(m.description)}</p>
        <div class="actions">
          ${(m.categories || []).filter(Boolean).map((k) => `<span class="pill line">${esc(k)}</span>`).join('')}
          ${m.pageUrl || m.website ? `<button class="btn sm ghost" data-act="link" data-url="${esc(m.pageUrl || m.website)}">Mod page ↗</button>` : ''}
        </div>
      </div>
    </div>`;
}

function browseCard(m, packs) {
  return `
    <article class="card" title="${esc(m.description)}">
      ${ico(m, 'thumb')}
      <div class="body">
        <div class="t">${esc(m.title)}</div>
        <div class="a">by ${esc(m.author)}</div>
        <div class="d">${esc(m.description)}</div>
        <div class="foot">
          <span class="s">${m.downloads != null ? `${I.dl} ${fmt(m.downloads)}` : m.version ? 'v' + esc(m.version) : ''}</span>
          ${getButton(m, packs)}
        </div>
      </div>
    </article>`;
}

async function loadBrowse(reset) {
  const b = ui.browse;
  const gameId = ui.gameId;
  const filters = await loadFilters();
  if (reset) { b.page = 1; b.items = []; }
  b.loading = true;
  if (reset) renderTab();
  try {
    const own = ui.state.game.source === 'own';
    const slug = b.tab === 'modpacks' ? 'modpacks' : 'mods';
    const section = own ? slug : (filters.sections.find((s) => s.slug === slug) || {}).id || null;
    const r = await invoke('browse', { gameId, q: b.q, page: b.page, ordering: b.ordering, section, categories: b.category ? [b.category] : [] });
    if (gameId !== ui.gameId || ui.browse !== b) return;
    b.items = reset ? r.items : b.items.concat(r.items);
    b.hasMore = r.hasMore;
    b.total = r.total;
  } catch (e) { fail(e); }
  b.loading = false;
  b.loaded = true;
  if (ui.browse === b && ui.view === 'game' && (ui.tab === 'browse' || ui.tab === 'modpacks')) {
    const sc = $('#tab')?.scrollTop || 0;
    renderTab();
    const el = $('#tab'); if (el) el.scrollTop = sc;
  }
}

// ---------- Profiles ----------
async function renderProfiles(c) {
  const { game: g, profiles } = ui.state;
  const isTs = g.source !== 'own';
  c.innerHTML = `
    <div class="section"><h3>Profiles</h3><div class="muted small">Each profile is its own set of mods. Switching swaps the mods in your game folder; nothing is lost.</div></div>
    <div class="profile-grid">
      ${profiles.map((p) => `
        <div class="pcard ${p.active ? 'active' : ''}">
          <div class="top"><span class="nm">${esc(p.name)}</span>${p.active ? '<span class="pill blue">In use</span>' : ''}${p.modpack ? '<span class="pill line">Modpack</span>' : ''}</div>
          <div class="list">${p.count ? esc(p.mods.join(' · ')) : 'Empty'}</div>
          <div class="acts">
            ${p.active ? '' : `<button class="btn sm primary" data-act="switch-profile" data-name="${esc(p.name)}">Use</button>`}
            <button class="btn sm" data-act="share-profile" data-name="${esc(p.name)}">Share</button>
            <button class="btn sm ghost" data-act="rename-profile" data-name="${esc(p.name)}">Rename</button>
            <button class="btn sm ghost" data-act="delete-profile" data-name="${esc(p.name)}">Delete</button>
          </div>
        </div>`).join('')}
      <button class="pcard new" data-act="new-profile">+ New empty profile</button>
      <button class="pcard new" data-act="save-as">+ Copy current mods to a new profile</button>
    </div>
    <div class="share-box">
      <h3>Import a profile</h3>
      <div class="muted small">Paste a ModNest code${isTs ? ', or a Thunderstore / r2modman profile code' : ''}. It becomes a new profile and ModNest switches to it.</div>
      <textarea class="input" id="code-in" rows="2" placeholder="Paste a code"></textarea>
      <div class="row2"><button class="btn primary" data-act="import-pasted">Import</button></div>
    </div>
    <div id="mgr-import"></div>
    <div style="height:20px"></div>`;
  try {
    const list = (await invoke('manager_profiles')).filter((p) => p.gameId === ui.gameId);
    const box = $('#mgr-import');
    if (!list.length || !box) return;
    box.innerHTML = `<div class="share-box"><h3>Found in Thunderstore Mod Manager</h3><div class="muted small">Profiles you made in another mod manager on this PC.</div>
      ${list.map((p) => `<div class="row2"><b>${esc(p.name)}</b><span class="muted small" style="flex:1">${plural(p.count, 'mod')}</span><button class="btn sm" data-act="import-mgr" data-name="${esc(p.name)}">Import</button></div>`).join('')}</div>`;
  } catch { /* none */ }
}

async function shareDialog(name) {
  const isTs = ui.state.game.source !== 'own';
  const code = await invoke('export_profile', { gameId: ui.gameId, name });
  const copied = await copy(code);
  const r = await modal(`<header>Share “${esc(name)}”</header><section>
    <p class="muted small" style="margin-top:0">${copied ? 'Copied to your clipboard. ' : ''}Send this code to a friend. In ModNest they open the game, click the profile button, then <b>Import a code</b>. The mod list is inside the code itself, so there's no server involved.</p>
    <textarea class="input" rows="4" readonly id="share-code">${esc(code)}</textarea>
    ${isTs ? '<p class="muted small">Friend uses r2modman or Thunderstore Mod Manager? Get a short Thunderstore code instead (uploads the mod list to Thunderstore).</p><div id="ts-code"></div>' : ''}
  </section><footer>${isTs ? '<button class="btn" value="ts">Get Thunderstore code</button>' : ''}<button class="btn" value="copy">Copy again</button><button class="btn primary" value="">Done</button></footer>`);
  if (r === 'copy') { await copy(code); toast('cp', 'Code copied', { kind: 'ok', ttl: 2000 }); }
  if (r === 'ts') {
    toast('ts', 'Getting a Thunderstore code…', { fraction: -1 });
    const short = await invoke('share_thunderstore', { gameId: ui.gameId, name });
    await copy(short);
    toast('ts', `Thunderstore code: ${short}`, { text: 'Copied. It works in r2modman and Thunderstore Mod Manager too.', kind: 'ok', ttl: 10000 });
  }
}

async function switchProfile(name) {
  toast('sw', `Switching to “${name}”…`, { fraction: -1 });
  const r = await invoke('apply_profile', { gameId: ui.gameId, name });
  toast('sw', `Now using “${name}”`, { kind: 'ok', ttl: 2500 });
  if (r.failed.length) toast('swf', `${plural(r.failed.length, 'mod')} couldn't be installed`, { text: r.failed.join('\n'), kind: 'error', ttl: 12000 });
  ui.browse = null;
  return r.state;
}

async function importCode(code) {
  toast('imp', 'Importing profile…', { fraction: -1 });
  const r = await invoke('import_code', { code, gameId: ui.gameId });
  if (r.gameId !== ui.gameId) {
    toast('imp', `Imported “${r.name}”`, { text: 'That code was for another game, so it was added there.', kind: 'ok', ttl: 5000 });
    return;
  }
  toast('imp', `Imported “${r.name}”`, { kind: 'ok', ttl: 2500 });
  ui.tab = 'installed';
  setState(await switchProfile(r.name));
}

// ---------- settings ----------
async function settingsDialog() {
  const r = await modal(`<header>Settings</header><section>
    <label class="check"><input type="checkbox" id="set-loader" ${ui.settings.autoInstallLoader ? 'checked' : ''} />
      <span><b>Set up the mod loader automatically</b><br/><small class="muted">Your games (Chained Beasts, Iron Nest) get BepInEx and ChainedForge when you open them. Other games get theirs with the first mod you install.</small></span></label>
    <label class="check"><input type="checkbox" id="set-steam" ${ui.settings.steamVanilla ? 'checked' : ''} />
      <span><b>Mods only when started from ModNest</b><br/><small class="muted">Starting a game from Steam runs the normal game; ModNest's Play button runs it with mods. Change it per game in the Play menu. (BepInEx games; Schedule I always runs with mods.)</small></span></label>
    <label class="check"><input type="checkbox" id="set-update" ${ui.settings.autoUpdateMods ? 'checked' : ''} />
      <span><b>Keep mods up to date</b><br/><small class="muted">When you open a game, its mods update to the newest version.</small></span></label>
    <p class="muted small">Downloads are kept so switching profiles is instant. <a href="#" id="clear-cache" style="color:var(--accent-hi)">Clear download cache</a></p>
    <p class="muted small">ModNest v${esc(ui.version)} updates itself from GitHub. Mods download straight from GitHub or Thunderstore; nothing goes through a server of ours.</p>
  </section><footer><button class="btn" value="">Cancel</button><button class="btn primary" value="ok">Save</button></footer>`);
  if (r === 'ok') {
    ui.settings = { autoInstallLoader: $('#set-loader').checked, autoUpdateMods: $('#set-update').checked, steamVanilla: $('#set-steam').checked };
    try {
      await invoke('save_settings', { settings: ui.settings });
      if (ui.view === 'game') setState(await invoke('game_state', { gameId: ui.gameId }));
    } catch (e) { fail(e); }
  }
}
document.addEventListener('click', async (e) => {
  if (e.target.id !== 'clear-cache') return;
  e.preventDefault();
  try { const n = await invoke('clear_cache'); toast('cc', `Cleared ${(n / 1048576).toFixed(1)} MB`, { kind: 'ok', ttl: 3000 }); } catch (err) { fail(err); }
});

// =====================================================================
// Clicks
// =====================================================================
document.addEventListener('click', async (e) => {
  if (e.target.closest('#modal')) return;
  // Close an open dropdown when clicking anywhere else.
  if (ui.menu && !e.target.closest('.menu-pop') && !e.target.closest('[data-act="profile-menu"],[data-act="play-menu"]')) closeMenu();

  const gameBtn = e.target.closest('[data-game]');
  if (gameBtn) return openGame(gameBtn.dataset.game);
  const scope = e.target.closest('[data-scope]');
  if (scope) { ui.libScope = scope.dataset.scope; return renderLibrary(); }
  const tabBtn = e.target.closest('[data-tab]');
  if (tabBtn && ui.view === 'game') { ui.menu = null; ui.tab = tabBtn.dataset.tab; return renderGame(); }
  const lay = e.target.closest('[data-layout]');
  if (lay) { ui.layout = lay.dataset.layout; try { localStorage.setItem('modnest.layout', ui.layout); } catch { /* fine */ } return renderTab(); }
  if (!e.target.closest('[data-stop]') && !e.target.closest('[data-act]')) {
    const head = e.target.closest('[data-open]');
    if (head) { const k = head.dataset.open; ui.open.has(k) ? ui.open.delete(k) : ui.open.add(k); head.parentElement.classList.toggle('open'); return; }
  }

  const el = e.target.closest('[data-act]');
  if (!el) return;
  e.preventDefault();
  const gameId = ui.gameId;
  const key = el.dataset.key;
  const name = el.dataset.name;
  try {
    switch (el.dataset.act) {
      case 'library': showLibrary(); break;
      case 'settings': await settingsDialog(); break;
      case 'profile-menu': ui.menu = ui.menu === 'profile' ? null : 'profile'; renderGame(); break;
      case 'play-menu': ui.menu = ui.menu === 'play' ? null : 'play'; renderGame(); break;
      case 'play': await invoke('launch', { gameId, vanilla: false }); toast('play', 'Starting with mods…', { ttl: 3000 }); break;
      case 'steam-mode': {
        ui.menu = null;
        const vanilla = el.dataset.v === '1';
        setState(await invoke('set_steam_mode', { gameId, vanilla }));
        toast('sm', vanilla ? 'Steam now starts the normal game' : 'Steam now starts the game with mods', { text: vanilla ? 'Use Play in ModNest to play with mods.' : '', kind: 'ok', ttl: 3500 });
        break;
      }
      case 'play-vanilla': closeMenu(); await invoke('launch', { gameId, vanilla: true }); toast('play', 'Starting without mods…', { ttl: 3000 }); break;
      case 'open-folder': closeMenu(); await invoke('open_folder', { gameId }); break;
      case 'link': await invoke('open_url', { url: el.dataset.url }); break;
      case 'folder': { closeMenu(); const st = await invoke('set_game_folder', { gameId }); if (st) setState(st); break; }
      case 'file': { closeMenu(); const st = await invoke('install_file', { gameId }); if (st) { setState(st); toast('file', 'Mod installed from file', { kind: 'ok', ttl: 3000 }); } break; }
      case 'base': el.disabled = true; setState(await invoke('ensure_base', { gameId })); break;
      case 'check': el.disabled = true; setState(await invoke('check_updates', { gameId })); toast('chk', 'Checked for updates', { ttl: 2000 }); break;
      case 'update-all': el.disabled = true; setState(await invoke('update_mods', { gameId, keys: ui.state.mods.filter((m) => m.updateAvailable).map((m) => m.key) })); break;
      case 'update': el.disabled = true; setState(await invoke('update_mods', { gameId, keys: [key] })); break;
      case 'more': ui.browse.page += 1; loadBrowse(false); break;
      case 'remove': {
        const m = ui.state.mods.find((x) => x.key === key);
        if (!(await confirmBox('Uninstall mod', `<p>Uninstall <b>${esc(m ? m.title : key)}</b>? Its files are removed from the game, along with dependencies nothing else needs.</p>`, 'Uninstall'))) return;
        setState(await invoke('uninstall', { gameId, key }));
        if (ui.browse) ui.browse.items.forEach((i) => { if (!ui.state.mods.some((x) => x.key === i.key)) i.installed = false; });
        break;
      }
      case 'install':
      case 'install-pack': {
        const pack = el.dataset.act === 'install-pack';
        ui.busy.add(key);
        renderTab();
        try {
          let st;
          if (pack) {
            const r = await invoke('install_modpack', { gameId, key, title: el.dataset.title });
            st = r.state;
            if (r.failed.length) toast('mpf', `${plural(r.failed.length, 'mod')} couldn't be installed`, { text: r.failed.join('\n'), kind: 'error', ttl: 12000 });
            toast('mp', `Modpack ready in profile “${st.game.activeProfile}”`, { kind: 'ok', ttl: 4000 });
          } else {
            st = await invoke('install', { gameId, key });
          }
          const have = new Set(st.mods.map((m) => m.key));
          if (ui.browse) ui.browse.items.forEach((i) => { i.installed = have.has(i.key); });
          setState(st);
        } finally {
          ui.busy.delete(key);
          renderTab();
        }
        break;
      }
      // profiles
      case 'switch-profile': ui.menu = null; setState(await switchProfile(name)); break;
      case 'new-profile': {
        ui.menu = null;
        const n = await ask('New profile', 'Name it. It starts empty; your current mods stay saved in their profile.', '', 'Create');
        if (!n) return renderGame();
        setState(await invoke('new_profile', { gameId, name: n }));
        ui.tab = 'browse';
        renderGame();
        break;
      }
      case 'save-as': {
        const n = await ask('Copy to new profile', 'Name for the copy', '', 'Save');
        if (n) setState(await invoke('save_profile', { gameId, name: n }));
        break;
      }
      case 'rename-profile': {
        const n = await ask('Rename profile', 'New name', name, 'Rename');
        if (n && n !== name) setState(await invoke('rename_profile', { gameId, from: name, to: n }));
        break;
      }
      case 'delete-profile': {
        const p = ui.state.profiles.find((x) => x.name === name);
        if (!(await confirmBox('Delete profile', `<p>Delete “${esc(name)}”? ${p?.active ? 'Its mods stay in the game until you switch profiles.' : 'Nothing in the game changes.'}</p>`, 'Delete'))) return;
        setState(await invoke('delete_profile', { gameId, name }));
        break;
      }
      case 'share-profile': await shareDialog(name); break;
      case 'import-code': {
        ui.menu = null;
        renderGame();
        const code = await ask('Import a profile', `Paste a ModNest code${ui.state.game.source !== 'own' ? ' or a Thunderstore / r2modman code' : ''}`, '', 'Import');
        if (code) await importCode(code);
        break;
      }
      case 'import-pasted': { const code = $('#code-in').value.trim(); if (code) await importCode(code); break; }
      case 'import-mgr': {
        const r = await invoke('import_manager_profile', { gameId, name });
        ui.tab = 'installed';
        setState(await switchProfile(r.name));
        break;
      }
    }
  } catch (err) {
    fail(err);
    el.disabled = false;
  }
});

document.addEventListener('change', async (e) => {
  const key = e.target.dataset.enable;
  if (!key) return;
  try { setState(await invoke('set_enabled', { gameId: ui.gameId, key, enabled: e.target.checked })); }
  catch (err) { fail(err); e.target.checked = !e.target.checked; }
});

// ---------- app self-update ----------
async function checkAppUpdate() {
  try {
    const u = await invoke('check_app_update');
    if (!u) return;
    $('#app-update-text').textContent = `ModNest ${u.version} is ready to install.`;
    $('#app-update').classList.remove('hidden');
  } catch { /* offline */ }
}
$('#app-update-btn').addEventListener('click', async (e) => {
  e.target.disabled = true;
  e.target.textContent = 'Updating…';
  try { await invoke('install_app_update'); } catch (err) { fail(err); e.target.disabled = false; e.target.textContent = 'Update and restart'; }
});
listen('app-update-progress', (f) => toast('selfupd', 'Downloading ModNest update', { fraction: f }));

// ---------- start ----------
(async function start() {
  try {
    const r = await invoke('init');
    ui.games = r.games;
    ui.settings = r.settings;
    ui.version = r.version;
    if (!ui.games.some((g) => g.installed)) ui.libScope = 'all';
    showLibrary();
  } catch (e) {
    $('#main').innerHTML = `<div class="center"><p>ModNest couldn't start: ${esc(e)}</p></div>`;
  }
  checkAppUpdate();
  setInterval(checkAppUpdate, 6 * 60 * 60 * 1000);
})();
