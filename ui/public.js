window.Views = window.Views || {};
Views.pool = (() => {
'use strict';
const timers = [], offs = [];
const on = (t, ev, fn) => { t.addEventListener(ev, fn); offs.push(() => t.removeEventListener(ev, fn)); };
const TEMPLATE = `<div class="statusline" id="statusline" role="status" aria-live="off"></div>
  <main class="grid pub">
    <section class="pane p-pool" aria-label="Pool"><h2><span>POOL</span></h2><pre id="pane-pool"></pre></section>
    <section class="pane p-connect" aria-label="Connect"><h2><span>CONNECT</span></h2><pre id="pane-connect"></pre></section>
    <section class="pane p-modes" aria-label="Payout modes"><h2><span>MODES</span></h2><pre id="pane-modes"></pre></section>
    <section class="pane p-history" aria-label="History"><h2><span>HISTORY</span><em class="rng"></em></h2><div class="chartpane"></div></section>
    <section class="pane p-devices" aria-label="Devices"><h2><span>DEVICES</span></h2><pre id="pane-devices"></pre></section>
    <section class="pane p-blocks" aria-label="Blocks"><h2><span>BLOCKS</span></h2><pre id="pane-blocks"></pre></section>
    <section class="pane p-payouts" aria-label="Payouts"><h2><span>PAYOUTS</span></h2><pre id="pane-payouts"></pre></section>
    <section class="pane p-tty" aria-label="Console"><h2><span>TTY</span><em>try <kbd>account &lt;your address&gt;</kbd> or <kbd>help</kbd></em></h2>
      <div class="tty" id="tty" tabindex="-1"></div>
      <form class="prompt" id="prompt" autocomplete="off">
        <label for="cmd"><span class="u">guest</span><span class="at">@</span><span class="h">photon</span><span class="sep">:~$</span></label>
        <input id="cmd" type="text" spellcheck="false" autocapitalize="off" autocomplete="off" autocorrect="off" aria-label="command line">
      </form>
    </section>
  </main>`;

const modeLabel = { solo: 'SOLO', pplns: 'PPLNS', pps: 'PPS' };
const modeShort = { solo: 'reward straight to you', pplns: 'share of each pool block, paid by the coinbase', pps: 'fixed pay per share' };
const modeText = {
  solo: 'direct payout in the block you find, no pool fee',
  pplns: 'share of the reward of every pool block by recent work',
  pps: 'fixed payment per share, paid by the pool',
};

const view = { src: null, pool: null, blocks: [], payouts: [], devices: [] };
let histCtl = null;

/* ---------- data ---------- */

function liveSource() {
  const get = async (p) => { const r = await fetch('/public/v1' + p, { cache: 'no-store' }); if (!r.ok) throw new Error(r.status); return r.json(); };
  return {
    demo: false,
    async refresh() { let d; [view.pool, view.blocks, view.payouts, d, view.engine] = await Promise.all([get('/pool'), get('/blocks?limit=12'), get('/payouts?limit=8'), get('/devices').catch(() => ({ devices: [] })), get('/engine').catch(() => null)]); view.devices = d.devices; },
    async account(addr) { const r = await fetch('/public/v1/account/' + encodeURIComponent(addr), { cache: 'no-store' }); if (!r.ok) throw new Error(r.status === 400 ? 'not a valid address for this network' : 'HTTP ' + r.status); return r.json(); },
  };
}

function demoSource() {
  const r = rng(77);
  const now = () => Date.now() / 1000;
  const hist = Array.from({ length: 60 }, () => 9.4e16 + r() * 4e15);
  const blocks = Array.from({ length: 10 }, (_, i) => ({ height: 132400 - i * 23, hash: hex(r, 64), time: now() - 1800 * (i + 1), mode: ['pplns', 'pplns', 'solo', 'pplns', 'pps'][i % 5], status: i < 2 ? 'maturing' : 'mature', finder: 'bc1q' + hex(r, 6) + '..' + hex(r, 4) }));
  const payouts = Array.from({ length: 5 }, (_, i) => ({ time: now() - 5400 * (i + 1), txid: hex(r, 64), amount: Math.round((3 + r() * 40) * LEPTON), status: 'broadcast' }));
  return {
    demo: true,
    async refresh() {
      view.devices = Stats.demoDevices();
      hist.push(9.4e16 + r() * 4e15); hist.shift();
      view.pool = {
        name: 'Photon Demo', version: '0.1.0-demo', network: 'mainnet', explorerUrl: '', uptime: 3 * 86400 + 4000,
        stratum: { host: 'pool.example.org', ports: [{ port: 3333, mode: 'pplns', override: true }, { port: 3334, mode: 'pps', override: false }, { port: 3335, mode: 'solo', override: false }] },
        modes: [{ mode: 'solo', fee: 0, available: true }, { mode: 'pplns', fee: 1, available: true }, { mode: 'pps', fee: 2, available: false }],
        minPayoutLeptons: 100000, payout: { dryRun: false, wallet: true, intervalSecs: 3600, nextRunAt: Date.now() / 1000 + 1500, lastRunAt: Date.now() / 1000 - 2100 }, pplnsWindowFactor: 2, hashrate: { now: hist[hist.length - 1], m1: 9.5e16, h1: 9.45e16, hist },
        miners: { online: 998, workers: 1000 }, found24h: 4, blocks: { found: 0, maturing: 2, mature: 40, orphaned: 1 },
        node: { height: 132457, tip: hex(r, 64), tipTime: now() - 23 }, networkDifficulty: 1.2e9,
      };
      view.blocks = blocks; view.payouts = payouts;
      view.engine = { modes: [{ mode: 'pplns', online: 700, hashrate: 7e16 + r() * 1e15 }, { mode: 'solo', online: 298, hashrate: 2.4e16 + r() * 1e15 }, { mode: 'pps', online: 0, hashrate: 0 }], node: { connected: true, rttMs: 0.4 + r() * 0.3, p50: 28 + r() * 10, p95: 55 + r() * 20 },
        net: { connections: 1000, inBps: 20000 + r() * 8000, outBps: 300000 + r() * 40000, jobsPerSec: 900 + r() * 100, connectsPerSec: r() < 0.2 ? 1 : 0, accepted: 1450, closed: 450, refused: 0, bytesIn: 3e8, bytesOut: 4e9 } };
    },
    async account(addr) {
      return { address: addr, hashrate: 1.2e14, workers: 3, shares1h: { pplns: { difficulty: 5200, count: 310 } }, shares24h: { pplns: { difficulty: 120000, count: 7400 } },
        balance: { pending: 12 * LEPTON, confirmed: 3.2 * LEPTON, paid: 40 * LEPTON }, payouts: [{ time: now() - 7000, txid: hex(r, 64), amount: 5 * LEPTON, status: 'broadcast' }] };
    },
  };
}

/* ---------- rendering ---------- */

function txUrl(base, txid) {
  return safeUrl(base) && /^[0-9a-f]{64}$/.test(txid || '') ? `${base.replace(/\/+$/, '')}/tx/${txid}` : '';
}

/* blocks and payouts of the pool: the newest page comes with every refresh, older pages are fetched when asked for */
const POOL_PAGE = { blocks: 12, payouts: 8 };
const poolPg = { blocks: { off: 0, items: null }, payouts: { off: 0, items: null } };
function poolPage(kind) {
  const g = poolPg[kind], size = POOL_PAGE[kind], first = view[kind];
  const total = (view.pool && view.pool[kind + 'Total']) ?? first.length;
  const items = g.items || first;
  return { items, bar: total > size ? ['', pagerBar(`data-pgp="${kind}"`, g.off, items.length, total, size)] : [] };
}
document.addEventListener('click', async (e) => {
  const b = e.target.closest && e.target.closest('.pgb[data-pgp]'); if (!b || view.src.demo) return;
  e.preventDefault();
  const kind = b.dataset.pgp, g = poolPg[kind], size = POOL_PAGE[kind], off = Math.max(0, g.off + (+b.dataset.d) * size);
  if (off === 0) { g.off = 0; g.items = null; onDepth(); return; }
  try {
    const r = await fetch('/public/v1/' + kind + '?offset=' + off + '&limit=' + size, { cache: 'no-store' });
    const items = r.ok ? await r.json() : [];
    if (items.length) { g.off = off; g.items = items; }
    onDepth();
  } catch (_) { /* keep the page that is shown */ }
});

const onDepth = () => { try { render(); } catch (_) { /* no data yet */ } };
function render() {
  const p = view.pool;
  if (!p) return;
  $('#pool-name').textContent = p.name;
  document.title = pageTitle('', p.name);
  const age = Date.now() / 1000 - p.node.tipTime;
  $('#statusline').innerHTML = [
    `<span>net <b>${esc(p.network)}</b></span>`,
    `<span>height <b>${num(p.node.height)}</b></span>`,
    `<span>pool <b>${fmtHash(p.hashrate.now)}</b></span>`,
    `<span>miners <b>${num(p.miners.online)}</b></span>`,
    `<span>blocks 24h <b>${num(p.found24h)}</b></span>`,
    `<span>tip age <b>${hms(age)}</b></span>`,
  ].join('');
  $('#pane-pool').innerHTML = [
    `${span('dim', 'hashrate  ')}${span('hl', fmtHash(p.hashrate.now))}  ${span('c2', spark(p.hashrate.hist, 24))}`,
    `${span('dim', 'avg 1h    ')}${fmtHash(p.hashrate.h1)}`,
    `${span('dim', 'miners    ')}${num(p.miners.online)} online, ${num(p.miners.workers)} workers`,
    `${span('dim', 'blocks    ')}${num(p.found24h)} in 24h, ${num(p.blocks.mature || 0)} mature`,
    `${span('dim', 'network   ')}difficulty ${p.networkDifficulty.toExponential(3)}`,
    `${span('dim', 'uptime    ')}${dhms(p.uptime)}`,
  ].join('\n');
  const host = p.stratum.host || location.hostname;
  $('#pane-connect').innerHTML = [
    span('dim', 'stratum url:'),
    ...p.stratum.ports.map((x) => `${span('c1', `stratum+tcp://${host}:${x.port}`)}  ${span('c2', modeLabel[x.mode] || x.mode)}${x.override ? span('c3', ' +m') : ''}`),
    '',
    `${span('dim', 'user      ')}${span('c3', '<your address>.<worker>')}`,
    `${span('dim', 'password  ')}x`,
    `${span('dim', 'miner     ')}enable version rolling`,
    '',
    `${span('dim', 'protocol  ')}Stratum V1, version rolling, extranonce2 4 bytes`,
  ].join('\n');
  const portsOf = (mode) => p.stratum.ports.filter((x) => x.mode === mode).map((x) => x.port + (x.override ? '*' : '')).join(',') || '-';
  $('#pane-modes').innerHTML = [span('row-h', pad('MODE', 7) + pad('FEE', 6) + pad('STATE', 8) + 'PORT')]
    .concat(p.modes.map((m) => `${span('c2', pad(modeLabel[m.mode] || m.mode, 7))}${pad(m.fee + '%', 6)}${m.available ? span('ok', pad('open', 8)) : span('warn', pad('paused', 8))}${portsOf(m.mode)}`))
    .concat(['',
      ...p.modes.map((m) => `${span('c2', pad((modeLabel[m.mode] || m.mode).toLowerCase(), 6))}${span('dim', modeShort[m.mode] || '')}`),
      '', span('dim', 'non-custodial: the coinbase of a found block'), span('dim', 'pays the miners directly, no balance, no payout run')]).join('\n');
  const fwk = colw(view.blocks.map((b) => b.finder), 6);
  trackSeries(p);
  const pgB = poolPage('blocks'), pgP = poolPage('payouts');
  const blocksS = sortRows('blocks', pgB.items, { height: (b) => b.height, time: (b) => b.time, mode: (b) => b.mode, finder: (b) => b.finder, status: (b) => b.status }, 'height');
  const payoutsS = sortRows('payouts', pgP.items, { time: (x) => x.time, amount: (x) => x.amount, txid: (x) => x.txid || '' }, 'time');
  $('#pane-blocks').innerHTML = [`<span class="row-h">${hdr('blocks', [['height', 'HEIGHT', 9], ['time', 'AGE', 8], ['mode', 'MODE', 7, false, 1], ['finder', 'FINDER', wide('blocks') ? fwk : 17, false, 1], ['status', 'STATE', 8, false, 1]], 'height')}</span>`]
    .concat(blocksS.map((b) => {
      const a = Date.now() / 1000 - b.time;
      const cls = b.status === 'mature' ? 'dim' : b.status === 'orphaned' ? 'err' : 'c3';
      return `${linkPad(b.url, num(b.height), 9)}${pad(a < 3600 ? Math.floor(a / 60) + 'm' : (a / 3600).toFixed(1) + 'h', 8)}${pad(modeLabel[b.mode] || b.mode, 7)}${fit(b.finder, wide('blocks') ? fwk : 17, wide('blocks'))}${span(cls, b.status)}`;
    })).concat(pgB.bar).join('\n');
  $('#pane-payouts').innerHTML = [`<span class="row-h">${hdr('payouts', [['time', 'AGE', 8], ['amount', 'AMOUNT', 15], ['txid', 'TX', 6, false, 1]], 'time')}</span>`]
    .concat(payoutsS.map((x) => {
      const a = Date.now() / 1000 - x.time;
      return `${pad(a < 3600 ? Math.floor(a / 60) + 'm' : (a / 3600).toFixed(1) + 'h', 8)}${pad(coins(x.amount), 15)}${link(txUrl(p.explorerUrl, x.txid), wide('payouts') ? (x.txid || '') : short(x.txid || '', 6, 4), 'c1')}`;
    })).concat(pgP.bar).join('\n') || span('dim', 'none: miners are paid by the coinbase of found blocks');
  Stats.S.devices = view.devices;
  $('#pane-devices').innerHTML = Stats.devicesHtml(view.devices || []);
  Extras.tick();
}


/* ---------- live series and the extra panels of maximized panes ---------- */

function trackSeries(p) {
  Series.seed('hr', p.hashrate.hist, 5);
  const en = view.engine;
  if (en && en.modes) en.modes.forEach((m) => { Series.push('mm_' + m.mode, m.online); Series.push('mh_' + m.mode, m.hashrate); });
  if (en) { Series.push('inbps', en.net.inBps); Series.push('outbps', en.net.outBps); Series.push('jobsps', en.net.jobsPerSec); Series.push('conns', en.net.connections); Series.push('cps', en.net.connectsPerSec); Series.push('nlat50', en.node.p50); Series.push('nlat95', en.node.p95); Series.push('nrtt', en.node.rttMs); }
  Series.push('hr', p.hashrate.now); Series.push('h1', p.hashrate.h1); Series.push('online', p.miners.online); Series.push('workers', p.miners.workers);
}
const card = Extras.card;
const timeLine = (host, series, fmt, extra = {}) => Neon.line(host, { series, fmt, xfmt: agoLabel, ...extra });

Extras.add('pool', (el) => {
  const k = card(el, 'pool numbers', 'wide'), row = Extras.row(el), hr = card(row, 'pool hashrate'), mi = card(row, 'miners and workers'), bl = card(row, 'blocks');
  return () => {
    const p = view.pool; if (!p) return;
    Neon.kpis(k, [{ label: 'hashrate', value: p.hashrate.now, fmt: fmtHash }, { label: '1 h avg', value: p.hashrate.h1, fmt: fmtHash, color: 'c2' }, { label: 'miners', value: p.miners.online, fmt: num }, { label: 'workers', value: p.miners.workers, fmt: num, color: 'c2' },
      { label: 'blocks 24 h', value: p.found24h, fmt: num, color: 'c3' }, { label: 'difficulty', value: p.networkDifficulty, fmt: (v) => v.toExponential(2), color: 'c3' }, { label: 'uptime', value: p.uptime, fmt: dhms, color: 'ok' }]);
    timeLine(hr, [{ name: 'hashrate', data: Series.get('hr'), color: 'c1' }, { name: '1 h avg', data: Series.get('h1'), color: 'c2' }], fmtHash);
    timeLine(mi, [{ name: 'miners', data: Series.get('online'), color: 'ok' }, { name: 'workers', data: Series.get('workers'), color: 'c2' }], num);
    const b = p.blocks || {};
    Neon.donut(bl, Object.entries({ maturing: b.maturing || 0, mature: b.mature || 0, orphaned: b.orphaned || 0, found: b.found || 0 }).filter(([, v]) => v > 0).map(([n, v]) => ({ label: n, value: v, color: n === 'orphaned' ? 'err' : n === 'maturing' ? 'c3' : n === 'found' ? 'ok' : 'c1', text: num(v) })), { big: num((b.maturing || 0) + (b.mature || 0) + (b.orphaned || 0) + (b.found || 0)), sub: 'blocks' });
  };
});

const fmtBytes = (v) => { v = Math.max(0, v); const u = ['B', 'KB', 'MB', 'GB', 'TB']; let i = 0; while (v >= 1000 && i < u.length - 1) { v /= 1000; i++; } return (i ? v.toFixed(v < 10 ? 2 : 1) : Math.round(v)) + ' ' + u[i]; };
const fmtMs = (v) => (v < 10 ? v.toFixed(1) : Math.round(v)) + ' ms';
// What the pool does on the network side: traffic, connections and the link to the node. Aggregates only, no address or worker.
Extras.add('connect', (el) => {
  const k = card(el, 'network', 'wide'), row = Extras.row(el), tr = card(row, 'traffic per second'), cn = card(row, 'connections'), lt = card(row, 'tip to job latency');
  return () => {
    const e = view.engine; if (!e) { k.innerHTML = '<span class="dim">no network data</span>'; return; }
    const n = e.net;
    Neon.kpis(k, [{ label: 'connections', value: n.connections, fmt: num }, { label: 'jobs / s', value: n.jobsPerSec, fmt: num, color: 'c2' }, { label: 'accepted / closed / refused', value: n.accepted, fmt: (v) => `${num(v)} / ${num(n.closed)} / ${num(n.refused)}`, color: 'c3' }, { label: 'in', value: n.inBps, fmt: (v) => fmtBytes(v) + '/s', color: 'c2' },
      { label: 'out', value: n.outBps, fmt: (v) => fmtBytes(v) + '/s', color: 'c3' }, { label: 'node link', value: e.node.connected ? 1 : 0, fmt: (v) => (v ? 'UP' : 'DOWN'), color: e.node.connected ? 'ok' : 'err', sub: 'rtt ' + fmtMs(e.node.rttMs) },
      { label: 'since start', value: n.bytesIn + n.bytesOut, fmt: fmtBytes, color: 'c3', sub: `${fmtBytes(n.bytesIn)} in, ${fmtBytes(n.bytesOut)} out` }]);
    timeLine(tr, [{ name: 'in', data: Series.get('inbps'), color: 'c2' }, { name: 'out', data: Series.get('outbps'), color: 'c3' }], (v) => fmtBytes(v) + '/s');
    timeLine(cn, [{ name: 'connected', data: Series.get('conns'), color: 'ok' }], num);
    timeLine(lt, [{ name: 'p50', data: Series.get('nlat50'), color: 'c1' }, { name: 'p95', data: Series.get('nlat95'), color: 'c3' }, { name: 'rpc rtt', data: Series.get('nrtt'), color: 'c2' }], fmtMs);
  };
});

const modeColor = { solo: 'c1', pplns: 'c2', pps: 'c3' };
// Payout modes: the rules as plain cards on top, below the live load per mode (connected miners and hash rate over time, share of the pool).
Extras.add('modes', (el) => {
  const info = Extras.row(el), row = Extras.row(el);
  let cards = null;
  const ct = card(row, 'miners per mode'), ch = card(row, 'hash rate per mode'), cs = card(row, 'hash rate share');
  return () => {
    const p = view.pool; if (!p) return;
    if (!cards) {
      info.style.gridTemplateColumns = `repeat(${Math.max(1, p.modes.length)}, 1fr)`;
      cards = Object.fromEntries(p.modes.map((m) => [m.mode, card(info, (modeLabel[m.mode] || m.mode) + ' mode')]));
    }
    const en = (view.engine && view.engine.modes) || [];
    const live = (id) => en.find((m) => m.mode === id) || { online: 0, hashrate: 0 };
    p.modes.forEach((m) => {
      const ports = p.stratum.ports.filter((x) => x.mode === m.mode).map((x) => x.port + (x.override ? ' (+m)' : '')).join(', ') || 'by password only: m=' + m.mode;
      cards[m.mode].innerHTML = [
        `<div class="mrow"><b class="mfee ${modeColor[m.mode]}">${esc(m.fee)}%</b><span class="dim">pool fee</span><span class="${m.available ? 'ok' : 'warn'}">${m.available ? 'open' : 'paused'}</span></div>`,
        `<div>${esc(modeShort[m.mode] || '')}</div>`,
        `<div class="dim">port ${esc(ports)}</div>`,
        `<div class="dim">${num(live(m.mode).online)} miners, ${fmtHash(live(m.mode).hashrate)}</div>`,
      ].join('');
    });
    timeLine(ct, p.modes.map((m) => ({ name: modeLabel[m.mode] || m.mode, data: Series.get('mm_' + m.mode), color: modeColor[m.mode] })), num);
    timeLine(ch, p.modes.map((m) => ({ name: modeLabel[m.mode] || m.mode, data: Series.get('mh_' + m.mode), color: modeColor[m.mode] })), fmtHash);
    const tot = p.modes.reduce((a, m) => a + live(m.mode).hashrate, 0);
    Neon.donut(cs, p.modes.map((m) => ({ key: m.mode, label: modeLabel[m.mode] || m.mode, value: live(m.mode).hashrate, color: modeColor[m.mode], text: tot ? pct(live(m.mode).hashrate / tot) : '0%' })), { big: fmtHash(tot), sub: 'all modes' });
  };
});

Extras.add('blocks', (el) => {
  const k = card(el, 'found blocks', 'wide'), row = Extras.row(el), gap = card(row, 'minutes between blocks'), md = card(row, 'blocks by mode'), st = card(row, 'status');
  return () => {
    const b = view.blocks; if (!b.length) { k.innerHTML = '<span class="dim">no block found yet</span>'; return; }
    const t = b.map((x) => x.time).sort((x, y) => x - y), gaps = t.slice(1).map((x, i) => (x - t[i]) / 60);
    Neon.kpis(k, [{ label: 'listed', value: b.length, fmt: num }, { label: 'last found', value: Date.now() / 1000 - t[t.length - 1], fmt: hms, color: 'c2' }, { label: 'avg gap', value: gaps.length ? gaps.reduce((x, y) => x + y, 0) / gaps.length : 0, fmt: (v) => v.toFixed(1) + ' min', color: 'c3' }]);
    if (gaps.length) Neon.columns(gap, gaps.map((g, i) => ({ key: t[i + 1], label: '', value: g, text: g.toFixed(1) + ' min', tip: agoLabel(t[i + 1]) })), { color: 'c3' });
    const by = (f) => { const o = {}; b.forEach((x) => { o[f(x)] = (o[f(x)] || 0) + 1; }); return Object.entries(o); };
    Neon.donut(md, by((x) => x.mode).map(([n, c]) => ({ label: modeLabel[n] || n, value: c, text: num(c) })), { big: num(b.length), sub: 'blocks' });
    Neon.donut(st, by((x) => x.status).map(([n, c]) => ({ label: n, value: c, color: n === 'orphaned' ? 'err' : n === 'mature' ? 'c1' : 'c3', text: num(c) })), { big: num(b.length), sub: 'blocks' });
  };
});

Extras.add('payouts', (el) => {
  const k = card(el, 'payouts', 'wide'), row = Extras.row(el), c = card(row, 'amount per payout (coins)'), cum = card(row, 'paid out in total (coins)');
  row.style.gridTemplateColumns = '2fr 1fr';
  return () => {
    const x = view.payouts; if (!x.length) { k.innerHTML = '<span class="dim">no payouts yet</span>'; return; }
    const tot = x.reduce((a, y) => a + y.amount, 0), sorted = [...x].sort((a, y) => a.time - y.time);
    Neon.kpis(k, [{ label: 'listed', value: x.length, fmt: num }, { label: 'total', value: tot, fmt: coins, color: 'c2' }, { label: 'average', value: tot / x.length, fmt: coins, color: 'c2' }, { label: 'largest', value: Math.max(...x.map((y) => y.amount)), fmt: coins, color: 'c3' }]);
    Neon.columns(c, sorted.map((y) => ({ key: y.txid || y.time, label: '', value: y.amount, text: coins(y.amount), tip: agoLabel(y.time) })), { color: 'c2' });
    let run = 0;
    if (sorted.length > 1) timeLine(cum, [{ name: 'total', data: sorted.map((y) => ({ x: y.time, y: (run += y.amount) })), color: 'c3' }], (v) => (v / LEPTON).toFixed(2));
    else cum.innerHTML = '<span class="dim">needs at least two payouts</span>';
  };
});

/* ---------- console ---------- */

let tty;
function out(html) {
  const d = document.createElement('div');
  d.innerHTML = html;
  tty.appendChild(d);
  while (tty.childElementCount > 300) tty.firstChild.remove();
  tty.scrollTop = tty.scrollHeight;
}

async function accountCmd(addr) {
  if (!addr) return out(span('warn', 'usage: account <your payout address>'));
  try {
    const a = await view.src.account(addr);
    const sh = (m) => Object.entries(m).map(([k, v]) => `${modeLabel[k] || k} ${num(v.count)} shares`).join(', ') || 'none';
    out(`${span('c1', maskAddr(a.address))}  ${span('hl', fmtHash(a.hashrate))}  ${num(a.workers)} workers online`);
    out(`${span('dim', 'last hour  ')}${esc(sh(a.shares1h))}`);
    out(`${span('dim', 'last 24h   ')}${esc(sh(a.shares24h))}`);
    out(`${span('dim', 'pending    ')}${coins(a.balance.pending)}   ${span('dim', 'payable')} ${coins(a.balance.confirmed)}   ${span('dim', 'paid')} ${coins(a.balance.paid)}`);
    for (const x of a.payouts.slice(0, 5)) {
      out(`${span('dim', new Date(x.time * 1000).toISOString().slice(0, 16).replace('T', ' '))}  ${coins(x.amount)}  ${link(txUrl(view.pool.explorerUrl, x.txid), short(x.txid || 'pending', 8, 6), 'c1')}`);
    }
  } catch (e) {
    out(span('err', 'account lookup failed: ' + e.message));
  }
}

const cmds = {
  help() {
    out(span('c1', 'commands'));
    [['pool', 'pool summary'], ['connect', 'how to point your miner at this pool'], ['modes', 'payout modes, fees, availability'], ['blocks', 'recent blocks'], ['payouts', 'recent payouts'],
      ['account <address>', 'your hashrate, shares, balance and payouts'], ['miner <address>', 'open your personal miner page'], ['theme [name]', 'list or set the colour theme (default cyan)'], ['clear', 'clear console']]
      .forEach(([c, d]) => out(`  ${span('c3', pad(c, 34))}${span('dim', d)}`));
  },
  pool() { const p = view.pool; out(`${fmtHash(p.hashrate.now)}, ${num(p.miners.online)} miners, ${num(p.found24h)} blocks in 24h, height ${num(p.node.height)}`); },
  connect() {
    const p = view.pool; const host = p.stratum.host || location.hostname;
    p.stratum.ports.forEach((x) => out(`${span('c1', `stratum+tcp://${host}:${x.port}`)}  ${span('c2', modeLabel[x.mode] || x.mode)}`));
    out(span('dim', 'user: <your address>.<worker>   password: x   (version rolling on)'));
  },
  modes() { view.pool.modes.forEach((m) => out(`${span('c2', pad(modeLabel[m.mode] || m.mode, 6))} fee ${m.fee}%  ${m.available ? span('ok', 'open') : span('warn', 'paused')}  ${span('dim', modeText[m.mode] || '')}`)); },
  miner(a) { if (!a[0]) return out(span('warn', 'usage: miner <your address>')); SPA.go('/miner/' + encodeURIComponent(a[0])); },
  blocks() { view.blocks.slice(0, 8).forEach((b) => out(`${linkPad(b.url, num(b.height), 9)}${pad(modeLabel[b.mode] || b.mode, 7)}${b.finder}  ${b.status}`)); },
  payouts() { view.payouts.forEach((x) => out(`${new Date(x.time * 1000).toISOString().slice(0, 16).replace('T', ' ')}  ${coins(x.amount)}  ${link(txUrl(view.pool.explorerUrl, x.txid), short(x.txid || '', 8, 6), 'c1')}`)); if (!view.payouts.length) out(span('dim', 'no payouts yet')); },
  account(a) { return accountCmd(a[0]); },
  theme(a) {
    const t = a[0];
    if (!t) return Themes.listHtml().split('\n').forEach((l) => out(l));
    if (!Themes.set(t)) return out(span('warn', 'usage: theme ' + Themes.list().join('|')));
  },
  clear() { tty.innerHTML = ''; },
};

let input;
const hist = []; let hi = 0;
function bindConsole() {
$('#prompt').addEventListener('submit', async (e) => {
  e.preventDefault();
  const raw = input.value.trim();
  input.value = '';
  if (!raw) return;
  hist.push(raw); hi = hist.length;
  out(`${span('u', 'guest')}${span('at', '@')}${span('h', 'photon')}${span('sep', ':~$')} ${esc(raw)}`);
  const [c, ...a] = raw.split(/\s+/);
  if (cmds[c]) { try { await cmds[c](a); } catch (err) { out(span('err', 'error: ' + err.message)); } } else out(span('warn', `unknown command: ${c}. try help`));
});
input.addEventListener('keydown', (e) => {
  if (e.key === 'ArrowUp') { if (hi > 0) input.value = hist[--hi]; e.preventDefault(); }
  else if (e.key === 'ArrowDown') { input.value = hi < hist.length - 1 ? hist[++hi] : ((hi = hist.length), ''); e.preventDefault(); }
  else if (e.key === 'Tab') { e.preventDefault(); const m = Object.keys(cmds).filter((c) => c.startsWith(input.value)); if (m.length === 1) input.value = m[0] + ' '; }
  else if (e.key === 'l' && e.ctrlKey) { e.preventDefault(); cmds.clear(); }
});
on(document, 'keydown', (e) => { if (!e.ctrlKey && !e.metaKey && !e.altKey && e.key.length === 1 && document.activeElement !== input) input.focus(); });
}

/* ---------- boot ---------- */

async function boot0() {
  let src = null;
  if (!new URLSearchParams(location.search).has('demo') && location.protocol !== 'file:') {
    try { const l = liveSource(); await l.refresh(); src = l; } catch (_) { /* fall back to demo */ }
  }
  view.src = src || demoSource();
  if (!src) await view.src.refresh();
  $('#who').textContent = 'photon';
  histCtl = Stats.historyPane($('.p-history'), '/public/v1', view.src.demo);
  const tag = $('#mode-tag');
  tag.textContent = view.src.demo ? 'DEMO DATA' : 'LIVE';
  tag.className = 'tag ' + (view.src.demo ? 'demo' : 'live');
  out(span('c1', 'welcome to the pool terminal. type help, or look up your address with: account <address>'));
  render();
  timers.push(setInterval(async () => { try { await view.src.refresh(); render(); } catch (_) { /* keep the last data */ } }, 5000));
  const hash = decodeURIComponent(location.hash.replace(/^#/, ''));
  if (hash) { out(`${span('u', 'guest')}${span('at', '@')}${span('h', 'photon')}${span('sep', ':~$')} account ${esc(hash)}`); accountCmd(hash); }
  if (matchMedia('(pointer: fine)').matches) input.focus();
}

async function mount(root) {
  root.innerHTML = TEMPLATE;
  tty = $('#tty'); input = $('#cmd');
  bindConsole();
  on(document, 'depthchange', onDepth);
  boot0().catch((e) => out(span('err', 'could not start: ' + e.message)));
}
function unmount() {
  if (histCtl) { histCtl.destroy(); histCtl = null; }
  timers.splice(0).forEach(clearInterval);
  offs.splice(0).forEach((f) => f());
}
return { mount, unmount };
})();
