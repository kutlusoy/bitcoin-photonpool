window.Views = window.Views || {};
// Engine view: what the pool is doing right now (chain tip, allocator, share quality, a sanitized event feed). It reads only the public API,
// so it is the same on the public and the operator port and shows no worker, address or node detail.
Views.engine = (() => {
'use strict';
const timers = [], offs = [];
const on = (t, ev, fn, o) => { t.addEventListener(ev, fn, o); offs.push(() => t.removeEventListener(ev, fn, o)); };
const TEMPLATE = `<div class="statusline" id="statusline" role="status" aria-live="off"></div>
  <main class="grid engine">
    <section class="pane p-tip" aria-label="Chain tip"><h2><span>TIP</span></h2><pre id="pane-tip"></pre></section>
    <section class="pane p-alloc" aria-label="Allocator"><h2><span>ALLOCATOR</span></h2><pre id="pane-alloc"></pre></section>
    <section class="pane p-shares" aria-label="Shares"><h2><span>SHARES</span></h2><pre id="pane-shares"></pre></section>
    <section class="pane p-events" aria-label="Events"><h2><span>EVENTS</span><em>public feed, no miner data</em></h2><div class="tty" id="events" tabindex="-1"></div></section>
    <section class="pane p-flow" aria-label="Engine flow"><h2><span>PHOTON CORE</span><em>live data</em></h2><div class="flowbox"><canvas id="flow"></canvas></div></section>
  </main>`;
let flow = null;
const view = { s: null, seq: 0, lastTip: { h: null, t: 0 }, follow: true };

// The 3D scene needs Three.js (served from the pool itself, loaded only here); without WebGL or when loading fails the light 2D scene is used.
const loadScript = (src) => new Promise((ok, no) => { if (document.querySelector(`script[src="${src}"]`)) return ok(); const s = document.createElement('script'); s.src = src; s.onload = ok; s.onerror = no; document.head.append(s); });
async function startFlow() {
  const canvas = $('#flow'); if (!canvas) return;
  let make = null;
  try {
    const gl = (() => { try { const c = document.createElement('canvas'); return !!(c.getContext('webgl2') || c.getContext('webgl')); } catch (_) { return false; } })();
    // phones and other touch-only devices get the light 2D scene (no WebGL, no 600 KB library)
    const phone = matchMedia('(pointer: coarse) and (hover: none)').matches || matchMedia('(max-width: 720px)').matches;
    if (gl && !phone) { await loadScript('/vendor/three.min.js'); await loadScript('/scene3d.js'); if (window.Flow3D) make = Flow3D; }
  } catch (_) { /* fall back to the 2D scene */ }
  if (!$('#flow')) return; // the view was left while loading
  try { flow = (make || Flow).create(canvas); } catch (_) { flow = Flow.create(canvas); }
  flow.start(); if (view.s) { const s = view.s; flow.feed({ modes: s.modes, jobs: s.net ? s.net.jobsPerSec : 0, shares: s.shares.perSec, cands: s.alloc.candidates, miners: s.miners.online, ok: s.shares.okN, stale: s.shares.staleN, bad: s.shares.dupN + s.shares.invN, epochUse: s.alloc.epochUse, batchMs: s.node.batchMs }); flow.tip(s.node.height); }
}

const get = async (p) => { const r = await fetch(p, { cache: 'no-store' }); if (!r.ok) throw new Error(r.status); return r.json(); };

// A found block: a banner over the scene (also with the 2D fallback) and a highlighted line in the feed.
function banner(h, mode) {
  const box = document.querySelector('.flowbox'); if (!box) return;
  const b = document.createElement('div'); b.className = 'flowbanner'; b.textContent = 'BLOCK FOUND' + (h ? ' ' + h : '') + (mode ? ' \u00b7 ' + mode.toUpperCase() : '');
  let stack = box.querySelector('.flowbanners');
  if (!stack) { stack = document.createElement('div'); stack.className = 'flowbanners'; box.appendChild(stack); }
  while (stack.children.length >= 3) stack.firstChild.remove();
  stack.appendChild(b); setTimeout(() => { b.remove(); if (!stack.children.length) stack.remove(); }, 5200);
}

function addEvent(e) {
  const box = $('#events');
  if (!box) return;
  const d = document.createElement('div');
  const t = new Date(e.t * 1000).toTimeString().slice(0, 8), lv = String(e.lvl || 'info').toLowerCase().replace(/[^a-z]/g, '');
  d.innerHTML = `<span class="t">${t}</span> <span class="lv lv-${lv}">${esc(lv.toUpperCase().padEnd(5))}</span>${hashSpans(e.msg)}`;
  if (String(e.lvl) === 'blk') d.className = 'blkrow';
  box.appendChild(d);
  while (box.childElementCount > 300) box.firstChild.remove();
  settle();
}

// Event feed: one line per event that is never wrapped (the font shrinks until the longest line fits, which matters when the pane is maximized and the
// hashes are complete), and a follow mode that only the reader switches off (wheel, keys, touch or dragging the scrollbar), not layout changes.
let settleRaf = 0, ours = 0;
function toBottom(smooth) {
  const box = $('#events'); if (!box) return;
  ours = performance.now() + 450; // scroll events until then come from us
  box.scrollTo({ top: box.scrollHeight, behavior: smooth ? 'smooth' : 'auto' });
}
function fitEvents() {
  const box = $('#events'); if (!box) return;
  box.style.fontSize = '';
  if (box.scrollWidth <= box.clientWidth + 1) return;
  const px = parseFloat(getComputedStyle(box).fontSize);
  box.style.fontSize = Math.max(8, Math.floor(px * box.clientWidth / box.scrollWidth * 100) / 100 - 0.2) + 'px';
}
function settle() {
  if (settleRaf) return;
  settleRaf = requestAnimationFrame(() => { settleRaf = 0; fitEvents(); if (view.follow) toBottom(true); });
}

// Photon gateways: counters of the public engine data turned into rates between two polls
let pPrev = null;
function photonRates(s) {
  const p = s.photon; if (!p || !p.enabled) { pPrev = null; return { photon: false }; }
  const now = performance.now(), r = { photon: true, gateways: p.connections || 0, channels: p.channels || 0 };
  if (pPrev && now > pPrev.t) { const dt = (now - pPrev.t) / 1000; r.pshares = Math.max(0, ((p.sharesOk || 0) + (p.sharesRejected || 0)) - pPrev.sh) / dt; const tot = Math.max(1, ((p.sharesOk || 0) + (p.sharesRejected || 0)) - pPrev.sh); r.pRej = Math.max(0, (p.sharesRejected || 0) - pPrev.rej) / tot; r.checks = Math.max(0, ((p.checksOk || 0) + (p.checksFailed || 0)) - pPrev.ck) / dt; }
  pPrev = { t: now, sh: (p.sharesOk || 0) + (p.sharesRejected || 0), rej: p.sharesRejected || 0, ck: (p.checksOk || 0) + (p.checksFailed || 0) };
  return r;
}

async function refresh() {
  const [s, ev] = await Promise.all([get('/public/v1/engine'), get('/public/v1/events?since=' + view.seq)]);
  view.s = s;
  const n = s.node;
  Series.seed('hr', s.hashrate.hist);
  Series.push('p50', n.p50); Series.push('p95', n.p95);
  Series.push('units', s.alloc.unitsPerSec); Series.push('epoch', s.alloc.epochUse * 100);
  Series.push('sps', s.shares.perSec);
  if (view.lastTip.h !== null && n.height !== view.lastTip.h && view.lastTip.t) Series.push('tipint', n.tipTime - view.lastTip.t, n.tipTime);
  if (n.height !== view.lastTip.h) view.lastTip = { h: n.height, t: n.tipTime };
  if (flow) { flow.feed({ jobs: s.net ? s.net.jobsPerSec : 0, shares: s.shares.perSec, units: s.alloc.unitsPerSec, cands: s.alloc.candidates, hr: s.hashrate.now, miners: s.miners.online, ok: s.shares.okN, stale: s.shares.staleN, bad: s.shares.dupN + s.shares.invN, epochUse: s.alloc.epochUse, batchMs: n.batchMs, backend: n.backend, modes: s.modes, units: s.alloc.unitsPerSec, tipTime: n.tipTime, rtt: n.rttMs, p50: n.p50, p95: n.p95, connected: n.connected, ...photonRates(s) }); flow.tip(n.height); }
  (ev.events || []).forEach((x) => { addEvent(x); if (x.lvl === 'blk') { const h = (/BLOCK FOUND (\d+)/.exec(x.msg) || [])[1], mode = (/\[(solo|pplns|pps)\]/.exec(x.msg) || [])[1]; if (flow) flow.block(h, mode); banner(h, mode); } });
  if (typeof ev.last === 'number') view.seq = ev.last;
}

function render() {
  const s = view.s;
  if (!s) return;
  const n = s.node, age = Date.now() / 1000 - n.tipTime;
  $('#statusline').innerHTML = [
    `<span>height <b>${num(n.height)}</b></span>`,
    `<span class="${n.connected ? 'ok' : 'warn'}">node <b>${n.connected ? 'UP' : 'DOWN'}</b></span>`,
    `<span>tip age <b>${hms(age)}</b></span>`,
    `<span>job latency <b>${n.p50.toFixed(0)}</b> ms p50 / <b>${n.p95.toFixed(0)}</b> ms p95</span>`,
    `<span>candidates <b>${num(s.alloc.candidates)}</b></span>`,
    `<span>pool <b>${fmtHash(s.hashrate.now)}</b></span>`,
  ].join('');
  $('#pane-tip').innerHTML = [
    `${span('dim', 'height    ')}${span('hl', num(n.height))}`,
    `${span('dim', 'hash      ')}${link(blockUrl(s.pool.explorerUrl, n.tip), wide('tip') ? n.tip : short(n.tip, 10, 8), 'c1')}`,
    `${span('dim', 'age       ')}${hms(age)} ${bar(age / 60, 14, age > 75 ? 'warn' : 'c1')}`,
    `${span('dim', 'node link ')}${n.connected ? span('ok', 'connected') : span('err', 'DOWN')} rtt ${n.rttMs.toFixed(1)}ms`,
    `${span('dim', 'tip>job   ')}p50 ${n.p50.toFixed(0)}ms  p95 ${n.p95.toFixed(0)}ms  ${span('dim', 'n=' + n.samples)}`,
    `${span('dim', 'backend   ')}${esc(n.backend || 'gbt')}  ${span('dim', 'batch rpc')} ${n.batchMs.toFixed(0)} ms`,
  ].join('\n');
  $('#pane-alloc').innerHTML = [
    `${span('dim', 'candidates ')}${num(s.alloc.candidates)}${n.candidatesAvailable ? span('dim', ' of ' + num(n.candidatesAvailable) + ' coinbase variant(s)') : ''}`,
    `${span('dim', 'units/s    ')}${num(s.alloc.unitsPerSec)}`,
    `${span('dim', 'epoch use  ')}${bar(s.alloc.epochUse, 14, s.alloc.epochUse > 0.9 ? 'warn' : 'c2')} ${pct(s.alloc.epochUse)}`,
    `${span('dim', 'overlap    ')}${s.alloc.overlap ? span('err', s.alloc.overlap + ' flagged') : span('ok', '0 flagged')}`,
    `${span('dim', 'unit       ')}${span('dim', 'extranonce1 per connection, extranonce2 and ntime rolled by the miner')}`,
  ].join('\n');
  $('#pane-shares').innerHTML = [
    `${span('dim', 'shares/s  ')}${s.shares.perSec.toFixed(1)}`,
    `${span('dim', 'accepted  ')}${span('ok', pct(s.shares.ok))}  ${num(s.shares.okN)}`,
    `${span('dim', 'stale     ')}${span('warn', pct(s.shares.stale))}  ${num(s.shares.staleN)}`,
    `${span('dim', 'duplicate ')}${pct(s.shares.dup)}  ${num(s.shares.dupN)}`,
    `${span('dim', 'invalid   ')}${span('err', pct(s.shares.inv))}  ${num(s.shares.invN)}`,
    `${span('dim', 'total     ')}${num(s.shares.total)}`,
  ].join('\n');
  Extras.tick();
}

const card = Extras.card;
const timeLine = (host, series, fmt, extra = {}) => Neon.line(host, { series, fmt, xfmt: agoLabel, ...extra });

Extras.add('tip', (el) => {
  const k = card(el, 'chain and node link', 'wide'), lat = card(el, 'tip to job latency (ms)', 'wide'), iv = card(el, 'seconds between new tips', 'wide');
  return () => {
    const s = view.s; if (!s) return;
    Neon.kpis(k, [{ label: 'height', value: s.node.height, fmt: num }, { label: 'tip age', value: Date.now() / 1000 - s.node.tipTime, fmt: (v) => hms(v), color: 'c2' }, { label: 'latency p50', value: s.node.p50, fmt: (v) => v.toFixed(0) + ' ms', color: 'c3' },
      { label: 'latency p95', value: s.node.p95, fmt: (v) => v.toFixed(0) + ' ms', color: 'c3' }, { label: 'rpc rtt', value: s.node.rttMs, fmt: (v) => v.toFixed(1) + ' ms', color: 'c2' }, { label: 'batch rpc', value: s.node.batchMs, fmt: (v) => v.toFixed(0) + ' ms', color: 'c2' }]);
    timeLine(lat, [{ name: 'p50', data: Series.get('p50'), color: 'c1' }, { name: 'p95', data: Series.get('p95'), color: 'c3' }], (v) => v.toFixed(0));
    const ti = Series.get('tipint');
    if (ti.length) Neon.columns(iv, ti.slice(-40).map((p) => ({ key: p.x, label: '', value: p.y, text: Math.round(p.y) + ' s', tip: agoLabel(p.x) })), { color: 'c2' });
    else iv.innerHTML = '<span class="dim">no new tip seen yet, waiting for the next block</span>';
  };
});

Extras.add('alloc', (el) => {
  const k = card(el, 'allocator', 'wide'), u = card(el, 'units per second'), e = card(el, 'epoch use (%)'), g = card(el, 'epoch use');
  return () => {
    const s = view.s; if (!s) return;
    Neon.kpis(k, [{ label: 'candidates', value: s.alloc.candidates, fmt: num }, { label: 'units / s', value: s.alloc.unitsPerSec, fmt: num, color: 'c2' }, { label: 'epoch use', value: s.alloc.epochUse * 100, fmt: (v) => v.toFixed(1) + '%', color: 'c3' },
      { label: 'overlap flagged', value: s.alloc.overlap, fmt: num, color: s.alloc.overlap ? 'err' : 'ok' }]);
    timeLine(u, [{ name: 'units/s', data: Series.get('units'), color: 'c2' }], (v) => (Math.abs(v) < 10 && v % 1 ? v.toFixed(1) : num(v)));
    timeLine(e, [{ name: 'epoch use', data: Series.get('epoch'), color: 'c3' }], (v) => v.toFixed(1) + '%');
    Neon.donut(g, [{ label: 'used', value: s.alloc.epochUse, color: s.alloc.epochUse > 0.9 ? 'warn' : 'c3', text: pct(s.alloc.epochUse) }, { label: 'free', value: Math.max(0, 1 - s.alloc.epochUse), color: 'c1', text: pct(1 - s.alloc.epochUse) }], { big: pct(s.alloc.epochUse), sub: 'of the epoch' });
  };
});

Extras.add('shares', (el) => {
  const k = card(el, 'share quality', 'wide'), q = card(el, 'share outcome'), r = card(el, 'shares per second');
  return () => {
    const s = view.s; if (!s) return;
    Neon.kpis(k, [{ label: 'shares / s', value: s.shares.perSec, fmt: (v) => v.toFixed(1), color: 'c3' }, { label: 'accepted', value: s.shares.ok * 100, fmt: (v) => v.toFixed(2) + '%', color: 'ok', sub: num(s.shares.okN) + ' shares' },
      { label: 'stale', value: s.shares.stale * 100, fmt: (v) => v.toFixed(2) + '%', color: 'warn', sub: num(s.shares.staleN) + ' shares' }, { label: 'duplicate / invalid', value: s.shares.dupN + s.shares.invN, fmt: num, color: 'c3', sub: `${num(s.shares.dupN)} duplicate, ${num(s.shares.invN)} invalid` }, { label: 'total', value: s.shares.total, fmt: num, color: 'c2', sub: 'shares since start' }]);
    Neon.donut(q, [{ label: 'accepted', value: s.shares.ok, color: 'ok', text: pct(s.shares.ok) + '  ' + num(s.shares.okN) }, { label: 'stale', value: s.shares.stale, color: 'warn', text: pct(s.shares.stale) + '  ' + num(s.shares.staleN) }, { label: 'duplicate', value: s.shares.dup, color: 'c3', text: pct(s.shares.dup) + '  ' + num(s.shares.dupN) }, { label: 'invalid', value: s.shares.inv, color: 'err', text: pct(s.shares.inv) + '  ' + num(s.shares.invN) }], { big: num(s.shares.total), sub: 'shares' });
    timeLine(r, [{ name: 'shares/s', data: Series.get('sps'), color: 'c2' }], (v) => v.toFixed(1));
  };
});

async function mount(root) {
  root.innerHTML = TEMPLATE;
  view.seq = 0; view.s = null; view.follow = true;
  $('#who').textContent = 'engine';
  const tag = $('#mode-tag'); tag.textContent = 'LIVE'; tag.className = 'tag live';
  const box = $('#events');
  const near = () => box.scrollTop + box.clientHeight >= box.scrollHeight - 8;
  on(box, 'scroll', () => { if (performance.now() > ours) view.follow = near(); });
  on(box, 'wheel', () => { setTimeout(() => { view.follow = near(); }, 80); }, { passive: true });
  on(box, 'touchmove', () => { view.follow = near(); }, { passive: true });
  on(box, 'keydown', () => { setTimeout(() => { view.follow = near(); }, 0); });
  const ro = new ResizeObserver(() => { fitEvents(); if (view.follow) toBottom(false); }); ro.observe(box); offs.push(() => ro.disconnect());
  on(document, 'depthchange', () => { try { render(); } catch (_) { /* no data yet */ } });
  // the 3D scene takes its colors when it is built: build it again with a fresh canvas (a lost WebGL context cannot be reused)
  on(document, 'themechange', () => { if (!flow) return; flow.stop(); flow = null; const old = $('#flow'); if (old) { const c = old.cloneNode(false); old.replaceWith(c); } startFlow(); });
  startFlow();
  const tick = async () => { try { await refresh(); render(); } catch (e) { if (!view.s) $('#pane-tip').textContent = 'pool API not reachable: ' + e.message; } };
  await tick();
  timers.push(setInterval(tick, 2000));
}
function unmount() { if (flow) { flow.stop(); flow = null; } timers.splice(0).forEach(clearInterval); offs.splice(0).forEach((f) => f()); }
return { mount, unmount };
})();
