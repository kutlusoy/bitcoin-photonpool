'use strict';
// Single-page shell of the public pages: one document with a persistent title bar (music player, clock), views for the pool and the
// engine, miner and operator views that are swapped in place, so the music keeps playing across pages. Paths: / (pool), /engine,
// /miner and /miner/<address>, /ops (operator view, only where the admin API answers, i.e. on the operator port).
window.SPA = (() => {
  let current = null, token = 0;
  const routeOf = (path) => (/^\/miner(\.html|\/|$)/.test(path) ? 'miner' : path === '/engine' ? 'engine' : path === '/ops' ? 'ops' : 'pool');
  // the operator view exists only where the admin API answers; its script is loaded on demand and never offered by the public listener
  const opsReady = (async () => {
    try {
      const r = await fetch('/api/v1/summary', { cache: 'no-store' });
      if (!r.ok) return false;
      await new Promise((ok, no) => { const sc = document.createElement('script'); sc.src = '/ops.js'; sc.onload = ok; sc.onerror = no; document.head.append(sc); });
      document.querySelectorAll('[data-nav=ops]').forEach((a) => { a.hidden = false; });
      return !!(window.Views && Views.ops);
    } catch (_) { return false; }
  })();
  const root = () => document.getElementById('view');

  function cascade(r) {
    const grid = r.querySelector('main.grid');
    if (!grid) return;
    [...grid.querySelectorAll('.pane')].forEach((p, i) => p.style.setProperty('--i', i));
    grid.classList.add('entering');
    setTimeout(() => grid.classList.remove('entering'), 1200);
  }

  async function show(path, instant) {
    let name = routeOf(path);
    const mine = ++token;
    if (name === 'ops' && !(await opsReady)) { name = 'pool'; history.replaceState(null, '', '/'); }
    if (mine !== token) return;
    const swap = () => {
      if (mine !== token) return;
      if (current) { try { Views[current].unmount(); } catch (_) { /* view already gone */ } }
      if (typeof Depth !== 'undefined') Depth.detach();
      const r = root();
      r.textContent = '';
      r.className = 'view v-' + name;
      current = name;
      document.querySelectorAll('[data-nav]').forEach((a) => a.classList.toggle('on', a.dataset.nav === name));
      document.title = pageTitle(name === 'pool' ? '' : name === 'engine' ? 'Engine' : name === 'ops' ? 'Operator' : 'Miner');
      Views[name].mount(r);
      if (typeof Depth !== 'undefined') Depth.attach();
      cascade(r);
    };
    if (!instant && current && document.startViewTransition && !matchMedia('(prefers-reduced-motion: reduce)').matches) {
      try { await document.startViewTransition(swap).finished; } catch (_) { /* transition skipped */ }
    } else swap();
  }

  function go(path, replace) {
    if (path === location.pathname + location.search) return;
    history[replace ? 'replaceState' : 'pushState'](null, '', path);
    show(path);
  }

  document.addEventListener('click', (e) => {
    const a = e.target.closest && e.target.closest('a[href]');
    if (!a || a.target || e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const u = new URL(a.href, location.href);
    if (u.origin !== location.origin || !(u.pathname === '/' || /^\/(miner|engine|ops)/.test(u.pathname))) return;
    e.preventDefault();
    go(u.pathname + u.search);
  });
  addEventListener('popstate', () => show(location.pathname));
  document.addEventListener('DOMContentLoaded', () => {
    setInterval(() => { const c = document.getElementById('clock'); if (c) c.textContent = new Date().toISOString().slice(11, 19) + ' UTC'; }, 500);
    show(location.pathname, true);
  });
  return { go };
})();
