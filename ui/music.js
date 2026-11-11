'use strict';
// Optional background music of the pool pages: audio files from the server's music folder (see /music/playlist.json), streamed by an <audio> element and
// looped as a playlist. Without files there is no player at all.
const Music = (() => {
  const KEY = 'photon.music.volume.v2', KEY_PLAY = 'photon.music.playing', KEY_POS = 'photon.music.pos';
  const P = { state: 'stopped', vol: 0.28, tracks: [], idx: 0, seek: 0, armed: false, audio: null, an: null, actx: null, unlocking: false, onTrack: () => {} };
  try { const v = parseFloat(localStorage.getItem(KEY)); if (v >= 0 && v <= 1) P.vol = v; } catch (_) { /* storage unavailable */ }
  const gainOf = (v) => v * v;
  const store = (k, v) => { try { localStorage.setItem(k, v); } catch (_) { /* storage unavailable */ } };
  const fetchKey = (k) => { try { return localStorage.getItem(k) || ''; } catch (_) { return ''; } };

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
  function unlockAudio() {
    if (!P.actx || P.unlocking) return;
    P.unlocking = true;
    const evs = ['pointerup', 'touchend', 'click', 'keydown'];
    const done = () => { evs.forEach((n) => document.removeEventListener(n, fix, true)); P.unlocking = false; };
    function fix() {
      if (P.actx.state === 'suspended' && P.state === 'playing') P.actx.resume().catch(() => {});
      if (P.state === 'playing' && P.audio && P.audio.paused) P.audio.play().catch(() => {});
      if (P.actx.state === 'running' || P.state !== 'playing') done();
    }
    P.actx.addEventListener('statechange', () => { if (P.actx.state === 'running') done(); });
    evs.forEach((n) => document.addEventListener(n, fix, true));
  }
  const muted = () => P.state === 'playing' && !!P.actx && P.actx.state === 'suspended';
  function skip(d) {
    if (!P.tracks.length) return;
    P.idx = (P.idx + d + P.tracks.length) % P.tracks.length;
    if (P.audio) loadTrack(P.state === 'playing'); else P.onTrack(P.tracks[P.idx]);
  }
  function play0() {
    if (P.state === 'playing' || !P.tracks.length) return;
    filePlayer();
    if (P.actx && P.actx.state === 'suspended') P.actx.resume().catch(() => {});
    unlockAudio();
    if (P.state === 'stopped' || !P.audio.src) loadTrack(true); else P.audio.play().catch(() => {});
    P.state = 'playing';
  }
  function pause0() { if (P.state === 'playing') { P.audio.pause(); P.state = 'paused'; } }
  function stop0() { if (P.state !== 'stopped') { P.audio.pause(); P.audio.currentTime = 0; P.state = 'stopped'; } }
  // what the visitor did last is remembered: playing music resumes at the first click or key press on the next page load (browsers only allow sound after one)
  const play = () => { play0(); if (P.state === 'playing') store(KEY_PLAY, '1'); };
  const pause = () => { pause0(); store(KEY_PLAY, '0'); };
  const stop = () => { stop0(); store(KEY_PLAY, '0'); store(KEY_POS, ''); };
  function armResume() {
    if (fetchKey(KEY_PLAY) !== '1' || P.state !== 'stopped') return;
    const pos = (() => { try { return JSON.parse(fetchKey(KEY_POS) || 'null'); } catch (_) { return null; } })();
    if (pos && pos.i >= 0 && pos.i < P.tracks.length) { P.idx = pos.i; P.seek = pos.t || 0; P.onTrack(P.tracks[P.idx]); }
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
    store(KEY, String(P.vol));
  }
  const time = () => (P.audio ? P.audio.currentTime || 0 : 0);
  const length = () => (P.audio && Number.isFinite(P.audio.duration) ? P.audio.duration : NaN);

  /* ---- widget in the title bar, only when there is something to play ---- */
  const fmt = (s) => (Number.isFinite(s) ? `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(Math.floor(s % 60)).padStart(2, '0')}` : '--:--');
  function mount(tracks) {
    const bar = document.querySelector('.titlebar'); if (!bar || bar.querySelector('.music')) return;
    P.tracks = tracks;
    const w = document.createElement('span'); w.className = 'music files'; w.classList.toggle('multi', tracks.length > 1);
    const mk = (cls, txt, title) => { const b = document.createElement('button'); b.type = 'button'; b.className = cls; b.textContent = txt; b.title = title; b.setAttribute('aria-label', title); w.append(b); return b; };
    const prev = mk('mbtn mnav', '⏮', 'previous track'), pp = mk('mbtn', '▶', 'play music'), st = mk('mbtn', '■', 'stop music'), nxt = mk('mbtn mnav', '⏭', 'next track');
    const eq = document.createElement('canvas'); eq.className = 'meq'; eq.width = 40; eq.height = 14; w.append(eq);
    const vol = document.createElement('input'); vol.type = 'range'; vol.min = 0; vol.max = 100; vol.value = Math.round(P.vol * 100); vol.className = 'mvol'; vol.title = 'volume'; vol.setAttribute('aria-label', 'music volume'); w.append(vol);
    const title = document.createElement('span'); title.className = 'mtitle'; title.tabIndex = 0; w.append(title);
    const tip = document.createElement('span'); tip.className = 'mtip'; w.append(tip);
    const tm = document.createElement('span'); tm.className = 'mtime'; w.append(tm);
    bar.insertBefore(w, bar.querySelector('.clock'));
    const showTime = () => {
      if (P.armed && P.state === 'stopped') { tm.textContent = 'click anywhere to resume'; return; }
      if (muted()) { tm.textContent = 'tap to start sound'; return; }
      if (P.state === 'playing' && P.audio && P.audio.readyState < 3) { tm.textContent = 'loading...'; return; }
      tm.textContent = `${fmt(time())} / ${fmt(length())}`;
    };
    // the track and its credit: visible title, tooltip with the credit line, artist link and license code
    P.onTrack = (t) => {
      title.textContent = '♪ ' + t.title + (t.artist ? ' - ' + t.artist : '');
      tip.textContent = '';
      const line = (txt) => { const d = document.createElement('div'); d.textContent = txt; tip.append(d); return d; };
      line(t.credit || (t.artist ? `${t.title} by ${t.artist}` : t.title));
      if (t.url) { const d = document.createElement('div'), a = document.createElement('a'); a.href = t.url; a.target = '_blank'; a.rel = 'noopener noreferrer'; a.textContent = t.url; d.append(a); tip.append(d); }
      if (t.license) line('License code: ' + t.license);
      w.classList.toggle('credit', !!(t.credit || t.url || t.license));
    };
    P.onTrack(tracks[0]);
    const sync = () => { const on = P.state === 'playing' && !muted(); pp.textContent = on ? '❚❚' : '▶'; pp.title = on ? 'pause music' : muted() ? 'start sound' : 'play music'; w.classList.toggle('on', on); };
    pp.addEventListener('click', () => { if (muted()) { P.actx.resume().catch(() => {}); if (P.audio && P.audio.paused) P.audio.play().catch(() => {}); } else if (P.state === 'playing') pause(); else play(); sync(); });
    st.addEventListener('click', () => { stop(); sync(); showTime(); });
    prev.addEventListener('click', () => skip(-1)); nxt.addEventListener('click', () => skip(1));
    vol.addEventListener('input', () => volume(vol.value / 100));
    showTime(); armResume();
    setInterval(() => { if (P.state === 'playing' && P.audio) store(KEY_POS, JSON.stringify({ i: P.idx, t: Math.floor(P.audio.currentTime) })); }, 5000);
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
  const init = () => fetch('/music/playlist.json', { cache: 'no-store' }).then((r) => (r.ok ? r.json() : null)).then((j) => {
    const tracks = j && Array.isArray(j.tracks) ? j.tracks.filter((t) => t && typeof t.file === 'string') : [];
    if (tracks.length) mount(tracks);
  }).catch(() => { /* no playlist: no player */ });
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init); else init();

  return { play, pause, stop, volume, time, skip, state: () => P.state, tracks: () => P.tracks };
})();
