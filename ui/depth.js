'use strict';
// Depth view: a pane can be maximized (button in its title, double-click on the title, click on a pane in
// the background); the other panes then recede in 3D and blur. Esc, the button or a double-click goes back.
const Depth = (() => {
  let detachCurrent = null;
  function attach() {
  detach();
  const grid = document.querySelector('main.grid');
  if (!grid) return;
  const offs = [];
  const on = (t, ev, fn) => { t.addEventListener(ev, fn); offs.push(() => t.removeEventListener(ev, fn)); };
  const narrow = matchMedia('(max-width: 720px)');
  let cur = null;

  const px = (v) => `${Math.round(v)}px`;
  const place = (el, r) => { el.style.left = px(r.left); el.style.top = px(r.top); el.style.width = px(r.width); el.style.height = px(r.height); };
  // layout box relative to the grid, unaffected by the 3D transforms
  const rel = (el) => ({ left: el.offsetLeft, top: el.offsetTop, width: el.offsetWidth, height: el.offsetHeight });
  const full = () => {
    const cs = getComputedStyle(grid), g = { width: grid.clientWidth, height: grid.clientHeight };
    const l = parseFloat(cs.paddingLeft), t = parseFloat(cs.paddingTop);
    const w = g.width - l - parseFloat(cs.paddingRight), h = g.height - t - parseFloat(cs.paddingBottom);
    // leave a margin so that the receding panes stay visible around the front one
    const mx = w * 0.11, my = h * 0.12;
    return { left: l + mx, top: t + my, width: w - 2 * mx, height: h - 2 * my };
  };
  // A pane that leaves the front flies back into its slot while it recedes; the placeholder keeps the slot free meanwhile.
  const finish = (p) => {
    p.classList.remove('front', 'leaving');
    p.style.left = p.style.top = p.style.width = p.style.height = '';
    if (p._ph) p._ph.remove();
    p._ph = null;
  };
  const release = (p) => {
    p.classList.add('leaving');
    place(p, rel(p._ph));
    clearTimeout(p._t);
    p._t = setTimeout(() => finish(p), 700);
  };

  function focus(p) {
    if (narrow.matches) return;
    if (cur && cur !== p) release(cur);
    cur = p;
    const gw = grid.clientWidth, gh = grid.clientHeight;
    for (const q of grid.querySelectorAll('.pane')) {
      const r = rel(q._ph || q);
      q.style.setProperty('--dx', (((r.left + r.width / 2) - gw / 2) / gw * 2).toFixed(3));
      q.style.setProperty('--dy', (((r.top + r.height / 2) - gh / 2) / gh * 2).toFixed(3));
    }
    if (p._ph) {
      clearTimeout(p._t);
      p.classList.remove('leaving');
    } else {
      const start = rel(p);
      p._ph = document.createElement('div');
      p._ph.style.gridArea = getComputedStyle(p).gridArea;
      p.before(p._ph);
      place(p, start);
      p.classList.add('front');
      p.getBoundingClientRect();
    }
    grid.classList.add('depth');
    place(p, full());
    document.dispatchEvent(new Event('depthchange'));
  }

  function restore() {
    if (!cur) return;
    release(cur);
    cur = null;
    grid.classList.remove('depth');
    document.dispatchEvent(new Event('depthchange'));
  }

  const toggle = (p) => (cur === p ? restore() : focus(p));

  for (const p of grid.querySelectorAll('.pane')) {
    const h = p.querySelector('h2');
    if (!h) continue;
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'max';
    b.textContent = '⤢';
    b.title = 'maximize (Esc to go back)';
    b.setAttribute('aria-label', 'maximize ' + (p.getAttribute('aria-label') || 'pane'));
    b.addEventListener('click', (e) => { e.stopPropagation(); toggle(p); });
    h.append(b);
    h.addEventListener('dblclick', () => toggle(p));
    p.addEventListener('click', () => { if (cur && p !== cur) focus(p); });
  }
  on(document, 'keydown', (e) => { if (e.key === 'Escape') restore(); });
  on(grid, 'mousemove', (e) => {
    if (!grid.classList.contains('depth')) return;
    const g = grid.getBoundingClientRect();
    grid.style.setProperty('--mx', (((e.clientX - g.left) / g.width) * 2 - 1).toFixed(3));
    grid.style.setProperty('--my', (((e.clientY - g.top) / g.height) * 2 - 1).toFixed(3));
  });
  on(narrow, 'change', () => { if (narrow.matches) { grid.classList.remove('depth'); for (const q of grid.querySelectorAll('.pane')) { clearTimeout(q._t); finish(q); } cur = null; } });
  on(window, 'resize', () => { if (cur && grid.classList.contains('depth')) place(cur, full()); });
  detachCurrent = () => { offs.forEach((f) => f()); for (const q of grid.querySelectorAll('.pane')) { clearTimeout(q._t); q.classList.remove('front', 'leaving'); } grid.classList.remove('depth'); document.dispatchEvent(new Event('depthchange')); };
  }
  function detach() { if (detachCurrent) { detachCurrent(); detachCurrent = null; } }
  return { attach, detach };
})();
// pages without the single-page shell (the operator page) attach at once
if (!window.SPA) Depth.attach();
