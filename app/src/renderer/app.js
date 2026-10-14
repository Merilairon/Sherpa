'use strict';
/* Base Camp Linux — renderer. Plain DOM, no framework. All device state lives in basecampd. */

// `bc` is the API exposed by preload.js through contextBridge.

// ---------------------------------------------------------------------------------------
// Helpers

function h(tag, attrs, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k === 'style' && typeof v === 'object') Object.assign(el.style, v);
    else if (k.startsWith('on') && typeof v === 'function') el.addEventListener(k.slice(2).toLowerCase(), v);
    else if (k === 'value') el.value = v;
    else if (k === 'checked') el.checked = !!v;
    else if (k === 'disabled') el.disabled = !!v;
    else if (k === 'selected') el.selected = !!v;
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const c of children.flat(Infinity)) {
    if (c == null || c === false) continue;
    el.append(c instanceof Node ? c : String(c));
  }
  return el;
}

const clone = (v) => (v == null ? v : JSON.parse(JSON.stringify(v)));
const $main = () => document.getElementById('main');

function toast(msg, kind = '') {
  const t = h('div', { class: `toast ${kind}` }, msg);
  document.getElementById('toasts').append(t);
  setTimeout(() => t.remove(), kind === 'error' ? 7000 : 3500);
}

async function call(method, params, { quiet = false, okMsg = null } = {}) {
  try {
    const r = await bc.call(method, params || {});
    if (okMsg) toast(okMsg, 'ok');
    return r;
  } catch (e) {
    const msg = String(e.message || e).replace(/^Error invoking remote method 'bc:call': (Error: )?/, '');
    if (!quiet) toast(msg, 'error');
    throw new Error(msg);
  }
}

function modal(title, body, buttons) {
  return new Promise((resolve) => {
    const root = document.getElementById('modal-root');
    const close = (v) => { root.innerHTML = ''; resolve(v); };
    const back = h('div', { class: 'modal-back', onclick: (e) => { if (e.target === back) close(null); } },
      h('div', { class: 'modal' },
        h('h2', {}, title),
        typeof body === 'string' ? h('p', {}, body) : body,
        h('div', { class: 'row' }, buttons.map((b) => h('button', { class: b.class || '', onclick: () => close(b.value) }, b.label)))));
    root.append(back);
  });
}

const confirmBox = (title, body, label = 'Continue', cls = 'primary') =>
  modal(title, body, [{ label: 'Cancel', value: false }, { label, value: true, class: cls }]);

function field(label, input, hint) {
  return h('label', { class: 'field' }, label, input, hint ? h('span', { class: 'hint' }, hint) : null);
}

function select(options, value, onchange, attrs = {}) {
  return h('select', { ...attrs, onchange: (e) => onchange(e.target.value) },
    options.map((o) => {
      const [v, l] = Array.isArray(o) ? o : [o, o];
      return h('option', { value: v, selected: String(v) === String(value) }, l);
    }));
}

function check(label, value, onchange) {
  return h('label', { class: 'check' }, h('input', { type: 'checkbox', checked: value, onchange: (e) => onchange(e.target.checked) }), label);
}

function slider(value, onchange, { min = 0, max = 100, step = 1 } = {}) {
  const out = h('span', { class: 'hint', style: { width: '36px', textAlign: 'right' } }, String(value));
  const input = h('input', { type: 'range', min, max, step, value, class: 'grow', oninput: (e) => { out.textContent = e.target.value; onchange(Number(e.target.value)); } });
  return h('div', { class: 'row', style: { gap: '8px' } }, input, out);
}

function colorInput(value, onchange) {
  return h('input', { type: 'color', value: value || '#000000', oninput: (e) => onchange(e.target.value) });
}

function shortcutInput(value, onchange, placeholder = 'Click, then press keys') {
  const input = h('input', {
    value: value || '', placeholder, readonly: true, class: 'grow',
    onkeydown: (e) => {
      e.preventDefault();
      const s = window.shortcutFromEvent(e);
      if (s) { input.value = s; onchange(s); }
    },
  });
  return h('div', { class: 'row', style: { gap: '6px', flexWrap: 'nowrap' } }, input,
    h('button', { class: 'small', onclick: () => { input.value = ''; onchange(''); } }, 'Clear'));
}

// ---------------------------------------------------------------------------------------
// State

const S = {
  info: {},
  status: null,
  devices: [],
  devId: null,
  dev: null,
  layout: null,
  catalog: null,
  macros: [],
  settings: null,
  view: 'device',
  tab: null,
  lightMenu: null,
  lightDraft: null,
  customDraft: null,
  paint: '#ff0000',
  paintEffect: 0,
  selKeys: new Set(),
  keySel: null,
  bindDraft: null,
  e60Layer: 0,
  dpLayer: 0,
  macroSel: null,
  macroDraft: null,
  recording: false,
  modes: null,
  dockDraft: null,
  dockState: null,
  mouseDraft: null,
  images: new Map(),
};

const KIND_TABS = {
  'everest-max': ['lighting', 'keys', 'dock', 'profile', 'device'],
  'macropad': ['lighting', 'keys', 'profile', 'device'],
  'everest-60': ['lighting', 'keys', 'profile', 'device'],
  'displaypad': ['keys', 'profile', 'device'],
  'makalu-67': ['sensor', 'mouseLighting', 'buttons', 'profile', 'device'],
};
const TAB_LABELS = { lighting: 'Lighting', keys: 'Key bindings', dock: 'Media dock', profile: 'Profile', device: 'Device', sensor: 'Sensor & DPI', mouseLighting: 'Lighting', buttons: 'Buttons' };

const cfg = () => S.dev && S.dev.config;
const kind = () => S.dev && S.dev.kind;
const selectedProfile = () => {
  const c = cfg();
  return c && (c.profiles.find((p) => p.selected) || c.profiles[0]);
};
const slot = () => selectedProfile().slot;

// ---------------------------------------------------------------------------------------
// Data loading

async function loadDevices() {
  S.devices = await call('ListDevices', {}, { quiet: true }).catch(() => []);
  if (S.devId && !S.devices.find((d) => d.id === S.devId)) {
    S.devId = null;
    S.dev = null;
  }
  if (!S.devId && S.devices.length && S.view === 'device') await openDevice(S.devices[0].id, false);
}

async function loadDevice() {
  if (!S.devId) return;
  try {
    S.dev = await call('GetDevice', { dev: S.devId }, { quiet: true });
  } catch {
    S.dev = null;
  }
}

async function openDevice(id, doRender = true) {
  if (S.devId !== id) {
    S.devId = id;
    S.layout = null;
    S.tab = null;
    resetDrafts();
    S.dpLayer = 0;
    S.e60Layer = 0;
  }
  S.view = 'device';
  await loadDevice();
  if (S.dev) {
    S.layout = await call('GetLayout', { dev: id }, { quiet: true }).catch(() => null);
    if (!S.tab || !KIND_TABS[kind()].includes(S.tab)) S.tab = KIND_TABS[kind()][0];
    if (S.dev.live && S.dev.kind === 'displaypad') S.dpLayer = S.dev.live.displayLayer || 0;
  }
  if (doRender) render();
}

function resetDrafts() {
  S.lightDraft = null;
  S.customDraft = null;
  S.keySel = null;
  S.bindDraft = null;
  S.selKeys = new Set();
  S.modes = null;
  S.dockDraft = null;
  S.dockState = null;
  S.mouseDraft = null;
}

async function boot() {
  S.info = await bc.info();
  if (!S.info.connected) {
    render();
    return;
  }
  S.status = await call('GetStatus', {}, { quiet: true }).catch(() => null);
  S.catalog = await call('GetCatalog', {}, { quiet: true }).catch(() => null);
  S.settings = await call('GetSettings', {}, { quiet: true }).catch(() => null);
  S.macros = await call('GetMacros', {}, { quiet: true }).catch(() => []);
  if (S.settings) bc.applyAppSettings(S.settings);
  await loadDevices();
  render();
}

let refreshTimer = null;
function scheduleRefresh(full = false) {
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(async () => {
    if (full) await loadDevices();
    await loadDevice();
    render();
  }, 120);
}

bc.onSignal(async (msg) => {
  const d = msg.data || {};
  switch (msg.signal) {
    case 'DaemonConnected':
      await boot();
      break;
    case 'DaemonDisconnected':
    case 'DaemonUnavailable':
      S.info = await bc.info();
      render();
      break;
    case 'DeviceAttached':
    case 'DeviceDetached':
      scheduleRefresh(true);
      break;
    case 'ProfileChanged':
    case 'ProfileChangedOnDevice':
      if (d.id === S.devId) { resetDrafts(); scheduleRefresh(true); }
      break;
    case 'StoreChanged':
    case 'LightingChangedOnDevice':
    case 'DeviceChanged':
    case 'SensorChangedOnDevice':
      if (d.id === S.devId) scheduleRefresh(false);
      break;
    case 'LayerChanged':
      if (d.id === S.devId && d.layer !== S.dpLayer) { S.dpLayer = d.layer; S.keySel = null; S.bindDraft = null; scheduleRefresh(false); }
      break;
    case 'MacrosChanged':
      S.macros = await call('GetMacros', {}, { quiet: true }).catch(() => S.macros);
      if (S.view === 'macros') render();
      break;
    case 'SettingsChanged':
      S.settings = await call('GetSettings', {}, { quiet: true }).catch(() => S.settings);
      break;
    case 'ActionFailed':
      toast(`Action failed: ${d.message}`, 'error');
      break;
    case 'LaunchApprovalRequired': {
      const ok = await confirmBox('Allow this program?',
        `A key on your device asks to open "${d.path}". It is not bound to that key on this computer (the device may have been set up elsewhere). Allow it from now on?`, 'Allow');
      if (ok) await call('ApproveLaunch', { path: d.path }, { okMsg: 'Allowed. Press the key again.' });
      break;
    }
    case 'CalibrationResult':
      toast(`Calibration result: ${JSON.stringify(d.result)}`, 'ok');
      break;
    case 'DeviceReset':
      toast('The device was reset to defaults.');
      scheduleRefresh(true);
      break;
    default:
      break;
  }
});

// ---------------------------------------------------------------------------------------
// Layout

function render() {
  renderSidebar();
  const main = $main();
  const scroll = main.scrollTop;
  main.innerHTML = '';
  if (!S.info.connected) {
    main.append(renderDisconnected());
    return;
  }
  if (S.view === 'macros') main.append(renderMacros());
  else if (S.view === 'settings') main.append(renderSettings());
  else main.append(renderDevice());
  main.scrollTop = scroll;
}

function renderSidebar() {
  const sb = document.getElementById('sidebar');
  sb.innerHTML = '';
  sb.append(
    h('div', { class: 'brand' }, h('div', { class: 'logo' }, '▲'), 'Base Camp'),
    S.info.fake ? h('div', { class: 'side-section' }, h('span', { class: 'badge sim' }, 'Simulated devices')) : null,
    h('div', { class: 'side-title' }, 'Devices'),
    h('div', { class: 'side-section' },
      S.devices.length === 0 ? h('div', { class: 'hint', style: { padding: '6px 10px' } }, S.info.connected ? 'No device connected' : '—') : null,
      S.devices.map((d) => h('div', {
        class: `side-item ${S.view === 'device' && S.devId === d.id ? 'active' : ''}`,
        onclick: () => openDevice(d.id),
      }, h('span', { class: `dot ${d.connected ? 'on' : 'off'}` }),
      h('div', {}, h('div', {}, d.name), h('div', { class: 'sub' }, d.connected ? `Profile ${d.selectedSlot}` : 'Disconnected'))))),
    h('div', { class: 'side-title' }, 'Library'),
    h('div', { class: 'side-section' },
      h('div', { class: `side-item ${S.view === 'macros' ? 'active' : ''}`, onclick: () => { S.view = 'macros'; render(); } }, h('span', {}, '⏺'), 'Macros'),
      h('div', { class: `side-item ${S.view === 'settings' ? 'active' : ''}`, onclick: () => { S.view = 'settings'; render(); } }, h('span', {}, '⚙'), 'Settings')),
    h('div', { class: 'side-footer' },
      h('div', { class: 'row', style: { gap: '6px' } }, h('span', { class: `dot ${S.info.connected ? 'on' : 'off'}` }), S.info.connected ? 'Device service running' : 'Device service offline'),
      S.status ? h('div', {}, `basecampd ${S.status.version} · ${S.status.platform}`) : null));
}

function renderDisconnected() {
  const linux = S.info.platform === 'linux';
  return h('div', { class: 'status-panel card' },
    h('h1', {}, 'Connecting to the device service…'),
    h('p', { class: 'hint' }, 'Base Camp talks to your devices through a small background service, basecampd.'),
    S.info.error ? h('p', { class: 'warn' }, S.info.error) : null,
    h('p', {}, 'Endpoint: ', h('code', {}, S.info.endpoint || '?')),
    h('p', {}, 'Service binary: ', h('code', {}, S.info.binary || 'not found')),
    linux ? h('p', { class: 'hint' }, 'If devices are not detected, install the udev rule shipped in the package (70-basecamp.rules) and re-plug the device.') : null,
    h('button', { class: 'primary', onclick: async () => { S.info = await bc.info(); if (S.info.connected) boot(); else render(); } }, 'Retry'));
}

// ---------------------------------------------------------------------------------------
// Device page

function renderDevice() {
  if (!S.dev) {
    return h('div', { class: 'empty' }, S.devices.length ? 'Select a device.' : 'Plug in a Mountain device. It will appear here automatically.');
  }
  const c = cfg();
  const live = S.dev.live || {};
  const details = [];
  if (live.fw) details.push(`Firmware ${live.fw}`);
  if (kind() === 'everest-max') {
    if (live.dockSide) details.push(`Media dock: ${live.dockSide}`);
    if (live.numpadSide) details.push(`Numpad: ${live.numpadSide}`);
  }
  if (kind() === 'everest-60' && live.numpadSide) details.push(`Numpad: ${live.numpadSide}`);
  const tabs = KIND_TABS[kind()];
  return h('div', {},
    h('div', { class: 'header' },
      h('div', { class: 'grow' },
        h('h1', {}, c.name, ' ', S.dev.connected ? h('span', { class: 'badge ok' }, 'connected') : h('span', { class: 'badge bad' }, 'disconnected')),
        h('div', { class: 'meta' }, details.join(' · ') || ' '))),
    renderProfiles(),
    h('div', { class: 'tabs' }, tabs.map((t) => h('div', { class: `tab ${S.tab === t ? 'active' : ''}`, onclick: () => { S.tab = t; render(); } }, TAB_LABELS[t]))),
    renderTab());
}

function renderProfiles() {
  const c = cfg();
  return h('div', { class: 'profiles' }, c.profiles.slice().sort((a, b) => a.slot - b.slot).map((p) =>
    h('div', {
      class: `profile-chip ${p.selected ? 'sel' : ''}`,
      title: p.linkedExe ? `Linked to ${p.linkedExe}` : '',
      onclick: async () => {
        if (p.selected) return;
        await call('SelectProfile', { dev: S.devId, slot: p.slot }).catch(() => {});
        resetDrafts();
        await loadDevice();
        render();
      },
    }, h('span', { class: 'n' }, p.slot), p.name, p.linkedExe ? h('span', { class: 'link' }, '⛓') : null, p.dirty ? h('span', { class: 'link', title: 'Changes pending until the device is connected' }, '•') : null)));
}

function renderTab() {
  switch (S.tab) {
    case 'lighting': return renderLighting();
    case 'keys': return renderKeys();
    case 'dock': return renderDock();
    case 'profile': return renderProfileTab();
    case 'device': return renderDeviceTab();
    case 'sensor': return renderSensor();
    case 'mouseLighting': return renderMouseLighting();
    case 'buttons': return renderMouseButtons();
    default: return h('div');
  }
}

// ---------------------------------------------------------------------------------------
// Keyboard drawing

const DOCK_KEYS = new Set([8, 17, 26, 35, 44, 53, 62]);
const SHORT_LABELS = {
  'PageDown': 'PgDn', 'PageUp': 'PgUp', 'Snapshot': 'PrtSc', 'CapsLock': 'Caps', 'Numlock': 'Num', 'Scroll Lock': 'ScrLk',
  'K29 Backslash': '\\', 'Comma': ',', 'Dot.': '.', 'Backspace': '⌫ Back', 'Play/ Pause': 'Play', 'NEXT TRACK': 'Next',
  'PRE TRACK': 'Prev', 'Scroll L': 'Dial ⟲', 'Scroll R': 'Dial ⟳', 'SW1 (G1)': 'D1', 'SW2(G2)': 'D2', 'SW3(G3)': 'D3', 'SW4(G4)': 'D4',
};
const keyLabel = (key) => (kind() === 'everest-max' && SHORT_LABELS[key.label]) || key.label;
const isPlaceholder = (label) => /^K\d+(\/K?\d+)?$/.test(label);

/** Draw the device's keys. `decorate(key, el)` customises each key element. */
function renderKeyboard(onClick, decorate) {
  const L = S.layout;
  if (!L || !L.keys) return h('div', { class: 'empty' }, 'No layout available.');
  const k = kind();
  const mk = (key, extra = {}) => {
    const el = h('div', { class: 'key', title: `${key.label} (LED ${key.led}, key ${key.fwKey})`, onclick: (e) => onClick(key, e), ...extra }, keyLabel(key));
    decorate(key, el);
    return el;
  };
  if (k === 'everest-max') {
    const cells = new Map();
    const dock = [];
    for (const key of L.keys) {
      if (DOCK_KEYS.has(key.index)) { dock.push(key); continue; }
      const id = `${key.row}:${key.col}`;
      const cur = cells.get(id);
      if (!cur || (isPlaceholder(cur.label) && !isPlaceholder(key.label))) cells.set(id, key);
    }
    const occupied = (r, c) => cells.has(`${r}:${c}`);
    const WIDE_BACK = new Set(['Backspace', 'Enter', 'RShift', 'LShift', 'CapsLock', 'Tab']);
    const els = [];
    for (const key of cells.values()) {
      let start = key.col;
      let end = key.col;
      if (key.label === 'Space') {
        while (start - 1 > 0 && !occupied(key.row, start - 1)) start--;
        while (!occupied(key.row, end + 1) && end + 1 < 15) end++;
      } else if (WIDE_BACK.has(key.label)) {
        while (start - 1 > 0 && !occupied(key.row, start - 1) && key.col - start < 2) start--;
      }
      els.push(mk(key, { style: { gridRow: key.row, gridColumn: `${start + 1} / ${end + 2}` } }));
    }
    const order = [17, 8, 44, 53, 26, 62, 35];
    dock.sort((a, b) => order.indexOf(a.index) - order.indexOf(b.index));
    return h('div', { class: 'kb-wrap' },
      h('div', { class: 'kb', style: { gridTemplateColumns: 'repeat(22, 36px)' } }, els),
      dock.length ? h('div', { class: 'dock-box' }, h('div', { class: 'title' }, 'Media dock'), dock.map((d) => mk(d))) : null);
  }
  if (k === 'macropad' || k === 'displaypad') {
    const big = k === 'displaypad';
    return h('div', { class: 'kb', style: { gridTemplateColumns: `repeat(${L.gridColumns}, ${big ? 92 : 64}px)` } },
      L.keys.slice().sort((a, b) => a.index - b.index).map((key) => mk(key, { class: `key ${big ? 'big' : ''}`, style: big ? {} : { height: '64px' } })));
  }
  if (k === 'everest-60') {
    const keys = S.e60Layer === 3 ? L.fnKeys : L.keys;
    return h('div', { class: 'kb', style: { gridTemplateColumns: `repeat(${L.gridColumns}, 52px)` } }, keys.map((key) => mk(key)));
  }
  return h('div');
}

// ---------------------------------------------------------------------------------------
// Lighting

const COLOR_TYPES = [['single', 'Single colour'], ['dual', 'Two colours'], ['random', 'Random']];
const BG_MENUS = new Set(['reactive', 'matrix', 'yeti']);
const ANIMATED = new Set(['colorWave', 'tornado', 'breathing', 'reactive', 'matrix', 'yeti']);
const LED_EFFECTS = [[0, 'Static'], [1, 'Breathing'], [3, 'Reactive'], [4, 'Color Wave'], [6, 'YETI'], [7, 'Tornado'], [9, 'Matrix']];

function renderLighting() {
  const p = selectedProfile();
  const menus = S.catalog ? S.catalog.menus : [];
  if (S.lightMenu == null) {
    const act = p.lighting.find((l) => l.active);
    S.lightMenu = act ? act.menuIndex : 'static';
  }
  const entry = p.lighting.find((l) => l.menuIndex === S.lightMenu) || p.lighting[0];
  if (!S.lightDraft || S.lightDraft.menuIndex !== S.lightMenu) S.lightDraft = clone(entry);
  const L = S.lightDraft;
  const list = h('div', { class: 'menu-row' }, menus.map((m) => {
    const e = p.lighting.find((l) => l.menuIndex === m.id);
    return h('div', { class: `menu-pill ${S.lightMenu === m.id ? 'sel' : ''}`, onclick: () => { S.lightMenu = m.id; S.lightDraft = null; render(); } },
      m.label, e && e.active ? h('span', { class: 'active-mark', title: 'Active effect' }, '●') : null);
  }));
  return h('div', { class: 'col' }, list, S.lightMenu === 'custom' ? renderCustom(L) : h('div', { style: { maxWidth: '640px' } }, renderLightEditor(L)));
}

function renderLightEditor(L) {
  const m = L.menuIndex;
  const off = m === 'off';
  const bg = BG_MENUS.has(m);
  const set = (k, v) => { L[k] = v; };
  const rerender = (k, v) => { L[k] = v; render(); };
  const rows = [];
  if (!off) {
    rows.push(field('Colour', select(COLOR_TYPES, L.colorType, (v) => rerender('colorType', v))));
    const colours = [];
    if (L.colorType !== 'random') colours.push(field(bg ? 'Colour' : 'Colour 1', colorInput(L.color1, (v) => set('color1', v))));
    if (L.colorType === 'dual' && !bg) colours.push(field('Colour 2', colorInput(L.color2, (v) => set('color2', v))));
    if (bg) colours.push(field('Background', colorInput(L.color3, (v) => set('color3', v))));
    rows.push(h('div', { class: 'row' }, colours));
    if (ANIMATED.has(m)) rows.push(field('Speed', slider(L.speed, (v) => set('speed', v))));
    rows.push(field('Brightness', slider(L.brightness, (v) => set('brightness', v))));
    if (m === 'colorWave') rows.push(field('Direction', select([[0, 'Direction 0'], [2, 'Direction 2'], [4, 'Direction 4'], [6, 'Direction 6']], L.direction, (v) => set('direction', Number(v)))));
    if (m === 'tornado') rows.push(field('Direction', select([[9, 'Direction 9'], [10, 'Direction 10']], L.direction === 10 ? 10 : 9, (v) => set('direction', Number(v)))));
  } else {
    rows.push(h('p', { class: 'hint' }, 'Turns the key lighting off for this profile.'));
  }
  rows.push(check('Apply to every profile', L.syncAcrossProfiles, (v) => set('syncAcrossProfiles', v)));
  const canPreview = S.dev.connected;
  return h('div', { class: 'card col' },
    h('h2', {}, (S.catalog.menus.find((x) => x.id === m) || {}).label),
    rows,
    h('div', { class: 'row' },
      h('button', { disabled: !canPreview, onclick: () => call('PreviewLighting', { dev: S.devId, lighting: L }) }, 'Preview'),
      h('button', { class: 'primary', onclick: async () => {
        await call('SetLighting', { dev: S.devId, slot: slot(), lighting: L, activate: true }, { okMsg: 'Lighting applied' }).catch(() => {});
        S.lightDraft = null;
        await loadDevice();
        render();
      } }, 'Apply & save')),
    h('p', { class: 'hint' }, 'Apply sends the effect and saves it to the device\'s memory for this profile.'));
}

function renderCustom(L) {
  const p = selectedProfile();
  if (!S.customDraft) S.customDraft = clone(p.custom);
  const C = S.customDraft;
  const leds = C.leds;
  const kb = renderKeyboard((key, e) => {
    if (e.shiftKey || e.ctrlKey) {
      if (S.selKeys.has(key.led)) S.selKeys.delete(key.led); else S.selKeys.add(key.led);
    } else if (S.selKeys.size === 1 && S.selKeys.has(key.led)) {
      S.selKeys.clear();
    } else {
      S.selKeys = new Set([key.led]);
    }
    render();
  }, (key, el) => {
    const col = leds[key.led] || '#000000';
    el.append(h('div', { class: 'led', style: { background: col } }));
    el.style.boxShadow = `inset 0 -14px 18px -14px ${col}`;
    if (S.selKeys.has(key.led)) el.classList.add('sel');
  });
  const allLeds = () => (S.layout.keys || []).map((k) => k.led).filter((l) => l < leds.length);
  const paint = () => {
    for (const led of S.selKeys) {
      if (led < leds.length) {
        leds[led] = S.paint;
        C.perLedEffect[led] = Number(S.paintEffect);
      }
    }
    render();
  };
  return h('div', { class: 'col' },
    h('div', { class: 'card' }, kb, h('p', { class: 'hint' }, 'Click a key to select it; Shift/Ctrl-click to select several.')),
    h('div', { class: 'card col' },
      h('div', { class: 'row' },
        field('Colour', colorInput(S.paint, (v) => { S.paint = v; })),
        field('Effect', select(LED_EFFECTS, S.paintEffect, (v) => { S.paintEffect = Number(v); })),
        h('div', { class: 'col', style: { gap: '4px' } }, h('span', { class: 'hint' }, 'Palette'),
          h('div', { class: 'palette' }, (C.palette || []).map((c) => h('div', { class: `swatch ${S.paint.toLowerCase() === c.toLowerCase() ? 'sel' : ''}`, style: { background: c }, onclick: () => { S.paint = c; render(); } })),
            h('button', { class: 'small', title: 'Add the current colour', onclick: () => { C.palette = [...(C.palette || []), S.paint].slice(-16); render(); } }, '+')))),
      h('div', { class: 'row' },
        h('button', { onclick: () => { S.selKeys = new Set(allLeds()); render(); } }, 'Select all'),
        h('button', { onclick: () => { S.selKeys.clear(); render(); } }, 'Select none'),
        h('button', { class: 'primary', disabled: S.selKeys.size === 0, onclick: paint }, `Paint ${S.selKeys.size || ''} key${S.selKeys.size === 1 ? '' : 's'}`)),
      field('Brightness', slider(L.brightness, (v) => { L.brightness = v; })),
      h('div', { class: 'row' },
        h('button', { onclick: () => { S.customDraft = null; render(); } }, 'Discard changes'),
        h('button', { class: 'primary', onclick: async () => {
          try {
            await call('SetLighting', { dev: S.devId, slot: slot(), lighting: L, activate: false });
            await call('SetCustom', { dev: S.devId, slot: slot(), custom: C, activate: true }, { okMsg: 'Custom lighting applied' });
            S.customDraft = null;
            await loadDevice();
            render();
          } catch { /* toast shown */ }
        } }, 'Apply & save'))));
}

// ---------------------------------------------------------------------------------------
// Key bindings

const BINDING_TYPES = [
  ['default', 'Default'], ['disable', 'Disable'], ['shortcut', 'Keyboard shortcut'], ['keyboardFunction', 'Keyboard function (F13–F24)'],
  ['media', 'Media'], ['mouse', 'Mouse'], ['profile', 'Profile'], ['osCommand', 'OS command'], ['macro', 'Run macro'],
  ['program', 'Run program'], ['url', 'Open website'], ['folder', 'Open folder'], ['obs', 'OBS Studio'],
  ['hotkeySwitch', 'Hotkey switch'], ['multiAction', 'Multi-action'], ['twitch', 'Twitch'], ['youTube', 'YouTube'], ['spotify', 'Spotify'],
];
const DP_TYPES = [['displayFolder', 'Folder'], ['displayBack', 'Back (parent folder)']];
const STEP_TYPES = [['shortcut', 'Shortcut'], ['keyboardFunction', 'F13–F24'], ['media', 'Media'], ['mouse', 'Mouse'], ['osCommand', 'OS command'], ['macro', 'Macro'], ['program', 'Program'], ['url', 'Website'], ['folder', 'Folder'], ['obs', 'OBS'], ['profile', 'Profile'], ['delay', 'Delay (ms)']];
const typeLabel = (t) => (BINDING_TYPES.concat(DP_TYPES).find((x) => x[0] === t) || [t, t])[1];

function currentLayer() {
  return kind() === 'everest-60' ? S.e60Layer : 0;
}

function bindingFor(key) {
  const p = selectedProfile();
  const layerId = kind() === 'displaypad' ? S.dpLayer : 0;
  return p.bindings.find((b) => b.led === key.led && (b.layer || 0) === currentLayer() && (b.layerId || 0) === layerId);
}

function imagePreview(id) {
  if (!id) return null;
  if (S.images.has(id)) return S.images.get(id);
  S.images.set(id, null);
  call('GetImage', { id }, { quiet: true }).then((url) => { S.images.set(id, url); render(); }).catch(() => {});
  return null;
}

function renderKeys() {
  const k = kind();
  const top = [];
  if (k === 'everest-60') {
    top.push(h('div', { class: 'row', style: { marginBottom: '10px' } },
      h('span', { class: 'hint' }, 'Layer:'),
      [[0, 'Base'], [3, 'Fn']].map(([v, l]) => h('button', { class: S.e60Layer === v ? 'primary small' : 'small', onclick: () => { S.e60Layer = v; S.keySel = null; S.bindDraft = null; render(); } }, l))));
  }
  if (k === 'displaypad') top.push(renderCrumbs());
  const kb = renderKeyboard((key) => {
    S.keySel = key;
    const b = bindingFor(key);
    S.bindDraft = b ? clone(b) : { type: 'default', value: '', led: key.led, fwKey: key.fwKey, layer: currentLayer(), layerId: k === 'displaypad' ? S.dpLayer : 0, isFirstImageSelected: true };
    render();
  }, (key, el) => {
    const b = bindingFor(key);
    if (S.keySel && S.keySel.led === key.led) el.classList.add('sel');
    if (k === 'displaypad') {
      el.textContent = '';
      const url = b && imagePreview(b.isFirstImageSelected === false ? (b.secondImage || b.image) : b.image);
      if (url) el.append(h('img', { src: url }));
      else el.append(h('span', { class: 'cap' }, b ? (b.alternateName || typeLabel(b.type)) : ''));
      if (b && b.type === 'displayFolder' && !url) el.append(h('span', { class: 'cap' }, ' 📁'));
      return;
    }
    if (b && b.type !== 'default') {
      el.classList.add('bound');
      el.append(h('div', { class: 'alt' }, b.alternateName || typeLabel(b.type)));
    }
  });
  const editor = S.keySel ? renderBindingEditor() : h('div', { class: 'card hint' }, 'No key selected.');
  const kbCard = h('div', { class: 'card' }, top, kb, h('p', { class: 'hint' }, 'Select a key to change what it does.'));
  if (k === 'everest-max' || k === 'everest-60') {
    return h('div', { class: 'col' }, kbCard, h('div', { style: { maxWidth: '720px' } }, editor));
  }
  return h('div', { class: 'split' }, kbCard, editor);
}

function layerName(id) {
  const p = selectedProfile();
  const l = (p.layers || []).find((x) => x.id === id);
  return l ? l.name : `Folder ${id}`;
}

function renderCrumbs() {
  const p = selectedProfile();
  const path = [];
  let cur = S.dpLayer;
  const seen = new Set();
  while (cur != null && !seen.has(cur)) {
    seen.add(cur);
    path.unshift(cur);
    const l = (p.layers || []).find((x) => x.id === cur);
    cur = l ? l.parent : null;
  }
  const go = async (id) => {
    S.dpLayer = id;
    S.keySel = null;
    S.bindDraft = null;
    if (S.dev.connected) await call('OpenLayer', { dev: S.devId, layer: id }, { quiet: true }).catch(() => {});
    render();
  };
  return h('div', { class: 'crumbs' }, path.map((id, i) => [i ? '›' : null,
    i === path.length - 1 ? h('span', {}, id === 0 ? 'Home' : layerName(id)) : h('span', { class: 'link', onclick: () => go(id) }, id === 0 ? 'Home' : layerName(id))]));
}

function valueSelect(list, value, onchange) {
  const opts = (list || []).map((x) => [x, x]);
  if (value && !list.includes(value)) opts.unshift([value, value]);
  if (!value && opts.length) setTimeout(() => onchange(opts[0][0]));
  return select(opts, value, onchange);
}

/** Type-specific inputs, shared by key bindings and multi-action steps. */
function bindingFields(B, rerender, { forStep = false } = {}) {
  const cat = S.catalog || {};
  const set = (k) => (v) => { B[k] = v; };
  const t = B.type;
  const browse = (key, folder) => h('button', { class: 'small', onclick: async () => {
    const p = folder ? await bc.openFolder() : await bc.openFile();
    if (p) { B[key] = p; rerender(); }
  } }, 'Browse…');
  switch (t) {
    case 'shortcut':
      return [field('Shortcut', shortcutInput(B.value, set('value')))];
    case 'keyboardFunction':
      return [field('Key', valueSelect(cat.keyboardFunctions, B.value, set('value')))];
    case 'media':
      return [field('Action', valueSelect(cat.media, B.value, set('value')))];
    case 'mouse':
      return [field('Action', valueSelect(cat.mouse, B.value, set('value')), forStep ? null : 'Scroll left/right are not supported by the device yet.')];
    case 'profile':
      return [field('Action', valueSelect(cat.profile, B.value, set('value')))];
    case 'osCommand':
      return [field('Command', valueSelect(cat.os, B.value, set('value')))];
    case 'macro':
      return S.macros.length
        ? [field('Macro', valueSelect(S.macros.map((m) => m.name), B.value, set('value')))]
        : [h('p', { class: 'warn' }, 'Create a macro in the Macros library first.')];
    case 'program':
      return [field('Program', h('div', { class: 'row', style: { flexWrap: 'nowrap' } }, h('input', { class: 'grow', value: B.value, oninput: (e) => { B.value = e.target.value; } }), browse('value', false))),
        field('Arguments', h('input', { value: B.value2 || '', placeholder: 'optional, separated by ~', oninput: (e) => { B.value2 = e.target.value; } }))];
    case 'url':
      return [field('Website', h('input', { value: B.value, placeholder: 'https://…', oninput: (e) => { B.value = e.target.value; } }), 'Only http and https addresses are opened.')];
    case 'folder':
      return [field('Folder', h('div', { class: 'row', style: { flexWrap: 'nowrap' } }, h('input', { class: 'grow', value: B.value, oninput: (e) => { B.value = e.target.value; } }), browse('value', true)))];
    case 'obs':
      return [field('Command', valueSelect(cat.obs, B.subType, (v) => { B.subType = v; })),
        field('Argument', h('input', { value: B.value || '', placeholder: 'scene / source / value, if needed', oninput: (e) => { B.value = e.target.value; } }))];
    case 'twitch':
    case 'youTube':
    case 'spotify': {
      const list = { twitch: cat.twitch, youTube: cat.youtube, spotify: cat.spotify }[t];
      return [h('p', { class: 'warn' }, 'This integration needs an account link that is not available in this version yet.'),
        field('Action', valueSelect(list, B.subType, (v) => { B.subType = v; })),
        field('Value', h('input', { value: B.value || '', oninput: (e) => { B.value = e.target.value; } })),
        t !== 'youTube' ? field('Value 2', h('input', { value: B.value2 || '', oninput: (e) => { B.value2 = e.target.value; } })) : null];
    }
    case 'hotkeySwitch':
      return [field('First press', shortcutInput(B.value, set('value'))), field('Second press', shortcutInput(B.value2, set('value2'))),
        h('p', { class: 'hint' }, 'Each press alternates between the two shortcuts.')];
    case 'delay':
      return [field('Milliseconds', h('input', { type: 'number', min: 0, value: B.value || '100', oninput: (e) => { B.value = e.target.value; } }))];
    case 'multiAction':
      return [renderSteps(B, rerender)];
    case 'displayFolder':
      return [h('p', { class: 'hint' }, B.value ? `Opens “${layerName(Number(B.value))}”.` : 'Saving creates a new folder with a Back key.'),
        B.value ? h('button', { onclick: async () => { S.dpLayer = Number(B.value); S.keySel = null; S.bindDraft = null; if (S.dev.connected) await call('OpenLayer', { dev: S.devId, layer: S.dpLayer }, { quiet: true }).catch(() => {}); render(); } }, 'Open folder') : null];
    case 'displayBack':
      return [h('p', { class: 'hint' }, 'Returns to the parent folder.')];
    case 'disable':
      return [h('p', { class: 'hint' }, 'The key does nothing.')];
    default:
      return [h('p', { class: 'hint' }, 'The key does what is printed on it.')];
  }
}

function renderSteps(B, rerender) {
  let steps = [];
  try { steps = JSON.parse(B.value || '[]'); } catch { steps = []; }
  const save = () => { B.value = JSON.stringify(steps); };
  save();
  const move = (i, d) => { const j = i + d; if (j < 0 || j >= steps.length) return; [steps[i], steps[j]] = [steps[j], steps[i]]; save(); rerender(); };
  return h('div', { class: 'col steps' },
    h('div', { class: 'hint' }, 'Steps run in order. Action delay waits after a step; key press is how long keys are held.'),
    steps.map((s, i) => h('div', { class: 'card col', style: { padding: '10px' } },
      h('div', { class: 'row' },
        h('strong', {}, `${i + 1}.`),
        select(STEP_TYPES, s.type, (v) => { s.type = v; s.value = ''; s.subType = null; save(); rerender(); }),
        h('span', { class: 'spacer' }),
        h('button', { class: 'small', onclick: () => move(i, -1) }, '↑'),
        h('button', { class: 'small', onclick: () => move(i, 1) }, '↓'),
        h('button', { class: 'small danger', onclick: () => { steps.splice(i, 1); save(); rerender(); } }, '✕')),
      bindingFields(new Proxy(s, { set: (o, k, v) => { o[k] = v; save(); return true; } }), rerender, { forStep: true }),
      s.type !== 'delay' ? h('div', { class: 'grid2' },
        field('Action delay (ms)', h('input', { type: 'number', min: 0, value: s.actionDelayMs || 0, oninput: (e) => { s.actionDelayMs = Number(e.target.value); save(); } })),
        field('Key press (ms)', h('input', { type: 'number', min: 0, value: s.keyPressDelayMs || 50, oninput: (e) => { s.keyPressDelayMs = Number(e.target.value); save(); } }))) : null)),
    h('button', { onclick: () => { steps.push({ type: 'shortcut', value: '', actionDelayMs: 0, keyPressDelayMs: 50 }); save(); rerender(); } }, '+ Add step'));
}

async function composeIcon(dataUrl, caption) {
  const img = new Image();
  if (dataUrl) {
    img.src = dataUrl;
    await img.decode();
  }
  const c = document.createElement('canvas');
  c.width = 102;
  c.height = 102;
  const g = c.getContext('2d');
  g.fillStyle = '#000';
  g.fillRect(0, 0, 102, 102);
  if (dataUrl) {
    const s = Math.max(102 / img.width, 102 / img.height);
    const w = img.width * s;
    const hgt = img.height * s;
    g.drawImage(img, (102 - w) / 2, (102 - hgt) / 2, w, hgt);
  }
  if (caption) {
    g.font = 'bold 15px sans-serif';
    g.textAlign = 'center';
    g.textBaseline = 'bottom';
    g.lineWidth = 4;
    g.strokeStyle = 'rgba(0,0,0,.8)';
    g.strokeText(caption, 51, 98, 98);
    g.fillStyle = '#fff';
    g.fillText(caption, 51, 98, 98);
  }
  const px = g.getImageData(0, 0, 102, 102).data;
  const bgr = new Uint8Array(102 * 102 * 3);
  for (let i = 0, j = 0; i < px.length; i += 4, j += 3) {
    bgr[j] = px[i + 2];
    bgr[j + 1] = px[i + 1];
    bgr[j + 2] = px[i];
  }
  let bin = '';
  for (let i = 0; i < bgr.length; i += 0x8000) bin += String.fromCharCode.apply(null, bgr.subarray(i, i + 0x8000));
  const preview = c.toDataURL('image/png');
  const id = await call('PutImage', { bgr: btoa(bin), preview });
  S.images.set(id, preview);
  return id;
}

function renderDisplayImages(B, rerender) {
  const pick = async (which) => {
    const file = await bc.openFile({ filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp'] }] });
    if (!file) return;
    const url = await bc.readImage(file);
    const cap = which === 'image' ? B.optionalText : B.secondOptionalText;
    B[which] = await composeIcon(url, cap);
    B[`_${which}Src`] = url;
    rerender();
  };
  const textIcon = async () => { B.image = await composeIcon(B._imageSrc || null, B.optionalText || B.alternateName || ''); rerender(); };
  const thumb = (id) => {
    const u = imagePreview(id);
    return u ? h('img', { src: u, style: { width: '64px', height: '64px', borderRadius: '6px' } }) : h('div', { class: 'key', style: { width: '64px', height: '64px' } }, '—');
  };
  return h('div', { class: 'col' },
    h('h3', {}, 'Key display'),
    h('div', { class: 'row' },
      h('div', { class: 'col' }, thumb(B.image), h('button', { class: 'small', onclick: () => pick('image') }, 'Image…')),
      h('div', { class: 'col' }, thumb(B.secondImage), h('button', { class: 'small', onclick: () => pick('secondImage') }, 'Pressed image…'))),
    field('Caption', h('input', { value: B.optionalText || '', oninput: (e) => { B.optionalText = e.target.value; } })),
    h('div', { class: 'row' },
      h('button', { class: 'small', onclick: textIcon }, 'Render caption on icon'),
      h('button', { class: 'small', onclick: () => { B.image = null; B.secondImage = null; rerender(); } }, 'Remove images')));
}

function renderBindingEditor() {
  const B = S.bindDraft;
  const key = S.keySel;
  const k = kind();
  const types = k === 'displaypad' ? BINDING_TYPES.filter(([t]) => t !== 'disable').concat(DP_TYPES) : BINDING_TYPES;
  const rerender = () => render();
  const save = async () => {
    const b = { ...B };
    for (const x of Object.keys(b)) if (x.startsWith('_')) delete b[x];
    try {
      if (b.type === 'displayFolder' && !b.value) {
        const id = await call('AddLayer', { dev: S.devId, slot: slot(), parent: S.dpLayer, name: b.alternateName || b.optionalText || 'Folder' });
        b.value = String(id);
      }
      if (b.type === 'obs' && !b.subType) b.subType = (S.catalog.obs || [])[0];
      await call('SetBinding', { dev: S.devId, slot: slot(), binding: b }, { okMsg: `${key.label}: ${typeLabel(b.type)}` });
      await loadDevice();
      const nb = bindingFor(key);
      S.bindDraft = nb ? clone(nb) : B;
      render();
    } catch { /* toast */ }
  };
  const reset = async () => {
    await call('ClearBinding', { dev: S.devId, slot: slot(), led: key.led, layer: currentLayer(), layerId: k === 'displaypad' ? S.dpLayer : 0 }, { okMsg: `${key.label} restored` }).catch(() => {});
    await loadDevice();
    S.bindDraft = { type: 'default', value: '', led: key.led, fwKey: key.fwKey, layer: currentLayer(), layerId: k === 'displaypad' ? S.dpLayer : 0, isFirstImageSelected: true };
    render();
  };
  return h('div', { class: 'card col' },
    h('h2', {}, key.label, h('span', { class: 'hint' }, `  LED ${key.led} · key ${key.fwKey}`)),
    field('Action', select(types, B.type, (v) => { B.type = v; B.value = ''; B.value2 = null; B.subType = null; render(); })),
    bindingFields(B, rerender),
    k === 'displaypad' ? renderDisplayImages(B, rerender) : null,
    field('Label', h('input', { value: B.alternateName || '', placeholder: 'optional name shown in Base Camp', oninput: (e) => { B.alternateName = e.target.value || null; } })),
    k !== 'displaypad' ? check('Apply to every profile', B.syncAcrossProfiles, (v) => { B.syncAcrossProfiles = v; }) : null,
    h('div', { class: 'row' },
      h('button', { class: 'primary', onclick: save }, 'Save'),
      h('button', { onclick: reset }, 'Reset key'),
      S.info.fake && S.dev.connected ? h('button', { onclick: () => call('SimulateKey', { dev: S.devId, led: key.led, layer: currentLayer() }) }, 'Simulate press') : null));
}

// ---------------------------------------------------------------------------------------
// Media dock (Everest Max)

const DOCK_PAGES = ['Clock', 'Profile', 'Lighting mode', 'Volume', 'Brightness', 'PC info', 'APM counter', 'Custom mode'];

function renderDock() {
  if (!S.dockDraft) S.dockDraft = clone(cfg().dock);
  const D = S.dockDraft;
  const set = (k) => (v) => { D[k] = v; };
  const ssTypes = (S.catalog && S.catalog.screenSaverTypes) || [];
  const levels = [0, 25, 50, 75, 100];
  return h('div', { class: 'col' },
    h('div', { class: 'card col' },
      h('h2', {}, 'Menu pages'),
      h('div', { class: 'grid3' }, DOCK_PAGES.map((name, i) => check(name, (D.pagesEnabled >> i) & 1, (v) => { D.pagesEnabled = v ? D.pagesEnabled | (1 << i) : D.pagesEnabled & ~(1 << i); }))),
      h('div', { class: 'row' },
        field('Clock', select([['true', 'Digital'], ['false', 'Analog']], String(D.clockDigital), (v) => { D.clockDigital = v === 'true'; })),
        field('Format', select([['false', '24 hour'], ['true', '12 hour']], String(D.clock12h), (v) => { D.clock12h = v === 'true'; })),
        field('Menu colour', colorInput(D.menuColor, set('menuColor'))))),
    h('div', { class: 'card col' },
      h('h2', {}, 'Screen'),
      h('div', { class: 'row' },
        check('Screen saver', D.screenSaverEnabled, set('screenSaverEnabled')),
        field('Type', select(ssTypes.map((x) => [x.value, x.label]), D.screenSaverType, (v) => { D.screenSaverType = Number(v); })),
        field('After (seconds)', h('input', { type: 'number', min: 0, max: 1800, value: D.screenSaverSec, oninput: (e) => { D.screenSaverSec = Number(e.target.value); } }))),
      h('div', { class: 'row' },
        check('Turn off automatically', D.turnOffEnabled, set('turnOffEnabled')),
        field('After (seconds)', h('input', { type: 'number', min: 0, max: 1800, value: D.turnOffSec, oninput: (e) => { D.turnOffSec = Number(e.target.value); } }))),
      h('p', { class: 'hint' }, 'Timeouts are limited to 1800 s. Screen-saver images are not supported yet.')),
    h('div', { class: 'card col' },
      h('h2', {}, 'Profiles and brightness'),
      h('div', { class: 'grid5' }, [0, 1, 2, 3, 4].map((i) => h('div', { class: 'col', style: { gap: '6px' } },
        h('strong', {}, `Profile ${i + 1}`),
        field('Shown as', select([1, 2, 3, 4, 5].map((n) => [n, `Profile ${n}`]), D.profilesShown[i], (v) => { D.profilesShown[i] = Number(v); })),
        field('Dock', select(levels.map((l) => [l, `${l}%`]), D.dockBrightness[i], (v) => { D.dockBrightness[i] = Number(v); })),
        field('Numpad', select(levels.map((l) => [l, `${l}%`]), D.numpadBrightness[i], (v) => { D.numpadBrightness[i] = Number(v); })))))),
    h('div', { class: 'row' },
      h('button', { class: 'primary', onclick: async () => {
        await call('SetDock', { dev: S.devId, dock: D }, { okMsg: 'Media dock settings saved' }).catch(() => {});
        S.dockDraft = null;
        await loadDevice();
        render();
      } }, 'Save to device'),
      h('button', { disabled: !S.dev.connected, onclick: () => call('SyncClock', { dev: S.devId }, { okMsg: 'Clock synchronised' }) }, 'Sync clock now'),
      h('button', { disabled: !S.dev.connected, onclick: async () => { S.dockState = await call('GetDockState', { dev: S.devId }).catch(() => null); render(); } }, 'Read dock state'),
      S.dockState ? h('span', { class: 'hint' }, `Dock ${S.dockState.dockSide}, numpad ${S.dockState.numpadSide}, page ${S.dockState.menuIndex}`) : null));
}

// ---------------------------------------------------------------------------------------
// Profile tab

function renderProfileTab() {
  const p = selectedProfile();
  const d = { name: p.name, linkedExe: p.linkedExe || '', revertOnExit: p.revertOnExit };
  let copyTo = cfg().profiles.find((x) => x.slot !== p.slot).slot;
  return h('div', { class: 'col', style: { maxWidth: '720px' } },
    h('div', { class: 'card col' },
      h('h2', {}, `Profile ${p.slot}`),
      field('Name', h('input', { value: d.name, oninput: (e) => { d.name = e.target.value; } })),
      field('Switch to this profile when this program starts',
        h('div', { class: 'row', style: { flexWrap: 'nowrap' } },
          h('input', { class: 'grow', value: d.linkedExe, placeholder: 'e.g. game.exe or firefox', oninput: (e) => { d.linkedExe = e.target.value; } }),
          h('button', { class: 'small', onclick: async () => { const f = await bc.openFile(); if (f) { d.linkedExe = f.split(/[\\/]/).pop(); render(); } } }, 'Browse…')),
        'Matched on the file name only.'),
      check('Switch back when the program exits', d.revertOnExit, (v) => { d.revertOnExit = v; }),
      h('div', { class: 'row' }, h('button', { class: 'primary', onclick: async () => {
        await call('UpdateProfile', { dev: S.devId, slot: p.slot, name: d.name, linkedExe: d.linkedExe, revertOnExit: d.revertOnExit }, { okMsg: 'Profile saved' }).catch(() => {});
        await loadDevice();
        render();
      } }, 'Save'))),
    h('div', { class: 'card col' },
      h('h2', {}, 'Copy, import and export'),
      h('div', { class: 'row' },
        'Copy this profile to',
        select(cfg().profiles.filter((x) => x.slot !== p.slot).map((x) => [x.slot, `${x.slot} · ${x.name}`]), copyTo, (v) => { copyTo = Number(v); }),
        h('button', { onclick: async () => {
          if (await confirmBox('Copy profile', `Replace the lighting and keys of profile ${copyTo} with this profile?`, 'Copy')) {
            await call('CopyProfile', { dev: S.devId, from: p.slot, to: copyTo }, { okMsg: 'Profile copied' }).catch(() => {});
            await loadDevice();
            render();
          }
        } }, 'Copy')),
      h('div', { class: 'row' },
        h('button', { onclick: async () => {
          const data = await call('ExportProfile', { dev: S.devId, slot: p.slot });
          if (await bc.saveJson(`${p.name}.basecamp-profile.json`, data)) toast('Profile exported', 'ok');
        } }, 'Export…'),
        h('button', { onclick: async () => {
          const data = await bc.loadJson();
          if (!data) return;
          await call('ImportProfile', { dev: S.devId, slot: p.slot, data }, { okMsg: 'Profile imported' }).catch(() => {});
          await loadDevice();
          render();
        } }, 'Import…'),
        h('span', { class: 'spacer' }),
        h('button', { class: 'danger', onclick: async () => {
          if (await confirmBox('Reset profile', 'Restore default lighting and key bindings for this profile?', 'Reset', 'danger')) {
            await call('ResetProfile', { dev: S.devId, slot: p.slot }, { okMsg: 'Profile reset' }).catch(() => {});
            resetDrafts();
            await loadDevice();
            render();
          }
        } }, 'Reset to default'))));
}

// ---------------------------------------------------------------------------------------
// Device tab

const MODES = {
  'everest-max': [['lighting', 'Key lighting'], ['indicator', 'Indicator LEDs'], ['gameMode', 'Game mode'], ['yeti', 'YETI mode'], ['syncProfiles', 'Same key bindings in all profiles'], ['keyFunctions', 'Fn key functions']],
  'macropad': [['lighting', 'Key lighting'], ['syncProfiles', 'Same key bindings in all profiles'], ['keyFunctions', 'Fn key functions']],
  'everest-60': [['lighting', 'Key lighting'], ['indicator', 'Indicator LEDs'], ['gameMode', 'Game mode'], ['yeti', 'YETI mode'], ['syncProfiles', 'Same key bindings in all profiles'], ['keyFunctions', 'Fn key functions']],
  'displaypad': [['logo', 'Show the Mountain logo']],
  'makalu-67': [],
};
const RESETS = {
  'everest-max': [['keys', 'Reset key bindings'], ['effects', 'Reset lighting'], ['all', 'Factory reset']],
  'macropad': [['keys', 'Reset key bindings'], ['effects', 'Reset lighting'], ['all', 'Factory reset']],
  'everest-60': [['keys', 'Reset key bindings'], ['effects', 'Reset lighting'], ['all', 'Factory reset']],
  'displaypad': [['pictures', 'Delete stored pictures'], ['keys', 'Reset key bindings']],
  'makalu-67': [['all', 'Factory reset']],
};

function renderDeviceTab() {
  const k = kind();
  const connected = S.dev.connected;
  if (connected && S.modes == null && MODES[k].length && k !== 'displaypad') {
    S.modes = {};
    call('GetModes', { dev: S.devId }, { quiet: true }).then((m) => { S.modes = m; render(); }).catch(() => {});
  }
  const modes = S.modes || cfg().modes || {};
  const live = S.dev.live || {};
  let dpBright = 80;
  let sleepOn = false;
  let sleepSec = 300;
  return h('div', { class: 'col', style: { maxWidth: '760px' } },
    MODES[k].length ? h('div', { class: 'card col' },
      h('h2', {}, 'Device settings'),
      MODES[k].map(([m, label]) => check(label, m === 'lighting' ? (modes[m] ?? 1) : modes[m], async (v) => {
        try {
          await call('SetMode', { dev: S.devId, mode: m, value: v ? 1 : 0 });
          S.modes = { ...modes, [m]: v ? 1 : 0 };
        } catch { render(); }
      })),
      connected ? null : h('p', { class: 'hint' }, 'Connect the device to change these settings.')) : null,
    k === 'displaypad' ? h('div', { class: 'card col' },
      h('h2', {}, 'Display'),
      field('Brightness', slider(dpBright, (v) => { dpBright = v; })),
      h('div', { class: 'row' }, h('button', { disabled: !connected, onclick: () => call('SetDisplayBrightness', { dev: S.devId, level: dpBright }, { okMsg: 'Brightness set' }) }, 'Set brightness')),
      h('div', { class: 'row' }, check('Sleep timer', sleepOn, (v) => { sleepOn = v; }),
        field('After (seconds)', h('input', { type: 'number', min: 0, value: sleepSec, oninput: (e) => { sleepSec = Number(e.target.value); } })),
        h('button', { disabled: !connected, onclick: () => call('SetDisplaySleep', { dev: S.devId, enabled: sleepOn, seconds: sleepSec }, { okMsg: 'Sleep timer set' }) }, 'Apply'))) : null,
    h('div', { class: 'card col' },
      h('h2', {}, 'Information'),
      h('div', { class: 'grid2' },
        h('div', {}, h('div', { class: 'hint' }, 'Device id'), h('code', {}, S.devId)),
        h('div', {}, h('div', { class: 'hint' }, 'Firmware'), live.fw || '—'),
        h('div', {}, h('div', { class: 'hint' }, 'Layout id'), live.layout != null ? String(live.layout) : '—'),
        h('div', {}, h('div', { class: 'hint' }, 'Active profile on device'), live.currentProfile ? String(live.currentProfile) : '—'))),
    h('div', { class: 'card col' },
      h('h2', {}, 'Reset'),
      h('div', { class: 'row' }, RESETS[k].map(([what, label]) => h('button', { class: 'danger', disabled: !connected, onclick: async () => {
        if (await confirmBox(label, 'This cannot be undone. Continue?', label, 'danger')) {
          await call('Reset', { dev: S.devId, what }, { okMsg: 'Done' }).catch(() => {});
          resetDrafts();
          await loadDevice();
          render();
        }
      } }, label)))));
}

// ---------------------------------------------------------------------------------------
// Makalu 67

function mouseProfile() {
  const p = selectedProfile();
  return p.mouse || { sensor: {}, lighting: {}, buttons: [] };
}

function renderSensor() {
  if (!S.mouseDraft || S.mouseDraft._slot !== slot()) S.mouseDraft = { ...clone(mouseProfile()), _slot: slot() };
  const M = S.mouseDraft.sensor;
  const save = async () => {
    await call('SetMouseSensor', { dev: S.devId, slot: slot(), sensor: M }, { okMsg: 'Sensor settings saved' }).catch(() => {});
    S.mouseDraft = null;
    await loadDevice();
    render();
  };
  return h('div', { class: 'col', style: { maxWidth: '760px' } },
    h('div', { class: 'card col' },
      h('h2', {}, 'DPI levels'),
      M.dpiLevels.map(([x, y], i) => h('div', { class: 'dpi-row' },
        h('input', { type: 'radio', name: 'dpi', checked: M.currentLevel === i + 1, onchange: () => { M.currentLevel = i + 1; } }),
        h('input', { type: 'number', min: 100, max: 26000, step: 50, value: x, oninput: (e) => { M.dpiLevels[i][0] = Number(e.target.value); } }),
        h('input', { type: 'number', min: 100, max: 26000, step: 50, value: y, oninput: (e) => { M.dpiLevels[i][1] = Number(e.target.value); } }),
        h('button', { class: 'small danger', disabled: M.dpiLevels.length <= 1, onclick: () => { M.dpiLevels.splice(i, 1); M.currentLevel = Math.min(M.currentLevel, M.dpiLevels.length); render(); } }, '✕'))),
      h('div', { class: 'row' },
        h('button', { disabled: M.dpiLevels.length >= 5, onclick: () => { M.dpiLevels.push([1600, 1600]); render(); } }, '+ Add level'),
        h('span', { class: 'hint' }, 'X and Y DPI per level; the selected level is active.'))),
    h('div', { class: 'card col' },
      h('h2', {}, 'Sensor'),
      h('div', { class: 'grid2' },
        field('Polling rate', select([[125, '125 Hz'], [250, '250 Hz'], [500, '500 Hz'], [1000, '1000 Hz']], M.pollingHz, (v) => { M.pollingHz = Number(v); })),
        field('Button response time (ms)', h('input', { type: 'number', min: 0, max: 32, value: M.responseMs, oninput: (e) => { M.responseMs = Number(e.target.value); } })),
        field('Lift-off distance', select([['false', 'Low'], ['true', 'High']], String(M.liftOffHigh), (v) => { M.liftOffHigh = v === 'true'; })),
        h('div', { class: 'col', style: { justifyContent: 'end' } }, check('Angle snapping', M.angleSnapping, (v) => { M.angleSnapping = v; })))),
    h('div', { class: 'row' }, h('button', { class: 'primary', onclick: save }, 'Save to mouse')),
    h('div', { class: 'card col' },
      h('h2', {}, 'Surface calibration'),
      h('p', { class: 'hint' }, 'Start, move the mouse on your surface for a few seconds, then read and save the result.'),
      h('div', { class: 'row' },
        h('button', { disabled: !S.dev.connected, onclick: () => call('Calibrate', { dev: S.devId, step: 'start' }, { okMsg: 'Calibration started' }) }, 'Start'),
        h('button', { disabled: !S.dev.connected, onclick: async () => {
          const r = await call('Calibrate', { dev: S.devId, step: 'result' }).catch(() => null);
          if (!r) return;
          if (await confirmBox('Calibration result', `Result ${r.result}, surface ${r.surface}. Save it?`, 'Save')) {
            await call('Calibrate', { dev: S.devId, step: 'save', surface: r.surface }, { okMsg: 'Surface saved' }).catch(() => {});
          }
        } }, 'Read & save'),
        h('button', { disabled: !S.dev.connected, onclick: () => call('Calibrate', { dev: S.devId, step: 'reset' }, { okMsg: 'Surface reset' }) }, 'Reset'))));
}

function renderMouseLighting() {
  if (!S.mouseDraft || S.mouseDraft._slot !== slot()) S.mouseDraft = { ...clone(mouseProfile()), _slot: slot() };
  const L = S.mouseDraft.lighting;
  const effects = (S.catalog && S.catalog.mouseEffects) || [];
  return h('div', { class: 'card col', style: { maxWidth: '760px' } },
    h('h2', {}, 'Mouse lighting'),
    field('Effect', select(effects.map((e) => [e.value, e.label]), L.effect, (v) => { L.effect = Number(v); })),
    h('div', { class: 'col', style: { gap: '4px' } }, h('span', { class: 'hint' }, 'LED colours'),
      h('div', { class: 'row' }, L.colors.map((c, i) => colorInput(c, (v) => { L.colors[i] = v; })),
        h('button', { class: 'small', onclick: () => { L.colors = L.colors.map(() => L.colors[0]); render(); } }, 'Same for all'))),
    field('Brightness', select([0, 25, 50, 75, 100].map((v) => [v, `${v}%`]), L.brightness, (v) => { L.brightness = Number(v); })),
    h('div', { class: 'grid2' },
      field('Direction', select([[0, 'Clockwise'], [1, 'Counter-clockwise']], L.direction, (v) => { L.direction = Number(v); })),
      field('Speed', select([[0, 'Slow'], [1, 'Normal'], [2, 'Fast']], L.speed, (v) => { L.speed = Number(v); }))),
    h('div', { class: 'row' },
      h('button', { disabled: !S.dev.connected, onclick: () => call('SetMouseLighting', { dev: S.devId, slot: slot(), lighting: L, live: true }) }, 'Preview'),
      h('button', { class: 'primary', onclick: async () => {
        await call('SetMouseLighting', { dev: S.devId, slot: slot(), lighting: L, live: false }, { okMsg: 'Lighting saved' }).catch(() => {});
        S.mouseDraft = null;
        await loadDevice();
        render();
      } }, 'Save to mouse')));
}

const MOUSE_VALUES = {
  mouse: ['left', 'right', 'middle', 'back', 'forward'],
  wheel: ['up', 'down'],
  profile: ['next', 'previous'],
  dpi: ['next', 'previous'],
  media: ['Play/Pause', 'Stop', 'Previous', 'Next', 'Volume up', 'Volume down', 'Mute', 'Media select'],
};

function renderMouseButtons() {
  const layoutButtons = (S.layout && S.layout.buttons) || [];
  const p = mouseProfile();
  return h('div', { class: 'col', style: { maxWidth: '860px' } }, layoutButtons.map((lb) => {
    const cur = p.buttons.find((b) => b.button === lb.button) || { button: lb.button, type: 'mouse', value: 'left' };
    const B = clone(cur);
    const valueInput = () => {
      if (MOUSE_VALUES[B.type]) return select(MOUSE_VALUES[B.type], B.value, (v) => { B.value = v; });
      if (B.type === 'key') return shortcutInput(B.value, (v) => { B.value = v; });
      if (B.type === 'macro') return select(S.macros.map((m) => m.name), B.value, (v) => { B.value = v; });
      if (B.type === 'sniper') return h('input', { type: 'number', value: B.value || '400', oninput: (e) => { B.value = e.target.value; } });
      if (B.type === 'program' || B.type === 'url') return h('input', { value: B.value, placeholder: B.type === 'url' ? 'https://…' : '/path/to/program', oninput: (e) => { B.value = e.target.value; } });
      return h('span', { class: 'hint' }, '—');
    };
    const holder = h('div', { class: 'grow' }, valueInput());
    return h('div', { class: 'card row' },
      h('strong', { style: { width: '90px' } }, lb.label),
      select((S.catalog && S.catalog.mouseButtonTypes) || [], B.type, (v) => { B.type = v; B.value = (MOUSE_VALUES[v] || [''])[0]; holder.innerHTML = ''; holder.append(valueInput()); }),
      holder,
      h('button', { class: 'primary', onclick: async () => {
        await call('SetMouseButton', { dev: S.devId, slot: slot(), button: B }, { okMsg: `${lb.label} saved` }).catch(() => {});
        await loadDevice();
        render();
      } }, 'Save'));
  }));
}

// ---------------------------------------------------------------------------------------
// Macros

function newMacro() {
  return { name: '', recordKeyboard: true, recordMouse: false, delayMode: 'recorded', customDelayMs: (S.settings && S.settings.defaultCustomDelayMs) || 50, playback: 'once', events: [], order: 0 };
}

function renderMacros() {
  const list = S.macros.slice().sort((a, b) => a.order - b.order);
  if (S.macroDraft == null && list.length && S.macroSel == null) {
    S.macroSel = list[0].name;
  }
  if (S.macroDraft == null && S.macroSel != null) {
    const m = list.find((x) => x.name === S.macroSel);
    S.macroDraft = m ? { ...clone(m), _old: m.name } : null;
  }
  return h('div', {},
    h('div', { class: 'header' }, h('div', { class: 'grow' }, h('h1', {}, 'Macros'), h('div', { class: 'meta' }, 'Macros are shared by all profiles and devices. Saving a macro updates every key that runs it.'))),
    h('div', { class: 'split left' },
      h('div', { class: 'col' },
        h('div', { class: 'card menu-list' },
          list.length === 0 ? h('div', { class: 'hint' }, 'No macros yet.') : null,
          list.map((m) => h('div', { class: `menu-item ${S.macroSel === m.name ? 'sel' : ''}`, onclick: () => { S.macroSel = m.name; S.macroDraft = null; render(); } }, m.name, h('span', { class: 'hint' }, `${m.events.length}`)))),
        h('div', { class: 'row' },
          h('button', { class: 'primary', onclick: () => { S.macroSel = null; S.macroDraft = { ...newMacro(), _old: null }; render(); } }, '+ New'),
          h('button', { onclick: async () => { const data = await bc.loadJson(); if (data) { const n = await call('ImportMacros', { data }).catch(() => 0); if (n) toast(`Imported ${n} macro(s)`, 'ok'); } } }, 'Import…'),
          h('button', { onclick: async () => { const data = await call('ExportMacros'); if (await bc.saveJson('macros.basecamp.json', data)) toast('Macros exported', 'ok'); } }, 'Export…'))),
      S.macroDraft ? renderMacroEditor(S.macroDraft) : h('div', { class: 'card hint' }, 'Create a macro to get started.')));
}

function eventLabel(e) {
  if (e.type === 'keydown' || e.type === 'keyup') return e.key || (e.rawcode != null ? `VK ${e.rawcode}` : '?');
  if (e.type === 'wheel') return e.rotation >= 0 ? 'Wheel up' : 'Wheel down';
  return ['', 'Left', 'Right', 'Middle'][e.button || 1] + ' button';
}

function renderMacroEditor(M) {
  const keys = (S.catalog && S.catalog.keys) || [];
  let addType = 'keytap';
  let addKey = 'A';
  const applyDelayMode = (mode) => {
    M.delayMode = mode;
    if (mode === 'none') M.events.forEach((e) => { e.delayMs = 0; });
    if (mode === 'custom') M.events.forEach((e) => { e.delayMs = M.customDelayMs; });
    render();
  };
  const add = () => {
    const d = M.delayMode === 'none' ? 0 : M.customDelayMs;
    if (addType === 'keytap') M.events.push({ type: 'keydown', key: addKey, delayMs: d }, { type: 'keyup', key: addKey, delayMs: d });
    else if (addType === 'keydown' || addType === 'keyup') M.events.push({ type: addType, key: addKey, delayMs: d });
    else if (addType === 'click') M.events.push({ type: 'mousedown', button: 1, delayMs: d }, { type: 'mouseup', button: 1, delayMs: d });
    else if (addType === 'wheelUp') M.events.push({ type: 'wheel', rotation: 1, delayMs: d });
    else if (addType === 'wheelDown') M.events.push({ type: 'wheel', rotation: -1, delayMs: d });
    render();
  };
  const move = (i, dlt) => { const j = i + dlt; if (j < 0 || j >= M.events.length) return; [M.events[i], M.events[j]] = [M.events[j], M.events[i]]; render(); };
  const save = async () => {
    const m = { ...M };
    const old = m._old;
    delete m._old;
    try {
      await call('PutMacro', { macro: m, oldName: old }, { okMsg: `Macro “${m.name}” saved` });
      S.macros = await call('GetMacros');
      S.macroSel = m.name.trim();
      S.macroDraft = null;
      render();
    } catch { /* toast */ }
  };
  const record = async () => {
    if (!S.recording) {
      try {
        await call('StartRecording', { keyboard: M.recordKeyboard, mouse: M.recordMouse });
        S.recording = true;
        toast('Recording… press Stop when done.');
      } catch { return; }
    } else {
      S.recording = false;
      const ev = await call('StopRecording').catch(() => null);
      if (ev) {
        M.events = M.events.concat(ev);
        if (M.delayMode !== 'recorded') applyDelayMode(M.delayMode);
      }
    }
    render();
  };
  return h('div', { class: 'card col' },
    h('div', { class: 'grid2' },
      field('Name', h('input', { value: M.name, oninput: (e) => { M.name = e.target.value; } })),
      field('Playback', select([['once', 'Play once'], ['hold', 'Repeat while held'], ['repeat', 'Repeat until pressed again']], M.playback, (v) => { M.playback = v; }))),
    h('div', { class: 'row' },
      check('Record keyboard', M.recordKeyboard, (v) => { M.recordKeyboard = v; }),
      check('Record mouse', M.recordMouse, (v) => { M.recordMouse = v; }),
      h('button', { class: S.recording ? 'danger' : '', onclick: record }, S.recording ? '■ Stop recording' : '⏺ Record')),
    h('div', { class: 'row' },
      field('Delays', select([['recorded', 'As recorded'], ['custom', 'Custom'], ['none', 'No delay']], M.delayMode, applyDelayMode)),
      M.delayMode === 'custom' ? field('Custom delay (ms)', h('input', { type: 'number', min: 0, value: M.customDelayMs, onchange: (e) => { M.customDelayMs = Number(e.target.value); applyDelayMode('custom'); } })) : null),
    h('table', { class: 'events' },
      h('thead', {}, h('tr', {}, h('th', {}, '#'), h('th', {}, 'Event'), h('th', {}, 'Key / button'), h('th', {}, 'Delay after (ms)'), h('th', {}))),
      h('tbody', {}, M.events.map((e, i) => h('tr', {},
        h('td', {}, i + 1),
        h('td', {}, { keydown: '↓ press', keyup: '↑ release', mousedown: '↓ press', mouseup: '↑ release', wheel: 'wheel' }[e.type]),
        h('td', {}, e.type.startsWith('key') ? select(keys.includes(e.key) ? keys : [e.key || '?', ...keys], e.key, (v) => { e.key = v; delete e.rawcode; }) : eventLabel(e)),
        h('td', {}, h('input', { type: 'number', min: 0, value: e.delayMs, disabled: M.delayMode === 'none', oninput: (ev) => { e.delayMs = Number(ev.target.value); } })),
        h('td', {}, h('div', { class: 'row', style: { gap: '4px' } },
          h('button', { class: 'small', onclick: () => move(i, -1) }, '↑'),
          h('button', { class: 'small', onclick: () => move(i, 1) }, '↓'),
          h('button', { class: 'small danger', onclick: () => { M.events.splice(i, 1); render(); } }, '✕'))))))),
    h('div', { class: 'row' },
      select([['keytap', 'Key press + release'], ['keydown', 'Key press'], ['keyup', 'Key release'], ['click', 'Left click'], ['wheelUp', 'Wheel up'], ['wheelDown', 'Wheel down']], addType, (v) => { addType = v; }),
      select(keys, addKey, (v) => { addKey = v; }),
      h('button', { onclick: add }, '+ Add event'),
      h('span', { class: 'hint' }, `${M.events.length} / 999 events. The last delay is always 0.`)),
    h('div', { class: 'row' },
      h('button', { class: 'primary', onclick: save }, 'Save macro'),
      M._old ? h('button', { class: 'danger', onclick: async () => {
        if (await confirmBox('Delete macro', `Delete “${M._old}”? Keys that run it go back to their default.`, 'Delete', 'danger')) {
          await call('DeleteMacro', { name: M._old }, { okMsg: 'Macro deleted' }).catch(() => {});
          S.macros = await call('GetMacros');
          S.macroSel = null;
          S.macroDraft = null;
          render();
        }
      } }, 'Delete') : null));
}

// ---------------------------------------------------------------------------------------
// Settings

function renderSettings() {
  const s = clone(S.settings || {});
  let approvals = null;
  const approvalsBox = h('div', { class: 'col' }, h('span', { class: 'hint' }, 'Loading…'));
  call('ListApprovals', {}, { quiet: true }).then((list) => {
    approvals = list;
    approvalsBox.innerHTML = '';
    if (!list.length) approvalsBox.append(h('span', { class: 'hint' }, 'None.'));
    for (const p of list) {
      approvalsBox.append(h('div', { class: 'row' }, h('code', { class: 'grow' }, p),
        h('button', { class: 'small danger', onclick: async () => { await call('RevokeLaunch', { path: p }); render(); } }, 'Revoke')));
    }
  }).catch(() => {});
  const save = async () => {
    try {
      await call('SetSettings', { settings: s }, { okMsg: 'Settings saved' });
      S.settings = s;
      await bc.applyAppSettings(s);
      await loadDevices();
      render();
    } catch { /* toast */ }
  };
  void approvals;
  return h('div', { style: { maxWidth: '760px' } },
    h('h1', {}, 'Settings'),
    h('div', { class: 'card col' },
      h('h2', {}, 'Application'),
      check('Start when I log in', s.startOnBoot, (v) => { s.startOnBoot = v; }),
      check('Start minimized', s.startMinimized, (v) => { s.startMinimized = v; }),
      check('Keep running in the tray when the window is closed', s.minimizeToTray, (v) => { s.minimizeToTray = v; }),
      check('Show only connected devices', s.displayOnlyConnectedDevices, (v) => { s.displayOnlyConnectedDevices = v; }),
      field('Default custom macro delay (ms)', h('input', { type: 'number', min: 0, value: s.defaultCustomDelayMs, oninput: (e) => { s.defaultCustomDelayMs = Number(e.target.value); } })),
      check('Re-apply the selected profile whenever a device is connected', s.syncOnAttach, (v) => { s.syncOnAttach = v; }),
      h('span', { class: 'hint' }, 'Off by default: rewriting the device on every connect wears its flash memory.')),
    h('div', { class: 'card col' },
      h('h2', {}, 'OBS Studio'),
      h('div', { class: 'grid2' },
        field('WebSocket port', h('input', { type: 'number', value: s.obsPort, oninput: (e) => { s.obsPort = Number(e.target.value); } })),
        field('Password', h('input', { type: 'password', value: s.obsPassword, oninput: (e) => { s.obsPassword = e.target.value; } }))),
      h('div', { class: 'row' },
        h('button', { onclick: async () => {
          await call('SetSettings', { settings: s }, { quiet: true }).catch(() => {});
          const scenes = await call('ObsQuery', { query: 'Get Scenes' }).catch(() => null);
          if (scenes) toast(`Connected to OBS. Scenes: ${scenes.join(', ') || 'none'}`, 'ok');
        } }, 'Test connection'),
        h('span', { class: 'hint' }, 'Enable the WebSocket server in OBS (Tools → WebSocket Server Settings).'))),
    h('div', { class: 'card col' },
      h('h2', {}, 'Programs allowed from device keys'),
      h('p', { class: 'hint' }, 'Programs and folders stored on a device only open if they are bound on this computer or you allowed them here.'),
      approvalsBox),
    h('div', { class: 'row', style: { marginTop: '14px' } }, h('button', { class: 'primary', onclick: save }, 'Save settings')),
    h('div', { class: 'card col', style: { marginTop: '14px' } },
      h('h2', {}, 'About'),
      h('div', { class: 'hint' }, `Base Camp Linux ${S.info.version || ''} · basecampd ${S.status ? S.status.version : '?'} · ${S.info.platform}`),
      h('div', { class: 'hint' }, 'Configuration: ', h('code', {}, (S.status && S.status.configDir) || '?')),
      h('div', { class: 'hint' }, 'Service endpoint: ', h('code', {}, S.info.endpoint || '?'))));
}

// ---------------------------------------------------------------------------------------

boot();
