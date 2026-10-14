'use strict';
// JSON-lines client for basecampd (Unix socket on Linux, named pipe on Windows).

const net = require('net');
const { EventEmitter } = require('events');

class DaemonClient extends EventEmitter {
  constructor(endpoint) {
    super();
    this.endpoint = endpoint;
    this.socket = null;
    this.connected = false;
    this.nextId = 1;
    this.pending = new Map();
    this.buffer = '';
  }

  connect() {
    return new Promise((resolve, reject) => {
      const s = net.createConnection(this.endpoint);
      let settled = false;
      s.setEncoding('utf8');
      s.once('connect', () => {
        settled = true;
        this.socket = s;
        this.connected = true;
        this.emit('connected');
        resolve();
      });
      s.on('data', (chunk) => this._onData(chunk));
      s.on('error', (err) => {
        if (!settled) {
          settled = true;
          reject(err);
        }
      });
      s.on('close', () => {
        const was = this.connected;
        this.connected = false;
        this.socket = null;
        for (const [, p] of this.pending) p.reject(new Error('daemon connection closed'));
        this.pending.clear();
        if (was) this.emit('disconnected');
      });
    });
  }

  _onData(chunk) {
    this.buffer += chunk;
    let i;
    while ((i = this.buffer.indexOf('\n')) >= 0) {
      const line = this.buffer.slice(0, i).trim();
      this.buffer = this.buffer.slice(i + 1);
      if (!line) continue;
      let msg;
      try {
        msg = JSON.parse(line);
      } catch {
        continue;
      }
      if (msg.signal) {
        this.emit('signal', msg);
      } else if (msg.id != null && this.pending.has(msg.id)) {
        const p = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        if (msg.error) p.reject(new Error(msg.error.message || 'error'));
        else p.resolve(msg.result);
      }
    }
  }

  call(method, params = {}, timeoutMs = 30000) {
    if (!this.connected) return Promise.reject(new Error('not connected to basecampd'));
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (this.pending.delete(id)) reject(new Error(`${method} timed out`));
      }, timeoutMs);
      this.pending.set(id, {
        resolve: (v) => { clearTimeout(timer); resolve(v); },
        reject: (e) => { clearTimeout(timer); reject(e); },
      });
      this.socket.write(JSON.stringify({ id, method, params }) + '\n');
    });
  }

  close() {
    if (this.socket) this.socket.end();
  }
}

module.exports = { DaemonClient };
