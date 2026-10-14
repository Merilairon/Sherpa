'use strict';
// Electron main process: window, tray, autostart, and the bridge to sherpad.

const { app, BrowserWindow, Tray, Menu, ipcMain, dialog, nativeImage, shell } = require('electron');
const path = require('path');
const fs = require('fs');
const os = require('os');
const { spawn, execFileSync } = require('child_process');
const { DaemonClient } = require('./daemon-client');

const isWin = process.platform === 'win32';
const FAKE = process.argv.includes('--fake');
const HIDDEN = process.argv.includes('--hidden');
const EXE = isWin ? 'sherpad.exe' : 'sherpad';

let win = null;
let tray = null;
let client = null;
let quitting = false;
let appSettings = { minimizeToTray: true, startMinimized: false, startOnBoot: false };
let daemonInfo = { endpoint: '', binary: '', spawned: false, connected: false, error: '' };

// App-window settings are cached locally so "start minimized" works before the daemon answers.
function appSettingsFile() {
  return path.join(app.getPath('userData'), 'window-settings.json');
}
function loadAppSettings() {
  try { appSettings = { ...appSettings, ...JSON.parse(fs.readFileSync(appSettingsFile(), 'utf8')) }; } catch { /* first run */ }
}
function saveAppSettings() {
  try {
    fs.mkdirSync(path.dirname(appSettingsFile()), { recursive: true });
    fs.writeFileSync(appSettingsFile(), JSON.stringify(appSettings));
  } catch { /* not fatal */ }
}

if (!app.requestSingleInstanceLock()) {
  app.quit();
}

function daemonBinary() {
  const candidates = [];
  if (process.env.SHERPAD_PATH) candidates.push(process.env.SHERPAD_PATH);
  if (app.isPackaged) candidates.push(path.join(process.resourcesPath, 'bin', EXE));
  const root = path.join(__dirname, '..', '..');
  candidates.push(path.join(root, 'target', 'release', EXE));
  candidates.push(path.join(root, 'target', 'debug', EXE));
  candidates.push(path.join(root, 'dist', 'bin', isWin ? 'win' : 'linux', EXE));
  return candidates.find((p) => { try { return fs.statSync(p).isFile(); } catch { return false; } }) || null;
}

function defaultEndpoint(bin) {
  if (bin) {
    try {
      return execFileSync(bin, ['--print-socket'], { encoding: 'utf8', timeout: 5000, windowsHide: true }).trim();
    } catch { /* fall through */ }
  }
  if (isWin) return `\\\\.\\pipe\\sherpad-${(process.env.USERNAME || 'user').replace(/[^\w-]/g, '')}`;
  const dir = process.env.XDG_RUNTIME_DIR;
  return dir ? path.join(dir, 'sherpad.sock') : `/tmp/sherpad-${process.getuid()}.sock`;
}

function simEndpoint(ep) {
  return isWin ? `${ep}-sim` : ep.replace(/\.sock$/, '-sim.sock');
}

function spawnDaemon(bin, endpoint) {
  const logFile = path.join(app.getPath('userData'), FAKE ? 'sherpad-sim.log' : 'sherpad.log');
  fs.mkdirSync(path.dirname(logFile), { recursive: true });
  const out = fs.openSync(logFile, 'a');
  const args = ['--socket', endpoint];
  if (FAKE) args.push('--fake');
  const child = spawn(bin, args, { detached: true, stdio: ['ignore', out, out], windowsHide: true });
  child.unref();
  daemonInfo.spawned = true;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function connectDaemon() {
  const bin = daemonBinary();
  daemonInfo.binary = bin || '';
  let endpoint = defaultEndpoint(bin);
  if (FAKE) endpoint = simEndpoint(endpoint);
  daemonInfo.endpoint = endpoint;
  client = new DaemonClient(endpoint);
  client.on('signal', (msg) => { if (win) win.webContents.send('bc:signal', msg); });
  client.on('disconnected', () => {
    daemonInfo.connected = false;
    if (win) win.webContents.send('bc:signal', { signal: 'DaemonDisconnected', data: {} });
    if (!quitting) setTimeout(() => reconnect(), 1000);
  });
  await reconnect(true);
}

async function reconnect(first = false) {
  for (let attempt = 0; attempt < 40 && !quitting; attempt++) {
    try {
      await client.connect();
      daemonInfo.connected = true;
      daemonInfo.error = '';
      if (win) win.webContents.send('bc:signal', { signal: 'DaemonConnected', data: {} });
      return;
    } catch (err) {
      if (attempt === 0 && (first || !daemonInfo.spawned)) {
        if (daemonInfo.binary) spawnDaemon(daemonInfo.binary, daemonInfo.endpoint);
        else daemonInfo.error = 'sherpad binary not found';
      }
      daemonInfo.error = daemonInfo.error || err.message;
      await sleep(250);
    }
  }
  if (win) win.webContents.send('bc:signal', { signal: 'DaemonUnavailable', data: { error: daemonInfo.error } });
}

function iconPath() {
  return path.join(__dirname, '..', 'assets', 'icon.png');
}

function createWindow() {
  win = new BrowserWindow({
    width: 1280,
    height: 820,
    minWidth: 960,
    minHeight: 640,
    show: false,
    title: FAKE ? 'Sherpa (simulated devices)' : 'Sherpa',
    icon: iconPath(),
    backgroundColor: '#14161a',
    autoHideMenuBar: true,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  win.loadFile(path.join(__dirname, 'renderer', 'index.html'));
  win.once('ready-to-show', () => {
    if (!(HIDDEN || appSettings.startMinimized)) win.show();
  });
  win.on('close', (e) => {
    if (!quitting && appSettings.minimizeToTray && tray) {
      e.preventDefault();
      win.hide();
    }
  });
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https?:\/\//.test(url)) shell.openExternal(url);
    return { action: 'deny' };
  });
  win.webContents.on('will-navigate', (e) => e.preventDefault());
}

function showWindow() {
  if (!win) createWindow();
  win.show();
  win.focus();
}

function createTray() {
  try {
    const img = nativeImage.createFromPath(iconPath()).resize({ width: 22, height: 22 });
    tray = new Tray(img);
  } catch {
    tray = null;
    return;
  }
  tray.setToolTip('Sherpa');
  tray.setContextMenu(Menu.buildFromTemplate([
    { label: 'Open Sherpa', click: showWindow },
    { type: 'separator' },
    { label: 'Quit (keep device service running)', click: () => { quitting = true; app.quit(); } },
    {
      label: 'Quit and stop device service',
      click: async () => {
        quitting = true;
        try { await client.call('Shutdown', {}, 2000); } catch { /* already gone */ }
        app.quit();
      },
    },
  ]));
  tray.on('click', showWindow);
}

// --- autostart -------------------------------------------------------------------------

function linuxAutostartFile() {
  const base = process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config');
  return path.join(base, 'autostart', 'sherpa.desktop');
}

function applyStartOnBoot(enabled) {
  if (process.platform === 'linux') {
    const file = linuxAutostartFile();
    if (enabled) {
      const exec = process.env.APPIMAGE || process.execPath;
      const extra = app.isPackaged ? '' : ` "${path.join(__dirname, '..')}"`;
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, [
        '[Desktop Entry]',
        'Type=Application',
        'Name=Sherpa',
        `Exec="${exec}"${extra} --hidden`,
        'X-GNOME-Autostart-enabled=true',
        'Terminal=false',
        '',
      ].join('\n'));
    } else {
      try { fs.unlinkSync(file); } catch { /* not present */ }
    }
  } else {
    app.setLoginItemSettings({ openAtLogin: enabled, args: ['--hidden'] });
  }
}

// --- IPC from the renderer -------------------------------------------------------------

ipcMain.handle('bc:call', async (_e, method, params) => {
  if (!client || !client.connected) throw new Error('Not connected to the Sherpa device service (sherpad).');
  return client.call(method, params);
});

ipcMain.handle('bc:info', () => ({
  ...daemonInfo,
  connected: !!(client && client.connected),
  fake: FAKE,
  platform: process.platform,
  version: app.getVersion(),
}));

ipcMain.handle('bc:openFile', async (_e, opts) => {
  const r = await dialog.showOpenDialog(win, { properties: ['openFile'], filters: opts.filters || [] });
  return r.canceled ? null : r.filePaths[0];
});

ipcMain.handle('bc:openFolder', async () => {
  const r = await dialog.showOpenDialog(win, { properties: ['openDirectory'] });
  return r.canceled ? null : r.filePaths[0];
});

ipcMain.handle('bc:saveJson', async (_e, name, data) => {
  const r = await dialog.showSaveDialog(win, { defaultPath: name, filters: [{ name: 'JSON', extensions: ['json'] }] });
  if (r.canceled || !r.filePath) return false;
  fs.writeFileSync(r.filePath, JSON.stringify(data, null, 2));
  return true;
});

ipcMain.handle('bc:loadJson', async () => {
  const r = await dialog.showOpenDialog(win, { properties: ['openFile'], filters: [{ name: 'JSON', extensions: ['json'] }] });
  if (r.canceled) return null;
  return JSON.parse(fs.readFileSync(r.filePaths[0], 'utf8'));
});

ipcMain.handle('bc:readImage', async (_e, file) => {
  const ext = path.extname(file).toLowerCase().replace('.', '');
  const mime = { png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', bmp: 'image/bmp', webp: 'image/webp' }[ext];
  if (!mime) throw new Error('unsupported image type');
  return `data:${mime};base64,${fs.readFileSync(file).toString('base64')}`;
});

ipcMain.handle('bc:applyAppSettings', (_e, s) => {
  const bootChanged = !!s.startOnBoot !== !!appSettings.startOnBoot;
  appSettings = {
    ...appSettings,
    minimizeToTray: !!s.minimizeToTray,
    startMinimized: !!s.startMinimized,
    startOnBoot: !!s.startOnBoot,
  };
  saveAppSettings();
  if (bootChanged) applyStartOnBoot(!!s.startOnBoot);
  return true;
});

// --- lifecycle -------------------------------------------------------------------------

app.on('second-instance', showWindow);

app.whenReady().then(async () => {
  Menu.setApplicationMenu(null);
  loadAppSettings();
  createTray();
  createWindow();
  await connectDaemon();
});

app.on('before-quit', () => { quitting = true; });

app.on('window-all-closed', () => {
  if (!tray || !appSettings.minimizeToTray) app.quit();
});
