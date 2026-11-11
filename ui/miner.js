window.Views = window.Views || {};
Views.miner = (() => {
'use strict';
const timers = [], offs = [];
const on = (t, ev, fn) => { t.addEventListener(ev, fn); offs.push(() => t.removeEventListener(ev, fn)); };
const TEMPLATE = `<div class="statusline" id="statusline" role="status" aria-live="off"></div>
  <main class="grid miner">
    <section class="pane p-acct" aria-label="Account"><h2><span>ACCOUNT</span></h2><pre id="pane-acct"></pre></section>
    <section class="pane p-bal" aria-label="Balance"><h2><span>BALANCE</span></h2><pre id="pane-bal"></pre></section>
    <section class="pane p-hr" aria-label="Hashrate"><h2><span>HASHRATE 24H</span></h2><pre id="pane-hr"></pre></section>
    <section class="pane p-workers" aria-label="Workers"><h2><span>WORKERS</span></h2><pre id="pane-workers"></pre></section>
    <section class="pane p-pend" aria-label="Pending credits"><h2><span>PENDING</span></h2><pre id="pane-pend"></pre></section>
    <section class="pane p-pay" aria-label="Payouts"><h2><span>PAYOUTS</span></h2><pre id="pane-pay"></pre></section>
    <section class="pane p-found" aria-label="Blocks found"><h2><span>BLOCKS FOUND</span></h2><pre id="pane-found"></pre></section>
    <section class="pane p-tty" aria-label="Console"><h2><span>TTY</span><em>paste your address or <kbd>help</kbd></em></h2>
      <div class="tty" id="tty" tabindex="-1"></div>
      <form class="prompt" id="prompt" autocomplete="off">
        <label for="cmd"><span class="u">miner</span><span class="at">@</span><span class="h">photon</span><span class="sep">:~$</span></label>
        <input id="cmd" type="text" spellcheck="false" autocapitalize="off" autocomplete="off" autocorrect="off" aria-label="command line">
      </form>
    </section>
  </main>`;

const modeLabel = { solo: 'SOLO', pplns: 'PPLNS', pps: 'PPS' };
const freshPg = () => ({ found: { off: 0, items: null }, pay: { off: 0, items: null }, pend: { off: 0, items: null } });
const M = { addr: '', acct: null, pool: null, demo: false, timer: null, pg: freshPg() };
const PAGE = 20;

/* ---------- data ---------- */

async function getJson(path) {
  const r = await fetch(path, { cache: 'no-store' });
  if (!r.ok) throw new Error(r.status === 400 ? 'not a valid address for this network' : r.status === 429 ? 'too many requests, try again in a moment' : 'HTTP ' + r.status);
  return r.json();
}

function demoData(addr) {
  const r = rng(5);
  const now = Date.now() / 1000;
  const history = Array.from({ length: 144 }, (_, i) => ({ time: now - (143 - i) * 600, hashrate: 1.1e14 + Math.sin(i / 9) * 2e13 + r() * 1e13 }));
  return {
    pool: { name: 'Photon Demo', explorerUrl: '', payout: { dryRun: false, wallet: true, intervalSecs: 3600, nextRunAt: Date.now() / 1000 + 1500, lastRunAt: 0 }, minPayoutLeptons: 100000, node: { height: 132457 }, modes: [{ mode: 'pplns', fee: 1, available: true }] },
    acct: {
      address: addr, hashrate: 1.2e14, workers: 3, shares1h: { pplns: { difficulty: 5200, count: 310 } }, shares24h: { pplns: { difficulty: 120000, count: 7400 } },
      balance: { pending: 12.5 * LEPTON, confirmed: 0.0008 * LEPTON, paid: 40 * LEPTON }, minPayoutLeptons: 100000,
      workerList: [{ name: 'rig1', mode: 'pplns', hashrate: 5.1e13, difficulty: 8192, accepted: 2210, stale: 12, invalid: 1, seen: 3 }, { name: 'rig2', mode: 'pplns', hashrate: 4.4e13, difficulty: 8192, accepted: 1930, stale: 9, invalid: 0, seen: 8 }, { name: 'rig3', mode: 'solo', hashrate: 2.5e13, difficulty: 4096, accepted: 880, stale: 2, invalid: 0, seen: 1 }],
      history,
      credits: [{ height: 132440, hash: hex(r, 64), amount: 3.1 * LEPTON, status: 'pending', blockStatus: 'maturing', maturesIn: 84 }, { height: 132390, hash: hex(r, 64), amount: 2.2 * LEPTON, status: 'pending', blockStatus: 'maturing', maturesIn: 34 }, { height: 132100, hash: hex(r, 64), amount: 1.9 * LEPTON, status: 'confirmed', blockStatus: 'mature', maturesIn: 0 }],
      payouts: [{ time: now - 7000, txid: hex(r, 64), amount: 5 * LEPTON, status: 'broadcast' }, { time: now - 90000, txid: hex(r, 64), amount: 8.2 * LEPTON, status: 'broadcast' }],
      blocksFound: [{ height: 132200, hash: hex(r, 64), time: now - 40000, mode: 'solo', status: 'mature' }],
    },
  };
}

/* blocks found and payouts: the first page comes with the account, further pages are fetched on demand */
const PATHS = { found: 'blocks', pay: 'payouts', pend: 'credits' };
const pageOf = (kind) => {
  const g = M.pg[kind], a = M.acct;
  const first = kind === 'found' ? a.blocksFound : kind === 'pay' ? a.payouts : a.credits;
  const total = (kind === 'found' ? a.blocksTotal : kind === 'pay' ? a.payoutsTotal : a.creditsTotal) ?? first.length;
  return { items: g.items || first, off: g.off, total };
};
function pager(kind) {
  const { items, off, total } = pageOf(kind);
  return total <= PAGE ? [] : ['', pagerBar(`data-pg="${kind}"`, off, items.length, total, PAGE)];
}
document.addEventListener('click', async (e) => {
  const b = e.target.closest && e.target.closest('.pgb[data-pg]'); if (!b || !M.acct) return;
  e.preventDefault();
  const kind = b.dataset.pg, g = M.pg[kind], off = Math.max(0, g.off + (+b.dataset.d) * PAGE);
  if (off === 0) { g.off = 0; g.items = null; render(); return; }
  try {
    const r = await getJson('/public/v1/account/' + encodeURIComponent(M.addr) + '/' + PATHS[kind] + '?offset=' + off + '&limit=' + PAGE);
    if (r.items.length) { g.off = off; g.items = r.items; }
    render();
  } catch (_) { /* keep the page that is shown */ }
});

async function load() {
  if (!M.addr) return;
  if (M.demo) { const d = demoData(M.addr); M.acct = d.acct; M.pool = d.pool; return; }
  const [acct, pool] = await Promise.all([getJson('/public/v1/account/' + encodeURIComponent(M.addr)), M.pool ? Promise.resolve(M.pool) : getJson('/public/v1/pool')]);
  if (M.acct && M.acct.address !== acct.address) M.pg = freshPg();
  M.acct = acct;
  M.pool = pool;
}

/* ---------- rendering ---------- */

const txUrl = (base, txid) => (safeUrl(base) && /^[0-9a-f]{64}$/.test(txid || '') ? `${base.replace(/\/+$/, '')}/tx/${txid}` : '');

function compress(values, n) {
  if (values.length <= n) return values;
  const size = values.length / n;
  return Array.from({ length: n }, (_, i) => { const part = values.slice(Math.floor(i * size), Math.floor((i + 1) * size)); return part.reduce((a, b) => a + b, 0) / Math.max(1, part.length); });
}

const onDepth = () => { try { render(); } catch (_) { /* no data yet */ } };
function render() {
  const p = M.pool, a = M.acct;
  if (p) { $('#pool-name').textContent = p.name; document.title = pageTitle('Miner', p.name); }
  if (!a) {
    $('#statusline').innerHTML = `<span>no address selected</span>`;
    for (const id of ['acct', 'bal', 'hr', 'workers', 'pend', 'pay', 'found']) $('#pane-' + id).innerHTML = '';
    $('#pane-acct').innerHTML = span('dim', 'paste your payout address into the console below\nor open /miner/<address>');
    return;
  }
  const explorer = p ? p.explorerUrl : '';
  const min = a.minPayoutLeptons || 100000;
  const avg24 = a.history.length ? a.history.reduce((s, x) => s + x.hashrate, 0) / a.history.length : 0;
  const online = a.workerList.filter((w) => w.seen < 120).length;
  const lastSeen = a.workerList.length ? Math.min(...a.workerList.map((w) => w.seen)) : null;
  $('#statusline').innerHTML = [
    `<span>miner <b>${esc(maskAddr(a.address))}</b></span>`,
    `<span>hashrate <b>${fmtHash(a.hashrate)}</b></span>`,
    `<span>workers <b>${online}</b></span>`,
    `<span class="${a.balance.paid > 0 ? 'ok' : ''}">paid by coinbase <b>${coins(a.balance.paid)}</b></span>`,
  ].join('');
  $('#pane-acct').innerHTML = [
    `${span('dim', 'address   ')}${span('c1', maskAddr(a.address))}`,
    `${span('dim', 'status    ')}${online ? span('ok', 'mining') : span('warn', 'no worker online')}`,
    `${span('dim', 'hashrate  ')}${span('hl', fmtHash(a.hashrate))}`,
    `${span('dim', 'avg 24h   ')}${fmtHash(avg24)}`,
    `${span('dim', 'workers   ')}${online} online of ${a.workerList.length}`,
    `${span('dim', 'last share')} ${lastSeen === null ? 'never' : lastSeen + 's ago'}`,
  ].join('\n');
  $('#pane-bal').innerHTML = [
    `${span('dim', 'maturing  ')}${coins(a.balance.pending)}  ${span('dim', 'in blocks that are not mature yet')}`,
    `${span('dim', 'received  ')}${span('hl', coins(a.balance.paid))}  ${span('dim', 'paid by the coinbase of mature blocks')}`,
    '',
    span('dim', 'non-custodial: the pool never holds your coins,'),
    span('dim', 'every block it finds pays you directly in its coinbase'),
  ].join('\n');
  const rates = compress(a.history.map((x) => x.hashrate), 56);
  const sh = (m) => Object.entries(m).map(([k, v]) => `${modeLabel[k] || k} ${num(v.count)}`).join('  ') || 'none';
  $('#pane-hr').innerHTML = [
    rates.length ? span('c2', spark(rates, 56)) : span('dim', 'no shares in the last 24 hours'),
    `${span('dim', 'shares 1h   ')}${esc(sh(a.shares1h))}`,
    `${span('dim', 'shares 24h  ')}${esc(sh(a.shares24h))}`,
  ].join('\n');
  const wwk = colw(a.workerList.map((w) => w.name), 6);
  const pw = pageRows('workers', sortRows('workers', a.workerList, { name: (w) => w.name, mode: (w) => w.mode, hashrate: (w) => w.hashrate, difficulty: (w) => w.difficulty, accepted: (w) => w.accepted, stale: (w) => w.stale, invalid: (w) => w.invalid, seen: (w) => w.seen }, 'hashrate'), 20);
  $('#pane-workers').innerHTML = [`<span class="row-h">${hdr('workers', [['name', 'WORKER', wide('workers') ? wwk : 16, false, 1], ['mode', 'MODE', 7, false, 1], ['hashrate', 'HASHRATE', 12, true], ['difficulty', 'DIFF', 9, true], ['accepted', 'OK', 8, true], ['stale', 'STALE', 7, true], ['invalid', 'BAD', 5, true]], 'hashrate')}  ${hdr('workers', [['seen', 'LAST', 6, false, 1]], 'hashrate')}</span>`]
    .concat(pw.rows.map((w) => `${wide('workers') ? pad(w.name, wwk) : pad(trunc(w.name, 15), 16)}${span('c2', pad(modeLabel[w.mode] || w.mode, 7))}${lpad(fmtHash(w.hashrate), 12)}${lpad(num(w.difficulty), 9)}${lpad(num(w.accepted), 8)}${span(w.stale > 0 ? 'warn' : 'dim', lpad(num(w.stale), 7))}${lpad(num(w.invalid), 5)}  ${w.seen < 120 ? span('ok', w.seen + 's') : span('dim', ago(Date.now() / 1000 - w.seen))}`))
    .concat(a.workerList.length ? pw.bar : [span('dim', 'no worker is connected right now')]).join('\n');
  $('#pane-pend').innerHTML = [span('row-h', pad('BLOCK', 9) + pad('AMOUNT', 14) + 'STATE')]
    .concat(pageOf('pend').items.map((c) => {
      const st = c.status === 'pending' ? span('c3', `matures in ${c.maturesIn}`) : c.status === 'confirmed' ? span('ok', 'received') : span('err', c.status);
      return `${linkPad(c.url, num(c.height), 9)}${pad(coins(c.amount), 14)}${st}`;
    }))
    .concat(a.credits.length ? pager('pend') : [span('dim', 'no coinbase payouts yet')]).join('\n');
  $('#pane-pay').innerHTML = [`<span class="row-h">${hdr('pay', [['time', 'AGE', 7], ['amount', 'AMOUNT', 14], ['txid', 'TX', 4, false, 1]], 'time')}</span>`]
    .concat(sortRows('pay', pageOf('pay').items, { time: (x) => x.time, amount: (x) => x.amount, txid: (x) => x.txid || '' }, 'time').map((x) => `${pad(ago(x.time), 7)}${pad(coins(x.amount), 14)}${link(txUrl(explorer, x.txid), wide('pay') ? (x.txid || 'pending') : short(x.txid || 'pending', 6, 4), 'c1')}`))
    .concat(a.payouts.length ? pager('pay') : [span('dim', 'no separate payouts: the coinbase pays you directly')]).join('\n');
  $('#pane-found').innerHTML = [span('row-h', pad('BLOCK', 9) + pad('AGE', 7) + pad('MODE', 7) + 'STATE')]
    .concat(pageOf('found').items.map((b) => `${linkPad(b.url, num(b.height), 9)}${pad(ago(b.time), 7)}${span('c2', pad(modeLabel[b.mode] || b.mode, 7))}${b.status}`))
    .concat(a.blocksFound.length ? pager('found') : [span('dim', 'no block found by this address')]).join('\n');
  Extras.tick();
}


/* ---------- extra panels of maximized panes ---------- */

const card = Extras.card;
const wlabel = (w) => trunc(w.name, 24);

Extras.add('hr', (el) => {
  const k = card(el, 'your hashrate', 'wide'), c = card(el, 'hashrate, last 24 hours', 'wide'), sh = card(el, 'shares in 24 h by mode'), sh1 = card(el, 'shares in 1 h by mode');
  return () => {
    const a = M.acct; if (!a) return;
    const h = a.history.map((x) => ({ x: x.time, y: x.hashrate })), ys = h.map((p) => p.y);
    Neon.kpis(k, [{ label: 'now', value: a.hashrate, fmt: fmtHash }, { label: '24 h avg', value: ys.length ? ys.reduce((x, y) => x + y, 0) / ys.length : 0, fmt: fmtHash, color: 'c2' }, { label: '24 h peak', value: Math.max(0, ...ys), fmt: fmtHash, color: 'c3' }, { label: 'workers', value: a.workers, fmt: num, color: 'ok' }]);
    Neon.line(c, { series: [{ name: 'hashrate', data: h, color: 'c1' }], fmt: fmtHash, xfmt: agoLabel });
    for (const [host, m] of [[sh, a.shares24h], [sh1, a.shares1h]]) {
      const parts = Object.entries(m).map(([n, v]) => ({ label: modeLabel[n] || n, value: v.count, text: num(v.count) }));
      Neon.donut(host, parts.length ? parts : [{ label: 'none', value: 1, color: 'c1', text: '0' }], { big: num(parts.reduce((x, p) => x + p.value, 0)), sub: 'shares' });
    }
  };
});

Extras.add('workers', (el) => {
  const h = card(el, 'hashrate per worker'), s = card(el, 'stale ratio per worker'), m = card(el, 'workers by mode');
  return () => {
    const a = M.acct; if (!a || !a.workerList.length) return;
    Neon.bars(h, [...a.workerList].sort((x, y) => y.hashrate - x.hashrate).slice(0, 12).map((w) => ({ key: w.name, label: wlabel(w), value: w.hashrate, text: fmtHash(w.hashrate) })));
    Neon.bars(s, a.workerList.filter((w) => w.accepted + w.stale > 0).map((w) => ({ key: w.name, label: wlabel(w), value: w.stale / (w.accepted + w.stale), text: pct(w.stale / (w.accepted + w.stale)) + `  ${num(w.stale)} of ${num(w.accepted + w.stale)}`, color: 'warn' })).slice(0, 12), { max: 0.05 });
    const c = {}; a.workerList.forEach((w) => { c[w.mode] = (c[w.mode] || 0) + 1; });
    Neon.donut(m, Object.entries(c).map(([n, v]) => ({ label: modeLabel[n] || n, value: v, text: num(v) })), { big: num(a.workerList.length), sub: 'workers' });
  };
});

Extras.add('pay', (el) => {
  const k = card(el, 'payouts', 'wide'), c = card(el, 'amount per payout (coins), latest ' + PAGE, 'wide');
  return () => {
    const a = M.acct; if (!a) return;
    if (!a.payouts.length) { k.innerHTML = '<span class="dim">nothing paid out yet</span>'; return; }
    const tot = a.payoutsSum ?? a.payouts.reduce((x, y) => x + y.amount, 0), cnt = a.payoutsTotal ?? a.payouts.length;
    Neon.kpis(k, [{ label: 'payouts', value: cnt, fmt: num }, { label: 'total', value: tot, fmt: coins, color: 'c2' }, { label: 'average', value: tot / cnt, fmt: coins, color: 'c2' }]);
    Neon.columns(c, [...a.payouts].sort((x, y) => x.time - y.time).map((y) => ({ key: y.txid || y.time, label: '', value: y.amount, text: coins(y.amount), tip: agoLabel(y.time) })), { color: 'c2' });
  };
});

Extras.add('found', (el) => {
  const k = card(el, 'blocks found by this address', 'wide'), md = card(el, 'by mode'), st = card(el, 'by status');
  return () => {
    const a = M.acct; if (!a || !a.blocksFound.length) { k.innerHTML = '<span class="dim">no block found by this address</span>'; return; }
    const total = a.blocksTotal ?? a.blocksFound.length;
    const count = (f) => { const o = {}; a.blocksFound.forEach((x) => { o[f(x)] = (o[f(x)] || 0) + 1; }); return o; };
    const byMode = Object.entries(a.blocksByMode || count((x) => x.mode)), byStatus = Object.entries(a.blocksByStatus || count((x) => x.status));
    Neon.kpis(k, [{ label: 'blocks', value: total, fmt: num }]);
    Neon.donut(md, byMode.map(([n, v]) => ({ label: modeLabel[n] || n, value: v, text: num(v) })), { big: num(total), sub: 'blocks' });
    Neon.donut(st, byStatus.map(([n, v]) => ({ label: n, value: v, text: num(v) })), { big: num(total), sub: 'blocks' });
  };
});

/* ---------- console ---------- */

let tty;
function out(html) {
  const d = document.createElement('div');
  d.innerHTML = html;
  tty.appendChild(d);
  while (tty.childElementCount > 200) tty.firstChild.remove();
  tty.scrollTop = tty.scrollHeight;
}

async function setAddress(addr) {
  M.addr = addr.trim();
  M.acct = null; M.pg = freshPg();
  try { localStorage.setItem('photon-miner-address', M.addr); } catch (_) { /* storage unavailable */ }
  history.replaceState(null, '', '/miner/' + encodeURIComponent(M.addr));
  try { await load(); out(span('ok', 'loaded ' + maskAddr(M.addr))); } catch (e) { out(span('err', e.message)); }
  render();
}

const cmds = {
  help() {
    out(span('c1', 'commands'));
    [['go <address>', 'show another address (or just paste it)'], ['refresh', 'reload now'], ['theme [name]', 'list or set the colour theme (default cyan)'], ['clear', 'clear console']]
      .forEach(([c, d]) => out(`  ${span('c3', pad(c, 34))}${span('dim', d)}`));
  },
  go(a) { return a[0] ? setAddress(a[0]) : out(span('warn', 'usage: go <address>')); },
  async refresh() { try { await load(); render(); out(span('dim', 'refreshed')); } catch (e) { out(span('err', e.message)); } },
  theme(a) {
    const t = a[0];
    if (!t) return Themes.listHtml().split('\n').forEach((l) => out(l));
    if (!Themes.set(t)) return out(span('warn', 'usage: theme ' + Themes.list().join('|')));
  },
  clear() { tty.innerHTML = ''; },
};

let input;
function bindConsole() {
$('#prompt').addEventListener('submit', async (e) => {
  e.preventDefault();
  const raw = input.value.trim();
  input.value = '';
  if (!raw) return;
  out(`${span('u', 'miner')}${span('at', '@')}${span('h', 'photon')}${span('sep', ':~$')} ${esc(raw)}`);
  const [c, ...a] = raw.split(/\s+/);
  if (cmds[c]) { try { await cmds[c](a); } catch (err) { out(span('err', 'error: ' + err.message)); } }
  else if (/^[a-z]{1,8}1[0-9a-z]{20,}$/i.test(raw)) await setAddress(raw);
  else out(span('warn', `unknown command: ${c}. try help, or paste your address`));
});
on(document, 'keydown', (e) => { if (!e.ctrlKey && !e.metaKey && !e.altKey && e.key.length === 1 && document.activeElement !== input) input.focus(); });
}

/* ---------- boot ---------- */

async function boot0() {
  const params = new URLSearchParams(location.search);
  M.demo = params.has('demo') || location.protocol === 'file:';
  $('#who').textContent = 'miner';
  const tag = $('#mode-tag');
  tag.textContent = M.demo ? 'DEMO DATA' : 'LIVE';
  tag.className = 'tag ' + (M.demo ? 'demo' : 'live');
  const pm = location.pathname.match(/^\/miner\/([^/]+)/);
  let addr = pm ? decodeURIComponent(pm[1]) : decodeURIComponent(location.hash.replace(/^#/, ''));
  if (!addr) { try { addr = localStorage.getItem('photon-miner-address') || ''; } catch (_) { addr = ''; } }
  if (M.demo && !addr) addr = 'bc1qdemodemodemodemodemodemodemodemodemo';
  out(span('c1', 'miner overview. paste your payout address to see balances, pending credits and payouts.'));
  if (addr) await setAddress(addr); else render();
  timers.push(setInterval(async () => { if (!M.addr) return; try { await load(); render(); } catch (_) { /* keep the last data */ } }, 10000));
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
  timers.splice(0).forEach(clearInterval);
  offs.splice(0).forEach((f) => f());
}
return { mount, unmount };
})();
