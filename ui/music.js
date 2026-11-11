'use strict';
// Original generative dark-synth score for the pool pages: synthesized live with the Web Audio API, no audio files
// (a few KB of code, no downloads). One full arrangement is 224 bars at 84 BPM (about 10 min 40 s), then it loops
// seamlessly. Every note is derived from fixed seeds, so the loop repeats exactly. Starts only on a click (browser rule).
const Music = (() => {
  const BPM = 84, STEP = 60 / BPM / 4, BARS = 224, STEPS = BARS * 16, TOTAL = STEPS * STEP;
  const hz = (n) => 440 * 2 ** ((n - 69) / 12);
  const rng = (seed) => () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; };
  const MIN = [0, 3, 7], MAJ = [0, 4, 7];
  // chord progressions: [bass root (MIDI), chord quality]
  const PROG = [
    [[29, MIN], [29, MIN], [32, MAJ], [34, MIN]], // Fm Fm Ab Bbm: long drone on F
    [[29, MIN], [37, MAJ], [32, MAJ], [34, MIN]], // Fm Db Ab Bbm
  ];
  const PENT = [65, 68, 70, 72, 75, 77, 80]; // F minor pentatonic for the lead
  // 14 sections of 16 bars: drums, hats, bass pattern, arp, pad, lead, progression
  const S = (kick, hat, bass, arp, pad, lead, prog, snare = kick) => ({ kick, hat, bass, arp, pad, lead, prog, snare });
  const SECTIONS = [
    S(0, 0, 0, 0, 1, 0, 0), S(0, 0, 1, 0, 1, 0, 0), S(1, 1, 1, 0, 1, 0, 0), S(1, 1, 2, 1, 1, 0, 0),
    S(2, 2, 2, 1, 1, 1, 0), S(2, 2, 2, 1, 1, 1, 1), S(0, 0, 0, 1, 1, 1, 1), S(0, 1, 1, 1, 1, 0, 0, 3),
    S(2, 2, 3, 1, 1, 1, 1), S(2, 2, 3, 1, 1, 1, 0), S(0, 0, 1, 0, 1, 0, 1), S(2, 2, 3, 1, 1, 1, 0),
    S(1, 1, 2, 1, 1, 0, 1), S(0, 0, 0, 0, 1, 0, 0),
  ];
  const BASSPAT = ['................', 'x...............', 'x.....x.x.....x.', 'x..x..x.x..x.xx.']; // pattern 1 is a sustained drone

  function build(ctx, dest) {
    const out = ctx.createGain(); out.gain.value = 0.5;
    const comp = ctx.createDynamicsCompressor(); comp.threshold.value = -16; comp.ratio.value = 4; comp.attack.value = 0.01; comp.release.value = 0.2;
    out.connect(comp); comp.connect(dest);
    const analyser = ctx.createAnalyser(); analyser.fftSize = 64; analyser.smoothingTimeConstant = 0.8; comp.connect(analyser);
    const dry = ctx.createGain(); dry.connect(out);
    const mus = ctx.createGain(); mus.connect(out); // everything melodic and low goes through here, ducked by the kick
    const delay = ctx.createDelay(1.5); delay.delayTime.value = STEP * 6; // dotted eighth
    const fb = ctx.createGain(); fb.gain.value = 0.34; const dlp = ctx.createBiquadFilter(); dlp.type = 'lowpass'; dlp.frequency.value = 2400;
    const wet = ctx.createGain(); wet.gain.value = 0.26;
    delay.connect(dlp); dlp.connect(fb); fb.connect(delay); dlp.connect(wet); wet.connect(out);
    const send = ctx.createGain(); send.connect(delay);
    const nb = ctx.createBuffer(1, ctx.sampleRate, ctx.sampleRate), nd = nb.getChannelData(0);
    for (let i = 0; i < nd.length; i++) nd[i] = Math.random() * 2 - 1;
    const noise = (t, dur, type, f, q, vol, to = dry) => {
      const src = ctx.createBufferSource(); src.buffer = nb; src.loop = true;
      const fl = ctx.createBiquadFilter(); fl.type = type; fl.frequency.value = f; fl.Q.value = q;
      const g = ctx.createGain(); g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(vol, t + 0.003); g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
      src.connect(fl); fl.connect(g); g.connect(to); src.start(t, Math.random() * 0.5); src.stop(t + dur + 0.05);
    };
    const tone = (t, dur, type, f, vol, to, o = {}) => {
      const osc = ctx.createOscillator(); osc.type = type; osc.frequency.setValueAtTime(f, t); if (o.det) osc.detune.value = o.det;
      if (o.to) osc.frequency.exponentialRampToValueAtTime(o.to, t + (o.glide || dur));
      const g = ctx.createGain(); const a = o.att || 0.004;
      g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(vol, t + a); g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
      let node = osc;
      if (o.lp) { const fl = ctx.createBiquadFilter(); fl.type = 'lowpass'; fl.Q.value = o.q || 1; fl.frequency.setValueAtTime(o.lp, t); if (o.lpTo) fl.frequency.exponentialRampToValueAtTime(o.lpTo, t + dur * 0.8); osc.connect(fl); node = fl; }
      node.connect(g); g.connect(to); osc.start(t); osc.stop(t + dur + 0.05);
      return osc;
    };
    const kick = (t, v) => tone(t, 0.45, 'sine', 68, v * 0.55, dry, { to: 40, glide: 0.08 });
    const snare = (t, v) => { noise(t, 0.12, 'bandpass', 1600, 0.9, v * 0.22); tone(t, 0.16, 'triangle', 200, v * 0.5, dry, { to: 130 }); };
    const hat = (t, v, open) => noise(t, open ? 0.12 : 0.03, 'highpass', 9500, 0.5, v * 0.45);
    const bass = (t, f, v, len) => { tone(t, len, 'sawtooth', f, v * 0.14, mus, { lp: 200, lpTo: 80, q: 1.5 }); tone(t, len, 'sine', f, v * 2.4, mus, { att: 0.03 }); tone(t, len, 'sine', f * 2, v * 0.2, mus, { att: 0.05 }); };
    const pluck = (t, f, v) => { tone(t, 0.3, 'sawtooth', f, v, mus, { lp: 1700, lpTo: 400, q: 2 }); tone(t, 0.3, 'square', f, v * 0.5, send, { lp: 1500, lpTo: 450, det: 6 }); };
    const pad = (t, dur, notes, v) => notes.forEach((n) => { [-6, 6].forEach((d) => tone(t, dur, 'sawtooth', hz(n), v / 4, send, { att: 1.8, det: d, lp: 520, lpTo: 820 })); tone(t, dur, 'sine', hz(n), v / 2, send, { att: 1.8 }); });
    const lead = (t, dur, n, v) => {
      const o = tone(t, dur, 'triangle', hz(n), v, send, { att: 0.02, lp: 3000 });
      const lfo = ctx.createOscillator(), lg = ctx.createGain(); lfo.frequency.value = 5; lg.gain.value = 7; lfo.connect(lg); lg.connect(o.detune); lfo.start(t); lfo.stop(t + dur + 0.05);
    };

    function step(i, t) {
      const bar = Math.floor(i / 16), s = i % 16, sec = SECTIONS[Math.floor(bar / 16)], barIn = bar % 16, chord = PROG[sec.prog][bar % 4];
      const r = rng(bar * 7919 + 13);
      if (s % 4 === 0) { const depth = sec.kick ? 0.28 : 0.7; mus.gain.cancelScheduledValues(t); mus.gain.setValueAtTime(depth, t); mus.gain.linearRampToValueAtTime(1, t + STEP * 3.2); }
      if (sec.kick) {
        if (s % 4 === 0) kick(t, 0.95);
        else if (sec.kick === 2 && s === 10 && barIn % 2 === 1) kick(t, 0.6);
        else if (sec.kick === 1 && s === 10) kick(t, 0.5);
      }
      if (sec.snare && (s === 4 || s === 12)) snare(t, 0.7);
      if (sec.snare === 3 && barIn >= 14 && s % 2 === 0) snare(t, 0.25 + 0.05 * ((barIn - 14) * 8 + s / 2));
      if (sec.hat === 1) hat(t, s % 4 === 2 ? 0.045 : s % 2 === 0 ? 0.028 : 0.014);
      if (sec.hat === 2) hat(t, s % 4 === 2 ? 0.06 : s % 2 === 0 ? 0.034 : 0.018, s === 14 && barIn % 2 === 1);
      if (barIn === 15 && sec.snare && s >= 12 && sec.snare !== 3) snare(t, 0.4);
      const bp = BASSPAT[sec.bass];
      if (bp[s] === 'x') bass(t, hz((chord[0] >= 37 ? chord[0] - 12 : chord[0]) + (s === 11 || s === 14 ? 12 : 0)), sec.bass === 1 ? 0.42 : 0.36, sec.bass === 1 ? STEP * 15 : STEP * 2.2);
      if (s === 0 && sec.pad) pad(t, STEP * 17, chord[1].map((o) => chord[0] + 24 + o), 0.085);
      if (sec.arp && (s % 2 === 0 || (sec.arp === 1 && r() < 0.28))) {
        const tones = chord[1].map((o) => chord[0] + 36 + o), k = (Math.floor(s / 2) + (barIn >> 2)) % tones.length;
        pluck(t, hz(tones[k] + (r() < 0.25 ? 12 : 0)), s % 4 === 0 ? 0.11 : 0.07);
      }
      if (sec.lead && s % 4 === 0 && barIn % 2 === 0 && r() < 0.75) lead(t, STEP * (4 + 4 * Math.floor(r() * 3)), PENT[Math.floor(r() * PENT.length)], 0.09);
    }
    return { step, out, analyser, startRain() {}, stopRain() {} };
  }

  /* ---- player ---- */
  // Two sources behind one set of controls: audio files from the server's music folder (streamed by an <audio> element, looped as
  // a playlist) when there are any, otherwise the generated score above.
  const KEY = 'photon.music.volume.v2'; // new key: older saved values (default was higher) do not apply
  const P = { ctx: null, b: null, master: null, timer: 0, next: 0, step: 0, t0: 0, state: 'stopped', vol: 0.28, tracks: [], idx: 0, seek: 0, armed: false, audio: null, an: null, actx: null, onTrack: () => {} };
  try { const v = parseFloat(localStorage.getItem(KEY)); if (v >= 0 && v <= 1) P.vol = v; } catch (_) { /* storage unavailable */ }
  const gainOf = (v) => v * v;
  const files = () => P.tracks.length > 0;

  function pump() {
    while (P.next < P.ctx.currentTime + 0.25) { P.b.step(P.step, P.next); P.step = (P.step + 1) % STEPS; P.next += STEP; }
  }
  function loadTrack(play) {
    const t = P.tracks[P.idx];
    P.audio.src = '/music/' + encodeURIComponent(t.file); // absolute: the page can be /miner/<address>
    P.onTrack(t);
    if (play) P.audio.play().catch(() => { P.state = 'stopped'; });
  }
  function filePlayer() {
    if (P.audio) return;
    const a = P.audio = new Audio(); a.preload = 'none'; a.volume = gainOf(P.vol);
    a.addEventListener('ended', () => skip(1));
    a.addEventListener('loadedmetadata', () => { if (P.seek > 0 && P.seek < a.duration - 3) a.currentTime = P.seek; P.seek = 0; });
    a.addEventListener('error', () => { if (P.tracks.length > 1) skip(1); });
    try {
      const AC = window.AudioContext || window.webkitAudioContext; P.actx = new AC();
      P.an = P.actx.createAnalyser(); P.an.fftSize = 64; P.an.smoothingTimeConstant = 0.8;
      P.actx.createMediaElementSource(a).connect(P.an); P.an.connect(P.actx.destination);
    } catch (_) { P.an = null; }
  }
  // Browsers only let an AudioContext run after a real user gesture (on touch screens the touch end, not the touch start). If it was created in a gesture that
  // does not count, the player would show "playing" while the audio element is connected to a suspended context: silence and a time that stays at 0.
  // So every later gesture tries again until the context runs.
  const ctxNow = () => P.actx || P.ctx;
  function unlockAudio() {
    if (!ctxNow() || P.unlocking) return;
    P.unlocking = true;
    const evs = ['pointerup', 'touchend', 'click', 'keydown'];
    const done = () => { evs.forEach((n) => document.removeEventListener(n, fix, true)); P.unlocking = false; };
    function fix() {
      const c = ctxNow();
      if (c && c.state === 'suspended' && P.state === 'playing') c.resume().catch(() => {});
      if (P.state === 'playing' && P.audio && P.audio.paused) P.audio.play().catch(() => {});
      if (!c || c.state === 'running' || P.state !== 'playing') done();
    }
    ctxNow().addEventListener('statechange', () => { if (ctxNow() && ctxNow().state === 'running') done(); });
    evs.forEach((n) => document.addEventListener(n, fix, true));
  }
  const muted = () => P.state === 'playing' && !!ctxNow() && ctxNow().state === 'suspended';
  function skip(d) {
    if (!files()) return;
    P.idx = (P.idx + d + P.tracks.length) % P.tracks.length;
    if (P.audio) loadTrack(P.state === 'playing');
    else P.onTrack(P.tracks[P.idx]);
  }
  function play0() {
    if (P.state === 'playing') return;
    if (files()) {
      filePlayer();
      if (P.actx && P.actx.state === 'suspended') P.actx.resume().catch(() => {});
      unlockAudio();
      if (P.state === 'stopped' || !P.audio.src) loadTrack(true); else P.audio.play().catch(() => {});
      P.state = 'playing'; return;
    }
    if (P.state === 'paused') { P.ctx.resume(); P.state = 'playing'; return; }
    const AC = window.AudioContext || window.webkitAudioContext; if (!AC) return;
    P.ctx = new AC(); P.master = P.ctx.createGain(); P.master.gain.value = gainOf(P.vol); P.master.connect(P.ctx.destination);
    P.b = build(P.ctx, P.master); P.an = P.b.analyser; P.b.startRain();
    P.step = 0; P.next = P.t0 = P.ctx.currentTime + 0.1; pump(); P.timer = setInterval(pump, 60); P.state = 'playing';
    if (P.ctx.state === 'suspended') P.ctx.resume().catch(() => {});
    unlockAudio();
  }
  function pause0() {
    if (P.state !== 'playing') return;
    if (files()) P.audio.pause(); else P.ctx.suspend();
    P.state = 'paused';
  }
  function stop0() {
    if (P.state === 'stopped') return;
    if (files()) { P.audio.pause(); P.audio.currentTime = 0; P.state = 'stopped'; return; }
    clearInterval(P.timer); const ctx = P.ctx; P.state = 'stopped';
    try { P.master.gain.cancelScheduledValues(ctx.currentTime); P.master.gain.setTargetAtTime(0, ctx.currentTime, 0.05); } catch (_) { /* already closed */ }
    setTimeout(() => ctx.close().catch(() => {}), 400); P.ctx = P.b = P.master = P.an = null;
  }
  // what the visitor did last is remembered: playing music resumes at the first click or key press on the next page load (browsers only allow sound after one)
  const KEY_PLAY = 'photon.music.playing', KEY_POS = 'photon.music.pos';
  const store = (k, v) => { try { localStorage.setItem(k, v); } catch (_) { /* storage unavailable */ } };
  const fetchKey = (k) => { try { return localStorage.getItem(k) || ''; } catch (_) { return ''; } };
  const play = () => { play0(); if (P.state === 'playing') store(KEY_PLAY, '1'); };
  const pause = () => { pause0(); store(KEY_PLAY, '0'); };
  const stop = () => { stop0(); store(KEY_PLAY, '0'); store(KEY_POS, ''); };
  function armResume() {
    if (fetchKey(KEY_PLAY) !== '1' || P.state !== 'stopped') return;
    const pos = (() => { try { return JSON.parse(fetchKey(KEY_POS) || 'null'); } catch (_) { return null; } })();
    if (pos && files() && pos.i >= 0 && pos.i < P.tracks.length) { P.idx = pos.i; P.seek = pos.t || 0; P.onTrack(P.tracks[P.idx]); }
    P.armed = true;
    const evs = ['pointerdown', 'pointerup', 'keydown'];
    const go = (e) => {
      evs.forEach((n) => document.removeEventListener(n, go, true));
      P.armed = false;
      if (e.target && e.target.closest && e.target.closest('.music')) return; // the visitor uses the player itself
      if (P.state === 'stopped') play();
    };
    evs.forEach((n) => document.addEventListener(n, go, true));
  }

  function volume(v) {
    P.vol = Math.max(0, Math.min(1, v));
    if (P.audio) P.audio.volume = gainOf(P.vol);
    if (P.master) P.master.gain.setTargetAtTime(gainOf(P.vol), P.ctx.currentTime, 0.03);
    try { localStorage.setItem(KEY, String(P.vol)); } catch (_) { /* ignore */ }
  }
  const time = () => {
    if (files()) return P.audio ? P.audio.currentTime || 0 : 0;
    return P.ctx && P.state !== 'stopped' ? Math.max(0, P.ctx.currentTime - P.t0) % TOTAL : 0;
  };
  const length = () => (files() ? (P.audio && Number.isFinite(P.audio.duration) ? P.audio.duration : NaN) : TOTAL);

  /* ---- widget in the title bar ---- */
  const fmt = (s) => (Number.isFinite(s) ? `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(Math.floor(s % 60)).padStart(2, '0')}` : '--:--');
  function mount() {
    const bar = document.querySelector('.titlebar'); if (!bar || bar.querySelector('.music')) return;
    const w = document.createElement('span'); w.className = 'music';
    const mk = (cls, txt, title) => { const b = document.createElement('button'); b.type = 'button'; b.className = cls; b.textContent = txt; b.title = title; b.setAttribute('aria-label', title); w.append(b); return b; };
    const prev = mk('mbtn mnav', '⏮', 'previous track'), pp = mk('mbtn', '▶', 'play music'), st = mk('mbtn', '■', 'stop music'), nxt = mk('mbtn mnav', '⏭', 'next track');
    const eq = document.createElement('canvas'); eq.className = 'meq'; eq.width = 40; eq.height = 14; w.append(eq);
    const vol = document.createElement('input'); vol.type = 'range'; vol.min = 0; vol.max = 100; vol.value = Math.round(P.vol * 100); vol.className = 'mvol'; vol.title = 'volume'; vol.setAttribute('aria-label', 'music volume'); w.append(vol);
    const title = document.createElement('span'); title.className = 'mtitle'; title.tabIndex = 0; w.append(title);
    const tip = document.createElement('span'); tip.className = 'mtip'; w.append(tip);
    const tm = document.createElement('span'); tm.className = 'mtime'; w.append(tm);
    const clock = bar.querySelector('.clock'); bar.insertBefore(w, clock);
    const showTime = () => {
      if (P.armed && P.state === 'stopped') { tm.textContent = 'click anywhere to resume'; return; }
      if (muted()) { tm.textContent = 'tap to start sound'; return; }
      if (P.state === 'playing' && P.audio && P.audio.readyState < 3 && files()) { tm.textContent = 'loading...'; return; }
      tm.textContent = `${P.state === 'stopped' && !files() ? '--:--' : fmt(time())} / ${fmt(length())}`; };
    showTime();
    // the track and its credit: visible title, tooltip with the credit line, artist link and license code
    P.onTrack = (t) => {
      title.textContent = '♪ ' + t.title + (t.artist ? ' — ' + t.artist : '');
      tip.textContent = '';
      const line = (txt) => { const d = document.createElement('div'); d.textContent = txt; tip.append(d); return d; };
      line(t.credit || (t.artist ? `${t.title} by ${t.artist}` : t.title));
      if (t.url) { const d = document.createElement('div'), a = document.createElement('a'); a.href = t.url; a.target = '_blank'; a.rel = 'noopener noreferrer'; a.textContent = t.url; d.append(a); tip.append(d); }
      if (t.license) line('License code: ' + t.license);
      w.classList.toggle('credit', !!(t.credit || t.url || t.license));
    };
    const sync = () => { const on = P.state === 'playing' && !muted(); pp.textContent = on ? '❚❚' : '▶'; pp.title = on ? 'pause music' : muted() ? 'start sound' : 'play music'; w.classList.toggle('on', on); };
    pp.addEventListener('click', () => { if (muted()) { ctxNow().resume().catch(() => {}); if (P.audio && P.audio.paused) P.audio.play().catch(() => {}); } else if (P.state === 'playing') pause(); else play(); sync(); });
    st.addEventListener('click', () => { stop(); sync(); showTime(); });
    prev.addEventListener('click', () => skip(-1)); nxt.addEventListener('click', () => skip(1));
    vol.addEventListener('input', () => volume(vol.value / 100));
    fetch('/music/playlist.json', { cache: 'no-store' }).then((r) => (r.ok ? r.json() : null)).then((j) => {
      if (!j || !Array.isArray(j.tracks) || !j.tracks.length) return;
      P.tracks = j.tracks.filter((t) => t && typeof t.file === 'string'); if (!P.tracks.length) return;
      w.classList.add('files'); w.classList.toggle('multi', P.tracks.length > 1); P.onTrack(P.tracks[0]); pp.title = 'play music'; showTime();
    }).catch(() => { /* no playlist: the generated score is used */ }).finally(armResume);
    setInterval(() => { if (files() && P.state === 'playing' && P.audio) store(KEY_POS, JSON.stringify({ i: P.idx, t: Math.floor(P.audio.currentTime) })); }, 5000);
    const g = eq.getContext('2d'), data = new Uint8Array(32);
    const color = () => getComputedStyle(document.body).getPropertyValue('--c1').trim() || '#0ff';
    setInterval(() => {
      showTime(); sync();
      g.clearRect(0, 0, 40, 14);
      if (P.state === 'playing' && P.an) {
        P.an.getByteFrequencyData(data); g.fillStyle = color(); g.shadowColor = g.fillStyle; g.shadowBlur = 4;
        for (let i = 0; i < 8; i++) { const v = data[i * 2 + 1] / 255, h = Math.max(1, v * 13); g.fillRect(i * 5, 14 - h, 3, h); }
      }
    }, 66);
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', mount); else mount();

  return { play, pause, stop, volume, time, skip, total: TOTAL, state: () => P.state, tracks: () => P.tracks, _build: build, _STEP: STEP, _STEPS: STEPS };
})();
