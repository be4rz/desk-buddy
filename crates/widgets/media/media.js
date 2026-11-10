// The media widget (design ## widgets): a YouTube IFrame player served from the loopback widget
// origin. The app resolves a query to a video id (or several, for `next`) and sends `play`/`next`
// with `{videoId, title?}`; the page plays it and answers with its state. Error 101/150 (the owner
// blocks embedding) is answered as `{embedError}` and the app opens the video in the browser widget.
// `?fake=1` (debug builds only, the fake media switch) loads fake-yt.js, a local stand-in for the IFrame API.
// `?mock=1|empty|error` in a plain browser shows a static state for design parity.
(function () {
  "use strict";
  var q = new URLSearchParams(location.search);
  var $ = function (id) { return document.getElementById(id); };
  var root = $("w");
  var w = BuddyWidget.connect();
  var player = null, ready = null, current = null, lastError = null;
  var PLAYING = 1, PAUSED = 2, ENDED = 0, BUFFERING = 3;

  function view(v) { root.dataset.view = v; }

  function state() {
    if (!player || !current) return { playing: false };
    var s = typeof player.getPlayerState === "function" ? player.getPlayerState() : -1;
    var t = typeof player.getCurrentTime === "function" ? player.getCurrentTime() : 0;
    var d = typeof player.getDuration === "function" ? player.getDuration() : 0;
    var data = typeof player.getVideoData === "function" ? player.getVideoData() || {} : {};
    return {
      playing: s === PLAYING || s === BUFFERING,
      paused: s === PAUSED,
      ended: s === ENDED,
      title: data.title || current.title || "",
      videoId: current.videoId,
      source: "youtube",
      time: Math.round((t || 0) * 10) / 10,
      duration: Math.round((d || 0) * 10) / 10,
      hidden: document.hidden,
    };
  }

  function paint() {
    var s = state();
    if (current) {
      $("title").textContent = s.title || current.videoId;
      $("pp").textContent = s.playing ? "❚❚" : "▶";
      $("pp").setAttribute("aria-label", s.playing ? "pause" : "play");
      $("prog").style.width = s.duration > 0 ? Math.min(100, (100 * s.time) / s.duration) + "%" : "0";
    }
    return s;
  }

  var lastPushed = "";
  function push() {
    var s = paint();
    var key = JSON.stringify([s.playing, s.paused, s.title, s.videoId]);
    if (key !== lastPushed) { lastPushed = key; w.setState(s); }
  }

  function loadApi() {
    if (ready) return ready;
    ready = new Promise(function (resolve, reject) {
      window.onYouTubeIframeAPIReady = function () { resolve(); };
      var s = document.createElement("script");
      s.src = q.get("fake") === "1" ? "fake-yt.js" : "https://www.youtube.com/iframe_api";
      s.onerror = function () { ready = null; reject({ code: "media_offline", message: "couldn't load the YouTube player (offline?)" }); };
      document.head.appendChild(s);
    });
    return ready;
  }

  // Resolve on the first PLAYING, or reject on an error / timeout.
  var waiter = null;
  function waitPlaying(ms) {
    return new Promise(function (resolve, reject) {
      var t = setTimeout(function () { waiter = null; resolve(state()); }, ms);
      waiter = function (ok, err) { clearTimeout(t); waiter = null; ok ? resolve(state()) : reject(err); };
    });
  }

  function onState(e) {
    if (e.data === PLAYING && waiter) waiter(true);
    if (e.data === PLAYING) view("playing");
    push();
  }

  function onError(e) {
    var code = e && e.data;
    lastError = code;
    var blocked = code === 101 || code === 150;
    var msg = blocked ? "Its owner blocks embedding (error " + code + "). Opened it in the browser widget instead."
      : "The player failed (error " + code + ").";
    $("errtext").textContent = msg;
    view("error");
    if (waiter) waiter(false, { code: blocked ? "embed_blocked" : "player_error", message: msg, embedError: code, videoId: current && current.videoId });
    push();
  }

  function play(args) {
    var id = String(args.videoId || "");
    if (!/^[A-Za-z0-9_-]{11}$/.test(id)) return Promise.reject({ code: "usage", message: "no video to play" });
    current = { videoId: id, title: args.title || "" };
    lastError = null;
    $("title").textContent = current.title || id;
    $("sub").textContent = "YouTube · " + (args.by === "user" ? "requested by you" : "requested by the bear");
    return loadApi().then(function () {
      if (!player) {
        return new Promise(function (resolve) {
          player = new YT.Player("player", {
            width: "100%", height: "100%", videoId: id, host: "https://www.youtube.com",
            playerVars: { autoplay: 1, playsinline: 1, enablejsapi: 1, origin: location.origin, rel: 0 },
            events: { onReady: function () { player.playVideo(); resolve(); }, onStateChange: onState, onError: onError },
          });
        });
      }
      player.loadVideoById(id);
    }).then(function () {
      return waitPlaying(9000);
    }).then(function (s) {
      push();
      return { state: s };
    }, function (err) {
      if (err && err.embedError) return { state: { playing: false }, embedError: err.embedError, videoId: id, message: err.message };
      throw err;
    });
  }

  w.onAction("play", play);
  w.onAction("next", play);
  w.onAction("pause", function () {
    if (!player || !current) throw { code: "nothing_playing", message: "nothing is playing" };
    player.pauseVideo();
    return new Promise(function (r) { setTimeout(r, 300); }).then(function () { push(); return { state: state() }; });
  });
  w.onAction("resume", function () {
    if (!player || !current) throw { code: "nothing_playing", message: "nothing is playing" };
    player.playVideo();
    return waitPlaying(4000).then(function (s) { push(); return { state: s }; });
  });
  w.onAction("status", function () { return { state: paint() }; });

  $("hide").onclick = function () { w.requestHide(); };
  $("close").onclick = function () { w.requestClose(); };
  $("pp").onclick = function () {
    if (!player) return;
    state().playing ? player.pauseVideo() : player.playVideo();
  };
  $("next").onclick = function () { w.control("next").catch(function () {}); };
  $("pick").onclick = function () { current = null; view("empty"); push(); };
  setInterval(function () { if (current) paint(); }, 1000);
  setInterval(function () { if (current && state().playing) w.setState(state()); }, 5000);

  // design parity in a plain browser (no IPC): ?mock=1 | empty | error
  var mock = q.get("mock");
  if (mock && !w.ipc()) {
    if (mock === "empty") view("empty");
    else if (mock === "error") view("error");
    else {
      $("player").innerHTML = '<div class="fake">YouTube IFrame player<br>' + location.origin + "/media/</div>";
      $("title").textContent = "lofi beats to study to";
      $("prog").style.width = "38%";
      view("playing");
    }
  }
})();
