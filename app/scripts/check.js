'use strict';
// Syntax-check every front-end script (no dependencies needed).
const { execFileSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..', 'src');
const files = [];
(function walk(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p);
    else if (p.endsWith('.js')) files.push(p);
  }
})(root);
for (const f of files) execFileSync(process.execPath, ['--check', f], { stdio: 'inherit' });
console.log(`ok: ${files.length} files`);

// Locale packs: every translation must keep the same {placeholders} as its English key.
const vm = require('vm');
const localeDir = path.join(root, 'renderer', 'locales');
let problems = 0;
for (const f of fs.readdirSync(localeDir).filter((n) => n.endsWith('.js'))) {
  let pack = null;
  vm.runInNewContext(fs.readFileSync(path.join(localeDir, f), 'utf8'), { I18N: { register: (code, name, dict) => { pack = { code, name, dict }; } } });
  const ph = (s) => (s.match(/\{\w+\}/g) || []).sort().join();
  for (const [k, v] of Object.entries(pack.dict)) {
    if (ph(k) !== ph(v)) { console.error(`${f}: placeholder mismatch in "${k}"`); problems++; }
  }
  console.log(`locale ${pack.code}: ${Object.keys(pack.dict).length} strings`);
}
if (problems) process.exit(1);
