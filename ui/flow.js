'use strict';
// Live scene of the engine in pseudo-3D (canvas 2D, no libraries): the node at the vanishing point, jobs flying toward the miners along one lane per
// candidate, shares flying back, a wave over the ground for every new chain tip, a flash for every found block. The speed of the ground follows the
// hash rate; the number of lanes, jobs and shares follows the real counters of the pool. About 30 frames per second, at most ~250 particles, no blur
// filters, paused while hidden or out of view.
window.Flow = (() => {
  const MAX = 250;
  function create(canvas) {
    const ctx = canvas.getContext('2d');
    const st = { w: 0, h: 0, dpr: 1, parts: [], waves: [], flashes: [], off: 0, last: 0, run: false, raf: 0, vis: true, col: {}, colT: 0,
      jobs: 0, shares: 0, units: 0, cands: 8, hr: 0, jobsT: 0, sharesT: 0, accJ: 0, accS: 0, tip: 0, label: '', labelT: 0 };
    const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
    const colors = () => {
      const cs = getComputedStyle(document.body);
      for (const k of ['c1', 'c2', 'c3', 'ok', 'dim', 'line']) st.col[k] = cs.getPropertyValue('--' + k).trim() || '#0ff';
    };
    const size = () => {
      const r = canvas.getBoundingClientRect(), dpr = Math.min(1.5, devicePixelRatio || 1);
      st.dpr = dpr; st.w = Math.max(10, r.width); st.h = Math.max(10, r.height);
      canvas.width = Math.round(st.w * dpr); canvas.height = Math.round(st.h * dpr);
    };
    const ro = new ResizeObserver(size); ro.observe(canvas);
    const io = new IntersectionObserver((e) => { st.vis = e[0].isIntersecting; kick(); }); io.observe(canvas);
    const onVis = () => kick(); document.addEventListener('visibilitychange', onVis);

    const L = () => Math.max(4, Math.min(16, st.cands | 0));
    const geo = () => { const w = st.w, h = st.h; return { w, h, vx: w / 2, hz: h * 0.3 }; };
    const lane = (g, i, n, p) => { const xb = g.w * (0.06 + 0.88 * (i + 0.5) / n); return [g.vx + (xb - g.vx) * p, g.hz + (g.h - g.hz) * p]; };

    function spawn(kind) {
      if (st.parts.length >= MAX) return;
      const n = L();
      st.parts.push({ k: kind, lane: (Math.random() * n) | 0, n, p: kind === 'job' ? 0.02 : 0.98, v: 0.35 + Math.random() * 0.5, j: (Math.random() - 0.5) * 0.35 });
    }
    function step(dt) {
      // smooth the numbers
      const a = 1 - Math.exp(-dt * 2);
      st.jobs += (st.jobsT - st.jobs) * a; st.shares += (st.sharesT - st.shares) * a;
      // particles per second: a small base so the scene is never dead, then growing with the real rate (logarithmic: 1000 jobs/s must not mean 1000 shapes)
      st.accJ += dt * (3 + 9 * Math.log10(1 + st.jobs)); st.accS += dt * (1 + 7 * Math.log10(1 + st.shares * 4));
      while (st.accJ >= 1) { st.accJ -= 1; spawn('job'); }
      while (st.accS >= 1) { st.accS -= 1; spawn('share'); }
      for (const q of st.parts) q.p += (q.k === 'job' ? 1 : -1) * q.v * dt * (0.4 + q.p);
      st.parts = st.parts.filter((q) => q.p > 0.01 && q.p < 1.02);
      st.waves.forEach((w) => { w.p += dt * 0.55; }); st.waves = st.waves.filter((w) => w.p < 1.05);
      st.flashes.forEach((f) => { f.t += dt; }); st.flashes = st.flashes.filter((f) => f.t < 2.2);
      st.off = (st.off + dt * (0.15 + 0.12 * Math.log10(1 + st.hr / 1e6))) % 1;
    }
    function draw() {
      const g = geo(), { w, h, vx, hz } = g, c = st.col;
      ctx.setTransform(st.dpr, 0, 0, st.dpr, 0, 0);
      ctx.globalCompositeOperation = 'source-over'; ctx.globalAlpha = 1;
      ctx.clearRect(0, 0, w, h);
      const n = L();
      // ground: horizontal lines moving toward the viewer, lanes radiating from the node
      ctx.lineWidth = 1; ctx.strokeStyle = c.line;
      const K = 14;
      for (let k = 0; k < K; k++) {
        const p = Math.pow((k + st.off) / K, 2.2), y = hz + (h - hz) * p, half = (w / 2) * (0.04 + 0.96 * p);
        ctx.globalAlpha = 0.15 + 0.6 * p; ctx.beginPath(); ctx.moveTo(vx - half * 1.05, y); ctx.lineTo(vx + half * 1.05, y); ctx.stroke();
      }
      ctx.globalAlpha = 0.5; ctx.strokeStyle = c.line;
      for (let i = 0; i <= n; i++) {
        const xb = w * (0.06 + 0.88 * i / n); ctx.beginPath(); ctx.moveTo(vx, hz); ctx.lineTo(xb, h); ctx.stroke();
      }
      ctx.globalCompositeOperation = 'lighter';
      // the node at the vanishing point
      const pulse = 0.5 + 0.5 * Math.sin(performance.now() / 420), r = 9 + 3 * pulse;
      const gr = ctx.createRadialGradient(vx, hz, 0, vx, hz, r * 3);
      gr.addColorStop(0, c.c1); gr.addColorStop(1, 'rgba(0,0,0,0)');
      ctx.globalAlpha = 0.55; ctx.fillStyle = gr; ctx.beginPath(); ctx.arc(vx, hz, r * 3, 0, 7); ctx.fill();
      ctx.globalAlpha = 1; ctx.fillStyle = c.c1; ctx.beginPath(); ctx.arc(vx, hz, 3.5, 0, 7); ctx.fill();
      // new-tip waves: a bright line sweeping over the ground toward the miners
      for (const wv of st.waves) {
        const p = wv.p * wv.p, y = hz + (h - hz) * p, half = (w / 2) * (0.04 + 0.96 * p);
        ctx.globalAlpha = Math.max(0, 1 - wv.p) * 0.9; ctx.strokeStyle = c.c3; ctx.lineWidth = 1 + 3 * p;
        ctx.beginPath(); ctx.moveTo(vx - half, y); ctx.lineTo(vx + half, y); ctx.stroke();
      }
      // particles: jobs come from the node, shares go back to it
      for (const q of st.parts) {
        const at = (pp) => { const p = pp * pp, [x0, y0] = lane(g, q.lane, q.n, p); return [x0 + q.j * (w / q.n) * p * 0.5, y0, p]; };
        const [x, y, p] = at(q.p), [tx, ty] = at(q.k === 'job' ? q.p - 0.07 : q.p + 0.07); // the trail lies behind the particle
        ctx.strokeStyle = q.k === 'job' ? c.c1 : c.ok; ctx.fillStyle = ctx.strokeStyle;
        ctx.globalAlpha = Math.min(1, 0.25 + p * 1.1); ctx.lineWidth = 0.6 + 2.2 * p;
        ctx.beginPath(); ctx.moveTo(x, y); ctx.lineTo(tx, ty); ctx.stroke();
        ctx.beginPath(); ctx.arc(x, y, 0.8 + 2.6 * p, 0, 7); ctx.fill();
      }
      // found block: ring and text
      for (const f of st.flashes) {
        const a = Math.max(0, 1 - f.t / 2.2);
        ctx.globalAlpha = a * 0.8; ctx.strokeStyle = c.ok; ctx.lineWidth = 2;
        ctx.beginPath(); ctx.ellipse(vx, hz, 20 + f.t * w * 0.3, 8 + f.t * h * 0.12, 0, 0, 7); ctx.stroke();
      }
      // labels
      ctx.globalCompositeOperation = 'source-over'; ctx.font = '10px ui-monospace, monospace'; ctx.fillStyle = c.dim; ctx.globalAlpha = 0.95;
      ctx.textAlign = 'center'; ctx.fillText('NODE', vx, hz - 20); ctx.fillText('MINERS', vx, h - 6);
      ctx.textAlign = 'left';
      ctx.fillText(`${n} lanes  ${st.jobsT.toFixed(0)} jobs/s  ${st.sharesT.toFixed(1)} shares/s  ${st.units.toFixed(0)} units/s`, 8, 14);
      if (st.label && performance.now() - st.labelT < 4000) {
        ctx.fillStyle = c.c3; ctx.globalAlpha = Math.max(0, 1 - (performance.now() - st.labelT) / 4000);
        ctx.textAlign = 'center'; ctx.fillText(st.label, vx, hz + 24);
      }
      ctx.globalAlpha = 1;
    }
    function frame(ts) {
      st.raf = 0;
      if (!st.run) return;
      if (document.hidden || !st.vis) { st.last = 0; return; }
      const dt = st.last ? Math.min(0.1, (ts - st.last) / 1000) : 0.033;
      if (ts - st.last >= 32 || !st.last) { // ~30 fps
        if (ts - st.colT > 1000) { colors(); st.colT = ts; }
        step(dt); draw(); st.last = ts;
      }
      st.raf = requestAnimationFrame(frame);
    }
    function kick() { if (st.run && !st.raf && st.vis && !document.hidden) st.raf = requestAnimationFrame(frame); }
    colors(); size();
    return {
      start() { st.run = true; if (reduce) { step(0.5); draw(); return; } kick(); },
      stop() { st.run = false; cancelAnimationFrame(st.raf); ro.disconnect(); io.disconnect(); document.removeEventListener('visibilitychange', onVis); },
      // real numbers of the pool: jobs/s sent, shares/s received, units/s allocated, candidates (lanes), hash rate
      feed(d) { st.jobsT = d.jobs || 0; st.sharesT = d.shares || 0; st.units = d.units || 0; st.cands = d.cands || st.cands; st.hr = d.hr || 0; },
      tip(height) { if (st.tip && height !== st.tip) { st.waves.push({ p: 0 }); st.label = 'new tip ' + height; st.labelT = performance.now(); } st.tip = height; },
      block() { st.flashes.push({ t: 0 }); st.label = 'BLOCK FOUND'; st.labelT = performance.now(); },
    };
  }
  return { create };
})();
