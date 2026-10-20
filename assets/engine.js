// Web Audio playback engine. The Rust side (src/ui/player.rs) owns the
// queue / shuffle / repeat / EQ state and drives this through
// `tqEngine.cmd(...)`; this file owns only what has to live in the webview -
// the AudioContext graph, decoded buffers and gapless scheduling - and
// reports back through the emitter installed with `tqEngine.setEmitter`.
//
// Graph:  source -> entry -> [correction filters -> correction preamp] ->
//         eqEntry -> [tone-EQ filters] -> preamp -> normGain -> volume -> out
// `entry`, `eqEntry`, `preamp` are permanent so either filter chain can be
// rebuilt mid-playback without touching a live source's connection.
(function () {
  const WAVEFORM_BARS = 100;
  const dbToGain = (db) => Math.pow(10, db / 20);

  // Rust installs this (see `Player` in ui/player.rs) to receive events.
  let emit = () => {};

  const S = {
    ctx: null,
    entry: null,
    eqEntry: null,
    preamp: null,
    norm: null,
    volume: null,
    eqFilters: [],
    corrFilters: [],
    corrPreamp: null,
    eq: { bands: [], preampDb: 0, enabled: true },
    corr: null, // { bands:[{type,freq,gain,q}], preampDb }
    corrEnabled: true,
    volumeGain: 1,
    duck: 1, // 0-1 multiplier while the system asks us to be quiet (Android audio focus)
    // current track
    buffer: null,
    source: null,
    startCtxTime: 0,
    pausedAt: 0,
    playing: false,
    seeking: false,
    // load bookkeeping
    pending: new Map(), // token -> {buffer, normDb}
    // gapless lookahead
    next: null, // {key, buffer, normDb}
    nextWanted: null, // {key, url, normDb}
    nextToken: 0,
    scheduled: null, // {source, timeout, key}
    timer: null,
  };

  function ctx() {
    if (!S.ctx) {
      S.ctx = new (window.AudioContext || window.webkitAudioContext)();
      // A hidden webview may suspend an idle context; a track that starts in a suspended one is silent.
      S.ctx.onstatechange = () => {
        if (S.ctx.state === "suspended" && S.playing) S.ctx.resume().catch(() => {});
      };
      buildGraph();
      startMeter();
    }
    return S.ctx;
  }

  function buildGraph() {
    const c = S.ctx;
    S.entry = c.createGain();
    S.eqEntry = c.createGain();
    S.preamp = c.createGain();
    S.norm = c.createGain();
    S.volume = c.createGain();
    S.volume.gain.value = S.volumeGain * S.duck;
    S.preamp.connect(S.norm);
    S.norm.connect(S.volume);
    // Meter tap: a dead-end branch, read-only, ahead of the volume fader so
    // the bars show the signal, not the fader position.
    S.analyser = c.createAnalyser();
    S.analyser.fftSize = 2048;
    S.analyser.smoothingTimeConstant = 0.6;
    S.norm.connect(S.analyser);
    S.volume.connect(c.destination);
    rebuildEq();
    rebuildCorrection();
  }

  function rebuildEq() {
    const c = S.ctx;
    if (!c) return;
    S.eqEntry.disconnect();
    S.eqFilters.forEach((f) => f.disconnect());
    const { bands, enabled } = S.eq;
    if (!bands.length) {
      S.eqFilters = [];
      S.eqEntry.connect(S.preamp);
    } else {
      const fs = bands.map((b) => {
        const f = c.createBiquadFilter();
        f.type = "peaking";
        f.frequency.value = b.freq;
        f.Q.value = b.q;
        f.gain.value = enabled ? b.gain : 0;
        return f;
      });
      S.eqEntry.connect(fs[0]);
      for (let i = 0; i < fs.length - 1; i++) fs[i].connect(fs[i + 1]);
      fs[fs.length - 1].connect(S.preamp);
      S.eqFilters = fs;
    }
    applyEqValues();
  }

  function applyEqValues() {
    const { bands, preampDb, enabled } = S.eq;
    S.eqFilters.forEach((f, i) => {
      const b = bands[i];
      if (!b) return;
      f.frequency.value = b.freq;
      f.Q.value = b.q;
      f.gain.value = enabled ? b.gain : 0;
    });
    if (S.preamp) S.preamp.gain.value = enabled ? dbToGain(preampDb) : 1;
  }

  function rebuildCorrection() {
    const c = S.ctx;
    if (!c) return;
    S.entry.disconnect();
    S.corrFilters.forEach((f) => f.disconnect());
    if (S.corrPreamp) S.corrPreamp.disconnect();
    const p = S.corr;
    if (!p || !p.bands.length) {
      S.corrFilters = [];
      S.corrPreamp = null;
      S.entry.connect(S.eqEntry);
      return;
    }
    const fs = p.bands.map((b) => {
      const f = c.createBiquadFilter();
      f.type = b.type;
      f.frequency.value = b.freq;
      f.Q.value = b.q;
      f.gain.value = S.corrEnabled ? b.gain : 0;
      return f;
    });
    const pre = c.createGain();
    pre.gain.value = S.corrEnabled ? dbToGain(p.preampDb) : 1;
    S.entry.connect(fs[0]);
    for (let i = 0; i < fs.length - 1; i++) fs[i].connect(fs[i + 1]);
    fs[fs.length - 1].connect(pre);
    pre.connect(S.eqEntry);
    S.corrFilters = fs;
    S.corrPreamp = pre;
  }

  function peaks(buffer) {
    const data = buffer.getChannelData(0);
    const per = Math.max(1, Math.floor(data.length / WAVEFORM_BARS));
    const out = [];
    for (let bar = 0; bar < WAVEFORM_BARS; bar++) {
      const start = bar * per;
      const end = Math.min(start + per, data.length);
      let max = 0;
      for (let i = start; i < end; i++) {
        const a = Math.abs(data[i]);
        if (a > max) max = a;
      }
      out.push(max);
    }
    const loudest = Math.max(...out, 0.0001);
    return out.map((p) => p / loudest);
  }

  async function fetchDecode(url) {
    const resp = await fetch(url);
    if (!resp.ok) throw new Error("couldn't load the audio file (HTTP " + resp.status + ")");
    const bytes = await resp.arrayBuffer();
    return await ctx().decodeAudioData(bytes);
  }

  function setNorm(db) {
    if (!S.norm) return;
    S.norm.gain.value = db == null ? 1 : dbToGain(db);
  }

  function position() {
    if (!S.ctx || !S.buffer) return 0;
    if (!S.playing) return S.pausedAt;
    return Math.min(S.ctx.currentTime - S.startCtxTime, S.buffer.duration);
  }

  function startTimer() {
    stopTimer();
    S.timer = setInterval(() => {
      if (S.playing && !S.seeking) emit({ ev: "time", t: position() });
    }, 250);
  }
  function stopTimer() {
    if (S.timer) clearInterval(S.timer);
    S.timer = null;
  }

  function cancelScheduled() {
    if (!S.scheduled) return;
    clearTimeout(S.scheduled.timeout);
    const stray = S.scheduled.source;
    stray.onended = null;
    try {
      stray.stop();
    } catch (_) {}
    S.scheduled = null;
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

  function playFrom(offset) {
    const c = ctx();
    if (c.state === "suspended") c.resume();
    if (!S.buffer) return;
    cancelScheduled();
    stopSource();
    const source = c.createBufferSource();
    source.buffer = S.buffer;
    source.connect(S.entry);
    source.onended = () => {
      if (S.source !== source) return;
      if (S.scheduled) {
        // Gapless handover. Don't wait for the timeout: a minimised webview throttles it.
        const sch = S.scheduled;
        clearTimeout(sch.timeout);
        promote(sch.endCtx);
        return;
      }
      stopTimer();
      S.playing = false;
      emit({ ev: "ended" });
    };
    source.start(0, offset);
    S.source = source;
    S.startCtxTime = c.currentTime - offset;
    S.playing = true;
    startTimer();
    maybeArmGapless();
  }

  function maybeArmGapless() {
    const c = S.ctx;
    const n = S.next;
    if (!c || !S.buffer || !n || !S.playing || S.scheduled) return;
    if (!S.nextWanted || S.nextWanted.key !== n.key) return;
    const source = c.createBufferSource();
    source.buffer = n.buffer;
    source.connect(S.entry);
    const endCtx = S.startCtxTime + S.buffer.duration;
    source.start(Math.max(endCtx, c.currentTime));
    const delay = Math.max(0, (endCtx - c.currentTime) * 1000);
    const timeout = setTimeout(() => promote(endCtx), delay);
    S.scheduled = { source, timeout, key: n.key, endCtx };
  }

  function promote(startCtxTime) {
    const sch = S.scheduled;
    const n = S.next;
    if (!sch || !n) return;
    S.buffer = n.buffer;
    S.source = sch.source;
    S.scheduled = null;
    S.next = null;
    S.startCtxTime = startCtxTime;
    S.playing = true;
    startTimer();
    setNorm(n.normDb);
    const source = sch.source;
    source.onended = () => {
      if (S.source !== source) return;
      if (S.scheduled) return;
      stopTimer();
      S.playing = false;
      emit({ ev: "ended" });
    };
    emit({ ev: "promoted", key: n.key, duration: n.buffer.duration, waveform: peaks(n.buffer) });
  }

  async function prefetch(want) {
    // Same track re-requested without a URL: just refresh norm / re-arm.
    if (want && !want.url && S.nextWanted && S.nextWanted.key === want.key) {
      S.nextWanted.normDb = want.normDb;
      if (S.next) S.next.normDb = want.normDb;
      maybeArmGapless();
      return;
    }
    const token = ++S.nextToken;
    S.nextWanted = want;
    if (!want) {
      S.next = null;
      cancelScheduled();
      return;
    }
    if (S.next && S.next.key === want.key) {
      S.next.normDb = want.normDb;
      maybeArmGapless();
      return;
    }
    S.next = null;
    cancelScheduled();
    try {
      const buffer = await fetchDecode(want.url);
      if (token !== S.nextToken) return;
      S.next = { key: want.key, buffer, normDb: want.normDb };
      maybeArmGapless();
    } catch (_) {
      // not fatal: that transition just won't be gapless
    }
  }

  const handlers = {
    async load({ token, url }) {
      try {
        const buffer = await fetchDecode(url);
        S.pending.set(token, buffer);
        emit({ ev: "loaded", token, duration: buffer.duration, waveform: peaks(buffer) });
      } catch (e) {
        emit({ ev: "loadError", token, message: String(e && e.message ? e.message : e) });
      }
    },
    start({ token, normDb, offset }) {
      const buffer = S.pending.get(token);
      S.pending.clear();
      if (!buffer) return;
      S.buffer = buffer;
      S.pausedAt = 0;
      setNorm(normDb);
      playFrom(offset || 0);
    },
    // Reuse the already-decoded lookahead buffer (manual skip to the track
    // that was prefetched).
    startNext({ key, normDb }) {
      const n = S.next;
      if (!n || n.key !== key) {
        emit({ ev: "nextMiss", key });
        return;
      }
      S.next = null;
      S.buffer = n.buffer;
      S.pausedAt = 0;
      setNorm(normDb);
      playFrom(0);
      emit({ ev: "promoted", key, duration: n.buffer.duration, waveform: peaks(n.buffer), manual: true });
    },
    pause() {
      if (!S.playing) return;
      cancelScheduled();
      S.pausedAt = position();
      stopSource();
      S.playing = false;
      stopTimer();
      emit({ ev: "time", t: S.pausedAt });
    },
    resume() {
      if (!S.buffer || S.playing) return;
      const at = S.pausedAt >= S.buffer.duration ? 0 : S.pausedAt;
      playFrom(at);
    },
    seek({ t }) {
      S.pausedAt = t;
      if (S.playing) playFrom(t);
    },
    seeking({ on }) {
      S.seeking = !!on;
    },
    stop() {
      S.pending.clear();
      cancelScheduled();
      stopSource();
      stopTimer();
      S.playing = false;
      S.buffer = null;
      S.next = null;
      S.nextWanted = null;
      ++S.nextToken;
      S.pausedAt = 0;
    },
    setNext(want) {
      return prefetch(want);
    },
    setNorm({ db }) {
      setNorm(db);
    },
    setVolume({ gain }) {
      S.volumeGain = gain;
      if (S.volume && S.ctx) S.volume.gain.setTargetAtTime(gain * S.duck, S.ctx.currentTime, 0.015);
    },
    // structure = true when the band count changed (needs a rebuild)
    setEq({ bands, preampDb, enabled, structure }) {
      S.eq = { bands, preampDb, enabled };
      if (!S.ctx) return;
      if (structure || S.eqFilters.length !== bands.length) rebuildEq();
      else applyEqValues();
    },
    setCorrection({ profile, enabled, structure }) {
      S.corr = profile;
      S.corrEnabled = enabled;
      if (!S.ctx) return;
      if (structure) {
        rebuildCorrection();
      } else {
        S.corrFilters.forEach((f, i) => {
          f.gain.value = enabled ? (profile.bands[i] ? profile.bands[i].gain : 0) : 0;
        });
        if (S.corrPreamp) S.corrPreamp.gain.value = enabled && profile ? dbToGain(profile.preampDb) : 1;
      }
    },
  };

  // Spectrum / VU meter. Drawn here, straight to the `#tq-meter` canvas the
  // player bar renders, so no per-frame traffic crosses to Rust. The canvas
  // comes and goes with the bar, so it is looked up each frame.
  const METER_BARS = 20;
  const meter = { raf: 0, bins: null, peaks: new Float32Array(METER_BARS), levels: new Float32Array(METER_BARS) };

  function meterFrame() {
    meter.raf = requestAnimationFrame(meterFrame);
    const canvas = document.getElementById("tq-meter");
    if (!canvas || !S.analyser) return;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.round(canvas.clientWidth * dpr);
    const h = Math.round(canvas.clientHeight * dpr);
    if (!w || !h) return;
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    const an = S.analyser;
    const data = new Uint8Array(an.frequencyBinCount);
    if (S.playing) an.getByteFrequencyData(data);
    if (!meter.bins) {
      // Log-spaced bands, 40 Hz .. 16 kHz.
      const hzPerBin = S.ctx.sampleRate / an.fftSize;
      meter.bins = [];
      for (let i = 0; i <= METER_BARS; i++) {
        const hz = 40 * Math.pow(16000 / 40, i / METER_BARS);
        meter.bins.push(Math.min(data.length - 1, Math.max(1, Math.round(hz / hzPerBin))));
      }
    }
    const g = canvas.getContext("2d");
    g.clearRect(0, 0, w, h);
    const slot = w / METER_BARS;
    const segH = Math.max(2, Math.round(2.5 * dpr));
    const gap = Math.max(1, Math.round(dpr));
    const segs = Math.floor(h / (segH + gap));
    for (let i = 0; i < METER_BARS; i++) {
      const lo = meter.bins[i];
      const hi = Math.max(lo + 1, meter.bins[i + 1]);
      let sum = 0;
      for (let k = lo; k < hi; k++) sum += data[k];
      // Pink-ish tilt so the highs aren't permanently dark.
      const v = Math.min(1, (sum / (hi - lo) / 255) * (1 + i / METER_BARS * 0.6));
      // Fast attack, gentle fall.
      meter.levels[i] = v > meter.levels[i] ? v : meter.levels[i] * 0.88 + v * 0.12;
      if (meter.levels[i] >= meter.peaks[i]) meter.peaks[i] = meter.levels[i];
      else meter.peaks[i] = Math.max(0, meter.peaks[i] - 0.012);
      const lit = Math.round(meter.levels[i] * segs);
      const peakSeg = Math.min(segs - 1, Math.round(meter.peaks[i] * segs));
      const x = Math.round(i * slot + gap);
      const bw = Math.max(1, Math.round(slot - gap * 2));
      for (let s = 0; s < segs; s++) {
        const y = h - (s + 1) * (segH + gap) + gap;
        const on = s < lit;
        g.fillStyle = on ? (s > segs * 0.85 ? "#ffffff" : "#c9c9cc") : s === peakSeg && meter.peaks[i] > 0.02 ? "#f1f1f2" : "#1c1c1f";
        g.fillRect(x, y, bw, segH);
      }
    }
  }
  function startMeter() {
    if (!meter.raf) meter.raf = requestAnimationFrame(meterFrame);
  }

  window.tqEngine = {
    cmd(name, args) {
      const h = handlers[name];
      if (h) return h(args || {});
    },
    setEmitter(fn) {
      emit = fn;
    },
    position,
    // Android audio focus: another app wants to talk over us (a notification). Quieter, not paused.
    duck(factor) {
      S.duck = factor;
      if (S.volume && S.ctx) S.volume.gain.setTargetAtTime(S.volumeGain * factor, S.ctx.currentTime, 0.1);
    },
  };

  // Where the Rust-side protocol handler lives (see main.rs). WebKitGTK uses
  // `tq://localhost/...`; WebView2 rewrites custom schemes to http://tq.localhost, Android WebView to https://tq.localhost.
  window.tqUrl = function (path) {
    const ua = navigator.userAgent;
    // Android's page is https (dioxus.index.html), so its protocol must be https too, or the fetch is blocked as mixed content.
    return (/Android/.test(ua) ? "https://tq.localhost" : /Windows/.test(ua) ? "http://tq.localhost" : "tq://localhost") + path;
  };
})();
