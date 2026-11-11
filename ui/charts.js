'use strict';
// Neon charts: small dependency-free SVG/HTML renderers with glow, draw-in animation and hover read-out.
// Neon.line / bars / columns / donut / kpis keep their DOM between calls, so a periodic update animates
// instead of redrawing. Series keeps a time series per name for the live charts; Extras shows additional
// panels (charts, statistics) in a pane while it is maximized.
const Neon = (() => {
  const NS = 'http://www.w3.org/2000/svg';
  // style goes through the CSSOM: the public pages forbid inline style attributes (Content Security Policy)
  const svg = (n, a, p) => { const e = document.createElementNS(NS, n); for (const k in a) { if (k === 'style') e.style.cssText = a[k]; else e.setAttribute(k, a[k]); } if (p) p.append(e); return e; };
  const html = (cls, p, tag = 'div') => { const e = document.createElement(tag); e.className = cls; if (p) p.append(e); return e; };
  const PAL = ['c1', 'c2', 'c3', 'ok', 'warn', 'err'];
  const col = (c) => `var(--${c})`;
  const nextFrame = (f) => requestAnimationFrame(() => requestAnimationFrame(f));

  function nice(lo, hi, n = 4) {
    const span = hi - lo || Math.abs(hi) || 1, raw = span / n, mag = 10 ** Math.floor(Math.log10(raw)), f = raw / mag;
    const step = (f > 5 ? 10 : f > 2 ? 5 : f > 1 ? 2 : 1) * mag, out = [];
    for (let v = Math.floor(lo / step) * step; v <= Math.ceil(hi / step) * step + step / 2; v += step) out.push(+v.toFixed(12));
    return out;
  }
  const norm = (d) => d.map((p, i) => (typeof p === 'number' ? { x: i, y: p } : p));
  function thin(pts, n) {
    if (pts.length <= n) return pts;
    const size = pts.length / n, out = [];
    for (let i = 0; i < n; i++) {
      const part = pts.slice(Math.floor(i * size), Math.max(Math.floor(i * size) + 1, Math.floor((i + 1) * size)));
      out.push({ x: part[part.length - 1].x, y: part.reduce((a, p) => a + p.y, 0) / part.length });
    }
    return out;
  }

  /* ---- line / area chart with crosshair ---- */
  function lineChart(host) {
    host.textContent = '';
    const wrap = html('nchart', host), tip = html('ntip', wrap);
    const s = svg('svg', { class: 'nsvg' }, wrap);
    const defs = svg('defs', {}, s), grid = svg('g', {}, s), areas = svg('g', {}, s), lines = svg('g', {}, s), dots = svg('g', {}, s);
    const cross = svg('line', { class: 'ncross' }, s), hdots = svg('g', {}, s), over = svg('rect', { fill: 'transparent' }, s);
    const st = { o: null, els: [], pts: [], W: 600, H: 180, m: { l: 58, r: 14, t: 10, b: 22 }, sx: null, sy: null, uid: Math.random().toString(36).slice(2, 7) };

    function ensure(n) {
      while (st.els.length < n) {
        const i = st.els.length, c = col(st.o.series[i].color || PAL[i % PAL.length]);
        const g = svg('linearGradient', { id: `g${st.uid}${i}`, x1: 0, y1: 0, x2: 0, y2: 1 }, defs);
        svg('stop', { offset: '0%', style: `stop-color:${c};stop-opacity:.38` }, g); svg('stop', { offset: '100%', style: `stop-color:${c};stop-opacity:0` }, g);
        const area = svg('path', { fill: `url(#g${st.uid}${i})`, class: 'narea' }, areas);
        const ln = svg('path', { class: 'nline', pathLength: 1, style: `stroke:${c};color:${c}` }, lines);
        const ring = svg('circle', { class: 'nring', r: 4, style: `stroke:${c}` }, dots), dot = svg('circle', { class: 'ndot', r: 3, style: `fill:${c};color:${c}` }, dots);
        const hd = svg('circle', { r: 4.5, class: 'nhov', style: `stroke:${c}` }, hdots);
        st.els.push({ area, ln, ring, dot, hd });
      }
      while (st.els.length > n) { const e = st.els.pop(); Object.values(e).forEach((x) => x.remove()); }
    }
    function draw() {
      const o = st.o; if (!o) return;
      const W = st.W = wrap.clientWidth || 600, H = st.H = wrap.clientHeight || 180, m = st.m;
      s.setAttribute('viewBox', `0 0 ${W} ${H}`); s.setAttribute('width', W); s.setAttribute('height', H);
      const sets = o.series.map((x) => thin(norm(x.data), Math.max(30, Math.floor((W - m.l - m.r) / 2))));
      ensure(sets.length); st.pts = sets;
      const all = sets.flat(); if (!all.length) return;
      let x0 = Math.min(...all.map((p) => p.x)), x1 = Math.max(...all.map((p) => p.x)); if (x1 === x0) x1 = x0 + 1;
      const zero = o.zero === true;
      let y0 = zero ? 0 : Math.min(...all.map((p) => p.y)), y1 = Math.max(...all.map((p) => p.y), zero ? 1e-12 : -Infinity);
      if (!zero) { const pad = (y1 - y0) * 0.12 || Math.abs(y1) * 0.05 || 1; y0 -= pad; y1 += pad; if (o.floor0 !== false && y0 < 0 && all.every((p) => p.y >= 0)) y0 = 0; } else y1 *= 1.1;
      const ticks = nice(y0, y1); y0 = Math.min(y0, ticks[0]); y1 = Math.max(y1, ticks[ticks.length - 1]);
      const sx = (x) => m.l + (x - x0) / (x1 - x0) * (W - m.l - m.r), sy = (y) => H - m.b - (y - y0) / (y1 - y0 || 1) * (H - m.t - m.b);
      st.sx = sx; st.x0 = x0; st.x1 = x1;
      grid.textContent = '';
      const fy = o.fmt || ((v) => String(+v.toPrecision(3)));
      let prevLabel = null;
      for (const t of ticks) {
        svg('line', { x1: m.l, x2: W - m.r, y1: sy(t), y2: sy(t), class: 'ngrid' }, grid);
        const lab = fy(t);
        if (lab !== prevLabel) svg('text', { x: m.l - 6, y: sy(t) + 3.5, class: 'ntxt', 'text-anchor': 'end' }, grid).textContent = lab; // a repeated label (integer format of a flat series) adds nothing
        prevLabel = lab;
      }
      const fx = o.xfmt || ((x) => String(Math.round(x)));
      let prevX = null;
      [[x0, 'start'], [(x0 + x1) / 2, 'middle'], [x1, 'end']].forEach(([x, a]) => { const lab = fx(x); if (lab !== prevX || a === 'end') svg('text', { x: sx(x), y: H - 5, class: 'ntxt', 'text-anchor': a }, grid).textContent = lab; prevX = lab; });
      sets.forEach((pts, i) => {
        const e = st.els[i]; if (!pts.length) return;
        const d = pts.map((p, k) => `${k ? 'L' : 'M'}${sx(p.x).toFixed(1)},${sy(p.y).toFixed(1)}`).join('');
        e.ln.setAttribute('d', d);
        e.area.setAttribute('d', o.area === false ? '' : `${d}L${sx(pts[pts.length - 1].x).toFixed(1)},${H - m.b}L${sx(pts[0].x).toFixed(1)},${H - m.b}Z`);
        const last = pts[pts.length - 1];
        for (const k of ['ring', 'dot']) { e[k].setAttribute('cx', sx(last.x).toFixed(1)); e[k].setAttribute('cy', sy(last.y).toFixed(1)); }
      });
      over.setAttribute('x', m.l); over.setAttribute('y', m.t); over.setAttribute('width', W - m.l - m.r); over.setAttribute('height', H - m.t - m.b);
      cross.setAttribute('y1', m.t); cross.setAttribute('y2', H - m.b);
      st.sy = sy;
    }
    over.addEventListener('mousemove', (ev) => {
      const o = st.o; if (!o || !st.pts[0] || !st.pts[0].length) return;
      const r = over.getBoundingClientRect(), x = st.x0 + (ev.clientX - r.left) / r.width * (st.x1 - st.x0);
      const base = st.pts[0]; let bi = 0;
      for (let i = 1; i < base.length; i++) if (Math.abs(base[i].x - x) < Math.abs(base[bi].x - x)) bi = i;
      const px = st.sx(base[bi].x);
      cross.setAttribute('x1', px); cross.setAttribute('x2', px); cross.style.opacity = 1;
      const fy = o.fmt || ((v) => String(+v.toPrecision(3))), fx = o.xfmt || ((v) => String(Math.round(v)));
      tip.textContent = '';
      const head = document.createElement('b'); head.textContent = fx(base[bi].x); tip.append(head);
      st.pts.forEach((pts, i) => {
        if (!pts.length) return;
        let k = 0; for (let j = 1; j < pts.length; j++) if (Math.abs(pts[j].x - base[bi].x) < Math.abs(pts[k].x - base[bi].x)) k = j;
        st.els[i].hd.setAttribute('cx', st.sx(pts[k].x)); st.els[i].hd.setAttribute('cy', st.sy(pts[k].y)); st.els[i].hd.style.opacity = 1;
        const row = html('', tip), sw = document.createElement('i'), v = document.createElement('b');
        sw.style.background = col(o.series[i].color || PAL[i % PAL.length]); v.textContent = fy(pts[k].y);
        row.append(sw, `${o.series[i].name} `, v);
      });
      tip.style.opacity = 1;
      const left = px + 14 + tip.offsetWidth > st.W ? px - tip.offsetWidth - 14 : px + 14;
      tip.style.left = `${Math.max(0, left)}px`; tip.style.top = `${st.m.t + 4}px`;
    });
    over.addEventListener('mouseleave', () => { cross.style.opacity = 0; tip.style.opacity = 0; st.els.forEach((e) => { e.hd.style.opacity = 0; }); });
    if (window.ResizeObserver) new ResizeObserver(draw).observe(wrap);
    return { update(o) { st.o = o; draw(); } };
  }
  const line = (host, o) => { if (!host._neon) host._neon = lineChart(host); host._neon.update(o); };

  /* ---- list of keyed rows, reconciled in place ---- */
  function keyed(host, cls, rows, make, upd) {
    const map = host._rows || (host._rows = new Map());
    host.classList.add(cls);
    const keep = new Set();
    rows.forEach((r, i) => {
      const k = r.key ?? r.label; keep.add(k);
      let e = map.get(k), fresh = false;
      if (!e) { e = make(r); map.set(k, e); fresh = true; }
      if (host.children[i] !== e.el) host.insertBefore(e.el, host.children[i] || null);
      if (fresh) e.el.offsetWidth; // start the transition from the initial style
      upd(e, r, i, fresh);
    });
    for (const [k, e] of map) if (!keep.has(k)) { e.el.remove(); map.delete(k); }
  }

  /* ---- horizontal bars ---- */
  function bars(host, rows, o = {}) {
    const max = o.max ?? Math.max(...rows.map((r) => r.value), 1e-12);
    keyed(host, 'nbars', rows, (r) => {
      const el = html('nrow', null); const label = html('nlab', el, 'span'), track = html('ntrack', el), fill = html('nfill', track), val = html('nval', el, 'span');
      return { el, label, fill, val };
    }, (e, r, i) => {
      const c = col(r.color || PAL[i % PAL.length]);
      e.label.textContent = r.label; e.label.title = r.label; e.val.textContent = r.text ?? r.value;
      e.fill.style.background = `linear-gradient(90deg, transparent, ${c})`; e.fill.style.color = c;
      e.el.dataset.tip = `${r.label}: ${r.text ?? r.value}`;
      nextFrame(() => { e.fill.style.width = `${Math.max(0.5, r.value / max * 100)}%`; });
    });
  }

  /* ---- vertical columns (histogram) ---- */
  function columns(host, rows, o = {}) {
    const max = o.max ?? Math.max(...rows.map((r) => r.value), 1e-12);
    keyed(host, 'ncols', rows, () => {
      const el = html('ncol', null); const bar = html('nbar', el), lab = html('nclab', el);
      return { el, bar, lab };
    }, (e, r, i) => {
      const c = col(r.color || o.color || 'c1');
      e.bar.style.background = `linear-gradient(0deg, transparent, ${c})`; e.bar.style.color = c;
      e.lab.textContent = r.label; e.el.dataset.tip = `${r.tip ?? r.label}: ${r.text ?? r.value}`;
      nextFrame(() => { e.bar.style.height = `${Math.max(1, r.value / max * 100)}%`; });
    });
  }

  /* ---- donut with legend ---- */
  function donut(host, parts, o = {}) {
    if (!host._donut) {
      host.textContent = ''; host.classList.add('ndonut');
      const s = svg('svg', { viewBox: '0 0 120 120', class: 'ndsvg' }, host);
      svg('circle', { cx: 60, cy: 60, r: 44, class: 'ndtrack' }, s);
      const c = svg('g', { transform: 'rotate(-90 60 60)' }, s);
      const mid = svg('text', { x: 60, y: 62, class: 'ndbig', 'text-anchor': 'middle' }, s), sub = svg('text', { x: 60, y: 76, class: 'ndsub', 'text-anchor': 'middle' }, s);
      host._donut = { c, mid, sub, segs: new Map(), legend: html('nleg', host) };
    }
    const d = host._donut, C = 2 * Math.PI * 44, total = parts.reduce((a, p) => a + p.value, 0) || 1;
    const show = (p) => { d.mid.textContent = p ? `${(p.value / total * 100).toFixed(p.value / total < 0.1 ? 1 : 0)}%` : (o.big ?? ''); d.mid.style.fontSize = Math.min(15, 62 / (Math.max(1, d.mid.textContent.length) * 0.62)) + 'px'; d.sub.textContent = p ? p.label : (o.sub ?? ''); d.c.classList.toggle('hov', !!p); d.segs.forEach((e) => e.circle.classList.toggle('on', !!p && e.key === p.key)); };
    let cum = 0; const keep = new Set();
    parts.forEach((p, i) => {
      const key = p.key ?? p.label; keep.add(key); p.key = key;
      let e = d.segs.get(key);
      if (!e) {
        const circle = svg('circle', { cx: 60, cy: 60, r: 44, class: 'ndseg' }, d.c), row = html('nlrow', d.legend);
        row.innerHTML = '<i></i><span></span><b></b>';
        e = { key, circle, row }; d.segs.set(key, e); circle.style.strokeDasharray = `0 ${C}`;
        for (const t of [circle, row]) { t.addEventListener('mouseenter', () => show(e.p)); t.addEventListener('mouseleave', () => show(null)); }
      }
      e.p = p;
      const c = col(p.color || PAL[i % PAL.length]), len = p.value / total * C;
      e.circle.style.stroke = c; e.circle.style.color = c;
      e.row.querySelector('i').style.background = c; e.row.querySelector('span').textContent = p.label; e.row.querySelector('b').textContent = p.text ?? p.value;
      const off = -cum; cum += len;
      nextFrame(() => { e.circle.style.strokeDasharray = `${Math.max(0, len - 1.2)} ${C - Math.max(0, len - 1.2)}`; e.circle.style.strokeDashoffset = off; });
    });
    for (const [k, e] of d.segs) if (!keep.has(k)) { e.circle.remove(); e.row.remove(); d.segs.delete(k); }
    if (!d.c.classList.contains('hov')) show(null);
  }

  /* ---- big numbers with count-up ---- */
  function kpis(host, items) {
    keyed(host, 'nkpis', items.map((x) => ({ ...x, key: x.label })), (r) => {
      const el = html('nkpi', null); const v = html('nkv', el), l = html('nkl', el), sub = html('nks', el);
      return { el, v, l, sub, cur: null };
    }, (e, r) => {
      const c = col(r.color || 'c1');
      e.v.style.color = c; e.l.textContent = r.label; e.sub.textContent = r.sub || '';
      const f = r.fmt || ((v) => String(v));
      if (typeof r.value !== 'number' || e.cur === null) { e.v.textContent = f(r.value); }
      else if (r.value !== e.cur) {
        const from = e.cur, to = r.value, t0 = performance.now();
        const step = (t) => { const k = Math.min(1, (t - t0) / 600), eased = 1 - (1 - k) ** 3; e.v.textContent = f(from + (to - from) * eased); if (k < 1) requestAnimationFrame(step); else e.v.textContent = f(to); };
        requestAnimationFrame(step);
      }
      e.cur = typeof r.value === 'number' ? r.value : null;
    });
  }

  return { line, bars, columns, donut, kpis };
})();

// Time series kept in the browser while the page is open (the pool API only returns the last minute).
const Series = (() => {
  const m = {};
  return {
    push(name, y, t = Date.now() / 1000) { const a = m[name] || (m[name] = []); if (Number.isFinite(y)) a.push({ x: t, y }); if (a.length > 3600) a.shift(); },
    get: (name) => m[name] || [],
    empty: (name) => !(m[name] && m[name].length),
    seed(name, ys, step = 1) { if (Series.empty(name)) { const now = Date.now() / 1000; ys.forEach((y, i) => Series.push(name, y, now - (ys.length - 1 - i) * step)); } },
  };
})();
const agoLabel = (x) => { const d = Math.max(0, Date.now() / 1000 - x); return d < 3 ? 'now' : d < 90 ? `-${Math.round(d)}s` : d < 5400 ? `-${Math.round(d / 60)}m` : d < 172800 ? `-${(d / 3600).toFixed(1)}h` : `-${(d / 86400).toFixed(1)}d`; };

// Extra panels of a maximized pane: Extras.add('miners', (el) => { ...build once...; return () => { ...update... }; })
const Extras = (() => {
  const reg = {};
  return {
    add(id, build) { reg[id] = { build, up: null }; },
    row(parent) { const r = document.createElement('div'); r.className = 'crow'; parent.append(r); return r; },
    card(parent, title, cls = '') { const c = document.createElement('section'); c.className = 'card ' + cls; const h = document.createElement('h3'); h.textContent = title; h.title = title; const b = document.createElement('div'); b.className = 'cbody'; c.append(h, b); parent.append(c); return b; },
    tick() {
      for (const id in reg) {
        const p = document.querySelector('.p-' + id); if (!p) continue;
        let ex = p.querySelector(':scope > .extra');
        if (!p.classList.contains('front')) { if (ex) { ex.remove(); p.classList.remove('has-extra'); } reg[id].up = null; continue; }
        if (!ex) { ex = document.createElement('div'); ex.className = 'extra'; p.append(ex); p.classList.add('has-extra'); reg[id].up = null; }
        if (!reg[id].up) reg[id].up = reg[id].build(ex);
        reg[id].up();
      }
    },
  };
})();

// A card in a row whose text is cut off grows while the mouse rests on it; the other cards of the row shrink a little to make room.
// The growth always starts from the row's own layout (never from a width in the middle of a transition) and waits a moment, so moving the
// pointer across several cards neither accumulates nor flickers.
(() => {
  const cut = (card) => card.scrollWidth > card.clientWidth + 1 || [...card.querySelectorAll('*')].some((e) => e.clientWidth && e.scrollWidth > e.clientWidth + 1 && getComputedStyle(e).overflow !== 'visible');
  const rest = (row) => { clearTimeout(row._g); clearTimeout(row._t); if (row._base) { row.style.gridTemplateColumns = row._inline; row._base = null; } row._grown = null; };
  const cardOf = (ev) => ev.target.closest && ev.target.closest('.crow > .card');
  document.addEventListener('mouseover', (ev) => {
    const card = cardOf(ev);
    if (!card) return;
    const row = card.parentElement;
    if (row._grown === card || row._want === card) return;
    clearTimeout(row._g);
    row._want = card;
    row._g = setTimeout(() => {
      row._want = null;
      if (!card.matches(':hover') || getComputedStyle(row).display !== 'grid') return;
      clearTimeout(row._t);
      const cols = row._base || getComputedStyle(row).gridTemplateColumns.split(' ').map(parseFloat);
      const kids = [...row.children];
      if (cols.length !== kids.length || cols.some((w) => !(w > 0)) || kids.some((k) => k.offsetLeft > 0 && k.offsetTop !== kids[0].offsetTop) || !cut(card)) return;
      const i = kids.indexOf(card), total = cols.reduce((a, b) => a + b, 0), want = Math.min(total * 0.6, cols[i] * 1.5);
      const scale = (total - want) / (total - cols[i]);
      if (!row._base) { row._base = cols; row._inline = row.style.gridTemplateColumns; }
      row.style.gridTemplateColumns = cols.map((w, k) => (k === i ? want : w * scale) + 'px').join(' ');
      row._grown = card;
    }, 160);
  });
  document.addEventListener('mouseout', (ev) => {
    const card = cardOf(ev);
    if (!card || (ev.relatedTarget && card.contains(ev.relatedTarget))) return;
    const row = card.parentElement;
    clearTimeout(row._g); row._want = null;
    if (row._grown !== card) return;
    row._grown = null;
    row.style.gridTemplateColumns = row._base.map((w) => w + 'px').join(' ');
    row._t = setTimeout(() => rest(row), 300);
  });
  addEventListener('resize', () => document.querySelectorAll('.crow').forEach(rest));
})();
