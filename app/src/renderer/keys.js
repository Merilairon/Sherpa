'use strict';
// KeyboardEvent.code → canonical key names used by the daemon (see basecamp-core keys.rs).
window.KEYCODES = (() => {
  const m = {
    Escape: 'Esc', Backquote: '`', Minus: '-', Equal: '=', Backspace: 'Backspace', Tab: 'Tab',
    BracketLeft: '[', BracketRight: ']', Enter: 'Enter', Semicolon: ';', Quote: "'", Backslash: '\\',
    IntlBackslash: '\\', Comma: ',', Period: '.', Slash: '/', Space: 'Space', CapsLock: 'CapsLock',
    NumLock: 'NumLock', ScrollLock: 'ScrollLock', PrintScreen: 'PrintScreen', Pause: 'Pause',
    Insert: 'Insert', Delete: 'Delete', Home: 'Home', End: 'End', PageUp: 'PageUp', PageDown: 'PageDown',
    ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right', ContextMenu: 'Menu',
    NumpadMultiply: 'Num *', NumpadAdd: 'Num +', NumpadSubtract: 'Num -', NumpadDecimal: 'Num .',
    NumpadDivide: 'Num /', NumpadEnter: 'Num Enter',
    ControlLeft: 'LCtrl', ControlRight: 'RCtrl', ShiftLeft: 'LShift', ShiftRight: 'RShift',
    AltLeft: 'LAlt', AltRight: 'RAlt', MetaLeft: 'LWin', MetaRight: 'RWin', OSLeft: 'LWin', OSRight: 'RWin',
    AudioVolumeMute: 'Mute', AudioVolumeDown: 'VolumeDown', AudioVolumeUp: 'VolumeUp',
    MediaTrackNext: 'NextTrack', MediaTrackPrevious: 'PrevTrack', MediaPlayPause: 'PlayPause', MediaStop: 'StopMedia',
  };
  for (let i = 0; i < 26; i++) { const c = String.fromCharCode(65 + i); m['Key' + c] = c; }
  for (let i = 0; i < 10; i++) { m['Digit' + i] = String(i); m['Numpad' + i] = 'Num ' + i; }
  for (let i = 1; i <= 24; i++) m['F' + i] = 'F' + i;
  return m;
})();

window.MODIFIER_CODES = new Set(['ControlLeft', 'ControlRight', 'ShiftLeft', 'ShiftRight', 'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight', 'OSLeft', 'OSRight']);

// Build "Ctrl+Shift+X" from a keydown event; null for modifier-only presses.
window.shortcutFromEvent = (e) => {
  if (window.MODIFIER_CODES.has(e.code)) return null;
  const name = window.KEYCODES[e.code];
  if (!name) return null;
  const parts = [];
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.shiftKey) parts.push('Shift');
  if (e.altKey) parts.push('Alt');
  if (e.metaKey) parts.push('Win');
  parts.push(name);
  return parts.join('+');
};
