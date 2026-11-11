'use strict';

const $ = (s) => document.querySelector(s);
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const pad = (s, n) => String(s).padEnd(n).slice(0, n);
const lpad = (s, n) => String(s).padStart(n);
const span = (cls, s) => `<span class="${cls}">${esc(s)}</span>`;
const num = (n) => Math.round(n).toLocaleString('en-US');
const pct = (x) => (x * 100).toFixed(1) + '%';
const hms = (s) => { s = Math.max(0, Math.floor(s)); return [Math.floor(s / 3600), Math.floor(s / 60) % 60, s % 60].map((v) => String(v).padStart(2, '0')).join(':'); };
const dhms = (s) => `${Math.floor(s / 86400)}d ${hms(s % 86400)}`;
const safeUrl = (u) => typeof u === 'string' && /^https?:\/\/[^\s"'<>]+$/.test(u);
const link = (url, text, cls) => (safeUrl(url) ? `<a class="lnk ${cls || ''}" href="${esc(url)}" target="_blank" rel="noopener noreferrer">${esc(text)}</a>` : span(cls || '', text));
const linkPad = (url, text, w, cls) => link(url, text, cls) + ' '.repeat(Math.max(0, w - text.length));
const blockUrl = (base, hash) => (safeUrl(base) && /^[0-9a-f]{64}$/.test(hash || '') ? `${base.replace(/\/+$/, '')}/block/${hash}` : '');
const trunc = (s, n) => (s.length > n ? s.slice(0, n - 2) + '..' : s);
const workerLabel = (name) => {
  const i = name.indexOf('.');
  const addr = i < 0 ? name : name.slice(0, i);
  const rest = i < 0 ? '' : name.slice(i);
  return (addr.length > 12 ? addr.slice(0, 4) + '..' + addr.slice(-4) : addr) + rest;
};
// A maximized pane (depth.js) shows complete texts instead of the shortened ones.
const wide = (id) => !!document.querySelector('.p-' + id + '.front');
// Column width that fits the longest value (maximized panes only).
const colw = (vals, min) => Math.max(min, ...vals.map((v) => String(v).length)) + 2;
const fit = (s, n, w) => (w ? String(s).padEnd(n) : pad(s, n));
const fmtDiff = (n) => { if (!(n > 0)) return '-'; const u = [[1e15, 'P'], [1e12, 'T'], [1e9, 'G'], [1e6, 'M'], [1e3, 'k']]; for (const [v, c] of u) if (n >= v) return (n / v).toFixed(2) + c; return n.toFixed(n < 10 ? 2 : 0); };
const short = (h, a = 8, b = 6) => (h.length > a + b + 1 ? h.slice(0, a) + '..' + h.slice(-b) : h);

function fmtHash(h) {
  const u = ['H/s', 'kH/s', 'MH/s', 'GH/s', 'TH/s', 'PH/s', 'EH/s'];
  let i = 0;
  while (h >= 1000 && i < u.length - 1) { h /= 1000; i++; }
  return `${h.toFixed(h >= 100 ? 1 : 2)} ${u[i]}`;
}
function spark(vals, width) {
  const ch = '▁▂▃▄▅▆▇█';
  const v = vals.slice(-width);
  if (!v.length) return '';
  const lo = Math.min(...v), hi = Math.max(...v), r = hi - lo || 1;
  return v.map((x) => ch[Math.min(7, Math.floor(((x - lo) / r) * 7.999))]).join('');
}
function bar(frac, w, cls) {
  const f = Math.max(0, Math.min(1, frac));
  const n = Math.round(f * w);
  return `<span class="${cls || 'c1'}">${'█'.repeat(n)}</span><span class="dim">${'░'.repeat(w - n)}</span>`;
}
function rng(seed) {
  return () => { seed |= 0; seed = (seed + 0x6D2B79F5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
}
const hex = (r, n) => Array.from({ length: n }, () => '0123456789abcdef'[Math.floor(r() * 16)]).join('');

const LEPTON = 1e8;
const coins = (l) => (l / LEPTON).toFixed(8);
const sats = (l) => Math.round(l).toLocaleString('en-US');
const maskAddr = (a) => (a.length > 14 ? a.slice(0, 6) + '..' + a.slice(-4) : a);
const ago = (t) => { const a = Math.max(0, Date.now() / 1000 - t); return a < 90 ? Math.floor(a) + 's' : a < 3600 ? Math.floor(a / 60) + 'm' : a < 172800 ? (a / 3600).toFixed(1) + 'h' : Math.floor(a / 86400) + 'd'; };

/* ---------- sortable columns ---------- */
// Header cells made with hdr() sort their table when clicked (again: reverse). sortRows() applies the order.
const SortS = {};
const sortOf = (id, def, dir = -1) => SortS[id] || (SortS[id] = { k: def, d: dir });
// cols: [key, label, width, right-aligned, first direction (1 = ascending)]
function hdr(id, cols, def) {
  const s = sortOf(id, def);
  return cols.map(([k, label, w, right, d]) => {
    const on = s.k === k, t = on ? (right ? (s.d > 0 ? '\u25b2' : '\u25bc') + label : label + (s.d > 0 ? '\u25b2' : '\u25bc')) : label;
    return `<span class="th${on ? ' on' : ''}" data-s="${id}" data-k="${k}" data-d="${d || -1}" title="sort by ${esc(label.toLowerCase())}">${esc(right ? lpad(t, w) : pad(t, w))}</span>`;
  }).join('');
}
function sortRows(id, rows, get, def) {
  const s = sortOf(id, def), g = get[s.k] || get[def];
  return [...rows].sort((a, b) => { const x = g(a), y = g(b); return (x < y ? -1 : x > y ? 1 : 0) * s.d; });
}
document.addEventListener('click', (e) => {
  const t = e.target.closest && e.target.closest('.th'); if (!t) return;
  const s = sortOf(t.dataset.s, t.dataset.k);
  if (s.k === t.dataset.k) s.d = -s.d; else { s.k = t.dataset.k; s.d = +t.dataset.d; }
  document.dispatchEvent(new Event('depthchange'));
});

// Long hashes in a log line: short while the pane is small, complete while it is maximized (both are in the line, CSS picks one).
const hashSpans = (text) => esc(text).replace(/\b([0-9a-f]{12})([0-9a-f]{52})\b/g, '<span class="h-short">$1..</span><span class="h-full">$1$2</span>');

// Color themes, from cool and technical to dangerous and hacker-like. The palette lives in style.css (body[data-theme]); the swatches here only draw the menu.
const THEMES = [
  { id: 'bitcoin', name: 'BITCOIN', mood: 'orange on black, the original', sw: ['#f7931a', '#ffd08a', '#fff3e0'] },
  { id: 'cyan', name: 'TECH', mood: 'cool and clean', sw: ['#00e5ff', '#ff2bd6', '#f9f002'] },
  { id: 'ice', name: 'ARCTIC', mood: 'calm, icy blue', sw: ['#7cc7ff', '#b9a8ff', '#eaf6ff'] },
  { id: 'abyss', name: 'ABYSS', mood: 'deep sea, bioluminescent', sw: ['#00ffd5', '#2f7bff', '#b6fff0'] },
  { id: 'magenta', name: 'SYNTHWAVE', mood: 'neon night drive', sw: ['#ff2bd6', '#00e5ff', '#f9f002'] },
  { id: 'amber', name: 'RETRO', mood: 'old phosphor terminal', sw: ['#ffb000', '#ff6a00', '#ffe08a'] },
  { id: 'gold', name: 'GOLD', mood: 'black and gold, luxury', sw: ['#f5c542', '#fff0b0', '#ff8a3d'] },
  { id: 'candy', name: 'CANDY', mood: 'bubblegum, pastel pop', sw: ['#ff7ac8', '#7ad8ff', '#ffe27a'] },
  { id: 'toxic', name: 'BIOHAZARD', mood: 'acid green, warning stripes', sw: ['#c6ff00', '#ff00e6', '#00ffd0'] },
  { id: 'crimson', name: 'DANGER', mood: 'red alert', sw: ['#ff2a3d', '#ff7a00', '#ffd400'] },
  { id: 'matrix', name: 'MATRIX', mood: 'green on black, hacker', sw: ['#00ff41', '#00b32c', '#b6ff9e'] },
  { id: 'mono', name: 'GHOST', mood: 'monochrome, one red wire', sw: ['#f2f2f2', '#ff3b3b', '#9aa0a6'] },
  { id: 'green', name: 'FOREST', mood: 'green with cyan', sw: ['#39ff88', '#00e5ff', '#d6ff3d'] },
];
// Liquid glass refraction (idea: kube.io/blog/liquid-glass-css-svg): a displacement map for a rounded rectangle is drawn once (the pixels at the edge are pushed
// inward like light through a convex bezel, the middle stays flat), stretched over any element and applied with an SVG feDisplacementMap as backdrop-filter.
// Only Chromium accepts an SVG filter in backdrop-filter, other browsers keep the blur-only glass (no class is set there).
function glass() {
  if (document.getElementById('lgsvg') || !/Chrome\//.test(navigator.userAgent) ) return; // every Chromium browser (Chrome, Edge, Opera, Brave) has Chrome/ in its agent
  const N = 128, cv = document.createElement('canvas'); cv.width = cv.height = N;
  const g = cv.getContext('2d'), img = g.createImageData(N, N), R = 30, BEZ = 22; // corner radius and bezel width in map pixels
  for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) {
    const cx = Math.min(Math.max(x, R), N - 1 - R), cy = Math.min(Math.max(y, R), N - 1 - R), dx = x - cx, dy = y - cy;
    const d = Math.hypot(dx, dy) - R;
    const edge = Math.min(x, y, N - 1 - x, N - 1 - y), dist = d > 0 ? 0 : (Math.hypot(dx, dy) > 0 && (x < R || x > N - 1 - R) && (y < R || y > N - 1 - R) ? R - Math.hypot(dx, dy) : edge);
    const t = Math.max(0, 1 - dist / BEZ), mag = Math.pow(t, 2.2); // strongest at the very edge (convex profile)
    let nx = 0, ny = 0; const len = Math.hypot(dx, dy);
    if (len > 0.001 && (x < R || x > N - 1 - R) && (y < R || y > N - 1 - R)) { nx = dx / len; ny = dy / len; }
    else { const m = Math.min(x, y, N - 1 - x, N - 1 - y); if (m === x) nx = -1; else if (m === N - 1 - x) nx = 1; else if (m === y) ny = -1; else ny = 1; }
    const i = (y * N + x) * 4;
    img.data[i] = 128 - nx * mag * 127; img.data[i + 1] = 128 - ny * mag * 127; img.data[i + 2] = 128; img.data[i + 3] = 255; // rays bend toward the middle
  }
  g.putImageData(img, 0, 0);
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); svg.id = 'lgsvg'; svg.setAttribute('width', '0'); svg.setAttribute('height', '0'); svg.style.position = 'absolute'; svg.setAttribute('color-interpolation-filters', 'sRGB');
  svg.innerHTML = `<filter id="lg" x="0" y="0" width="100%" height="100%"><feImage href="${cv.toDataURL()}" x="0" y="0" width="100%" height="100%" preserveAspectRatio="none" result="m"/><feDisplacementMap in="SourceGraphic" in2="m" scale="26" xChannelSelector="R" yChannelSelector="G"/></filter>`;
  document.body.append(svg); document.body.classList.add('refract');
}
const Themes = (() => {
  // Themes are chosen in the terminal (`theme <name>`, `theme` lists them). The default is cyan; the choice is remembered in this browser (localStorage).
  const has = (id) => THEMES.some((t) => t.id === id);
  function set(id) {
    if (!has(id)) return false;
    document.body.dataset.theme = id;
    try { localStorage.setItem('photon-theme', id); } catch (_) { /* storage unavailable */ }
    if (id === 'candy') glass(); // the refraction map is only built when candy is chosen
    document.dispatchEvent(new Event('themechange'));
    return true;
  }
  // apply the remembered theme as early as possible (this script runs before the views are drawn)
  try { const s = localStorage.getItem('photon-theme'); if (s && has(s)) { const apply = () => { document.body.dataset.theme = s; if (s === 'candy') glass(); }; if (document.body) apply(); else document.addEventListener('DOMContentLoaded', apply); } } catch (_) { /* storage unavailable */ }
  const listHtml = () => THEMES.map((t) => `${span(document.body.dataset.theme === t.id ? 'ok' : 'c1', pad(t.id, 9))}${span('dim', t.mood)}`).join('\n');
  return { set, list: () => THEMES.map((t) => t.id), listHtml };
})();


/* footer: the release name (v<node>-p<pool>) comes from the pool */
(function () {
  const el = document.getElementById('release');
  if (!el) return;
  const take = (j) => { const r = j && (j.release || (j.pool && j.pool.release)); if (r) el.textContent = 'Photon Pool ' + r; return !!r; };
  fetch('/public/v1/pool', { cache: 'no-store' }).then((r) => r.json()).then(take).catch(() => {}).then(() => { if (el.textContent === 'Photon Pool') return fetch('/api/v1/summary', { cache: 'no-store' }).then((r) => r.json()).then(take).catch(() => {}); });
})();


/* tab title: "<view> | <pool name> for Elektron Net", the start page adds what the pool is */
function pageTitle(part, name) {
  const n = (name || (document.getElementById('pool-name') || {}).textContent || 'Photon Pool').trim();
  const base = (/^pool$/i.test(n) ? 'Photon Pool' : n) + ' for Bitcoin';
  return part ? part + ' | ' + base : base + ' | Fast SHA-256d Mining Pool';
}

/* "in 23m" for a time in the future */
const until = (t) => { const a = Math.max(0, t - Date.now() / 1000); return a < 90 ? Math.ceil(a) + 's' : a < 5400 ? Math.round(a / 60) + 'm' : (a / 3600).toFixed(1) + 'h'; };
const everyText = (s) => (s % 3600 === 0 ? s / 3600 + ' h' : s % 60 === 0 ? s / 60 + ' min' : s + ' s');
/* one line about the payout run: live or dry run, how often, when next */
function payoutLine(po) {
  if (!po) return '';
  if (!po.wallet) return 'no payout wallet';
  if (po.dryRun) return 'dry run: payouts are calculated but nothing is sent';
  return 'payout run every ' + everyText(po.intervalSecs) + (po.nextRunAt ? ', next in ' + until(po.nextRunAt) : '');
}


/* pagination for lists that grow: a bar "1-20 of 137  < newer  older >"; pageRows() pages a list that is already loaded, the views fetch further pages of
   the long server lists themselves and use pagerBar() for the bar */
function pagerBar(attrs, off, shown, total, size) {
  const btn = (d, label, on) => (on ? `<a class="pgb" href="#" ${attrs} data-d="${d}">${label}</a>` : span('dim', label));
  return `${span('dim', `${num(off + 1)}-${num(off + shown)} of ${num(total)}  `)}${btn(-1, '\u2039 newer', off > 0)}  ${btn(1, 'older \u203a', off + size < total)}`;
}
const PgState = {};
function pageRows(id, rows, size) {
  const s = PgState[id] || (PgState[id] = { off: 0 });
  if (s.off >= rows.length) s.off = Math.max(0, Math.floor((rows.length - 1) / size) * size);
  const part = rows.slice(s.off, s.off + size);
  return { rows: part, off: s.off, bar: rows.length > size ? ['', pagerBar(`data-pgid="${id}" data-size="${size}"`, s.off, part.length, rows.length, size)] : [] };
}
document.addEventListener('click', (e) => {
  const b = e.target.closest && e.target.closest('.pgb[data-pgid]'); if (!b) return;
  e.preventDefault();
  const s = PgState[b.dataset.pgid] || (PgState[b.dataset.pgid] = { off: 0 });
  s.off = Math.max(0, s.off + (+b.dataset.d) * (+b.dataset.size));
  document.dispatchEvent(new Event('depthchange'));
});
