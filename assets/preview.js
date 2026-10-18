// Web Audio preview/A-B engine for AudioAnalysisModal. It plays by
// decode-then-play (AudioBuffer + AudioBufferSourceNode), NOT an <audio>
// element tapped through
// createMediaElementSource - that combination reliably failed with
// `NotSupportedError` on WebKitGTK even when the file and its Content-Type
// were correct, pointing at a gap in MediaElementAudioSourceNode support.
// Decoding fully avoids that API; the tradeoff is no native transport, so
// play/pause/seek are hand-rolled on AudioContext's own clock.
//
// Rust (src/ui/components/audio_analysis_modal.rs) drives this through
// `tqPreview.cmd(name, args)` and receives events through the emitter it
// installs with `tqPreview.setEmitter(fn)`:
//   {ev:"loaded", duration} | {ev:"error", message} | {ev:"time", t}
//   {ev:"state", playing, t} | {ev:"ended", duration}
//
// Graph:  source -> entry -> [peaking filters] -> preamp -> volume -> out
// `entry` is permanent so sources don't care whether any filters exist
// between it and the preamp (an already-balanced track can legitimately
// be suggested zero bands).
(function () {
  const dbToGain = (db) => Math.pow(10, db / 20);

  let emit = () => {};

  const S = {
    ctx: null,
    buffer: null,
    source: null,
    entry: null,
    filters: [],
    preamp: null,
    volume: null,
    eq: { bands: [], preampDb: 0, enabled: true },
    volumeGain: 1,
    startCtxTime: 0, // ctx.currentTime that corresponds to track position 0
    pausedAt: 0,
    playing: false,
    timer: null,
    loadToken: 0,
  };

  function ensureCtx() {
    if (!S.ctx) {
      S.ctx = new (window.AudioContext || window.webkitAudioContext)();
      buildGraph();
    }
    return S.ctx;
  }

  function buildGraph() {
    const c = S.ctx;
    S.entry = c.createGain();
    S.preamp = c.createGain();
    S.volume = c.createGain();
    S.volume.gain.value = S.volumeGain;
    S.preamp.connect(S.volume);
    S.volume.connect(c.destination);
    rebuildFilters();
  }

  // One peaking BiquadFilterNode per band; each band's own Q (bandwidth)
  // comes straight from the analysis - narrower for an isolated standout
  // frequency, wider for one spanning several adjacent points.
  function rebuildFilters() {
    const c = S.ctx;
    if (!c) return;
    S.entry.disconnect();
    S.filters.forEach((f) => f.disconnect());
    const fs = S.eq.bands.map((b) => {
      const f = c.createBiquadFilter();
      f.type = "peaking";
      f.frequency.value = b.freq;
      f.Q.value = b.q;
      return f;
    });
    if (fs.length) {
      S.entry.connect(fs[0]);
      for (let i = 0; i < fs.length - 1; i++) fs[i].connect(fs[i + 1]);
      fs[fs.length - 1].connect(S.preamp);
    } else {
      S.entry.connect(S.preamp);
    }
    S.filters = fs;
    applyEqValues();
  }

  // Live-toggling the EQ for A/B comparison only touches gains - no rebuild.
  function applyEqValues() {
    const { bands, preampDb, enabled } = S.eq;
    S.filters.forEach((f, i) => {
      f.gain.value = enabled ? bands[i].gain : 0;
    });
    if (S.preamp) S.preamp.gain.value = enabled ? dbToGain(preampDb) : 1;
  }

  function position() {
    if (!S.ctx || !S.buffer || !S.playing) return S.pausedAt;
    return Math.min(S.ctx.currentTime - S.startCtxTime, S.buffer.duration);
  }

  function startTimer() {
    stopTimer();
    S.timer = setInterval(() => {
      if (S.playing) emit({ ev: "time", t: position() });
    }, 50);
  }
  function stopTimer() {
    if (S.timer) clearInterval(S.timer);
    S.timer = null;
  }

  function stopSource() {
    if (S.source) {
      S.source.onended = null;
      try {
        S.source.stop();
      } catch (_) {}
      S.source = null;
    }
  }

  // AudioBufferSourceNode is single-use (no native pause/resume), so every
  // play/resume/seek makes a new one against the same decoded buffer and
  // the same persistent filter chain.
  function playFrom(offset) {
    const c = ensureCtx();
    if (c.state === "suspended") c.resume();
    if (!S.buffer) return;
    stopSource();
    const buffer = S.buffer;
    const source = c.createBufferSource();
    source.buffer = buffer;
    source.connect(S.entry);
    source.onended = () => {
      // A rapid seek/pause/replay can stop this exact node and start a
      // newer one before this (async) event fires - only the node that's
      // still current when its own "ended" arrives is a genuine
      // end-of-track; a superseded one firing late is ignored. (A shared
      // "was this manually stopped" flag doesn't survive quick back-to-back
      // stop/restart cycles.)
      if (S.source !== source) return;
      stopTimer();
      S.source = null;
      S.playing = false;
      S.pausedAt = buffer.duration;
      emit({ ev: "ended", duration: buffer.duration });
    };
    source.start(0, offset);
    S.source = source;
    S.startCtxTime = c.currentTime - offset;
    S.playing = true;
    S.pausedAt = offset;
    startTimer();
    emit({ ev: "state", playing: true, t: offset });
  }

  function pause() {
    if (!S.playing || !S.source) return;
    const t = position();
    stopSource();
    stopTimer();
    S.playing = false;
    S.pausedAt = t;
    emit({ ev: "state", playing: false, t });
  }

  async function load(url) {
    const token = ++S.loadToken;
    try {
      const resp = await fetch(url);
      if (!resp.ok) throw new Error("couldn't load the audio file (HTTP " + resp.status + ")");
      const bytes = await resp.arrayBuffer();
      const decoded = await ensureCtx().decodeAudioData(bytes);
      if (token !== S.loadToken) return; // closed/reloaded while decoding
      S.buffer = decoded;
      S.pausedAt = 0;
      emit({ ev: "loaded", duration: decoded.duration });
    } catch (e) {
      if (token !== S.loadToken) return;
      emit({ ev: "error", message: String((e && e.message) || e) });
    }
  }

  function close() {
    S.loadToken++;
    stopTimer();
    stopSource();
    if (S.ctx) {
      try {
        S.ctx.close();
      } catch (_) {}
    }
    S.ctx = null;
    S.buffer = null;
    S.entry = S.preamp = S.volume = null;
    S.filters = [];
    S.playing = false;
    S.pausedAt = 0;
  }

  const handlers = {
    // Reset for a fresh modal: drops any previous session.
    open() {
      close();
    },
    // args: {bands:[{freq,gain,q}], preampDb, enabled}. A changed band
    // count/frequency rebuilds the chain; otherwise only gains update.
    setEq(a) {
      const prev = S.eq.bands;
      S.eq = a;
      if (!S.ctx) return;
      const same = prev.length === a.bands.length && prev.every((b, i) => b.freq === a.bands[i].freq && b.q === a.bands[i].q);
      if (same) applyEqValues();
      else rebuildFilters();
    },
    // args: {gain} - the same squared master volume the main engine uses.
    setVolume(a) {
      S.volumeGain = a.gain;
      if (S.volume && S.ctx) S.volume.gain.setTargetAtTime(a.gain, S.ctx.currentTime, 0.015);
    },
    load(a) {
      return load(a.url);
    },
    play(a) {
      playFrom(a.offset || 0);
    },
    pause,
    seek(a) {
      S.pausedAt = a.t;
      if (S.playing) playFrom(a.t);
    },
    close,
  };

  window.tqPreview = {
    setEmitter(fn) {
      emit = fn;
    },
    cmd(name, args) {
      const h = handlers[name];
      if (h) return h(args || {});
    },
  };
})();
