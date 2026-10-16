'use strict';
// UI localisation. The English source text is the message key: `t('Save')`. Static strings
// handed to h() (text children, placeholder, title) are translated automatically; strings
// that embed values use `t('Profile {n}', { n })`. A missing translation falls back to English.
window.I18N = (() => {
  const packs = {}; // code → { name, dict }
  let lang = 'en';
  const api = {
    register(code, name, dict) { packs[code] = { name, dict }; },
    languages() { return [['en', 'English'], ...Object.entries(packs).map(([c, p]) => [c, p.name])]; },
    get lang() { return lang; },
    set(code) {
      lang = packs[code] ? code : 'en';
      document.documentElement.lang = lang;
    },
    t(text, params) {
      const p = packs[lang];
      let out = (p && Object.prototype.hasOwnProperty.call(p.dict, text) ? p.dict[text] : text);
      if (params) out = out.replace(/\{(\w+)\}/g, (m, k) => (k in params ? params[k] : m));
      return out;
    },
  };
  return api;
})();
window.t = (text, params) => window.I18N.t(text, params);
