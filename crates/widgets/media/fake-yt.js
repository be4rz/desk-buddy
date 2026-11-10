// A local stand-in for the YouTube IFrame API (debug builds' fake media switch): the same
// YT.Player surface media.js uses, backed by a real looping <audio> element (a generated, silent
// WAV) so "keeps playing while hidden" is measured on real media time, not a JS timer.
// A video id starting with "blocked" fails with error 150 (the owner blocks embedding).
(function () {
  "use strict";
  function silentWav(seconds) {
    var rate = 8000, n = rate * seconds, buf = new ArrayBuffer(44 + n), v = new DataView(buf);
    function s(o, t) { for (var i = 0; i < t.length; i++) v.setUint8(o + i, t.charCodeAt(i)); }
    s(0, "RIFF"); v.setUint32(4, 36 + n, true); s(8, "WAVE"); s(12, "fmt "); v.setUint32(16, 16, true);
    v.setUint16(20, 1, true); v.setUint16(22, 1, true); v.setUint32(24, rate, true); v.setUint32(28, rate, true);
    v.setUint16(32, 1, true); v.setUint16(34, 8, true); s(36, "data"); v.setUint32(40, n, true);
    for (var i = 0; i < n; i++) v.setUint8(44 + i, 128);
    return URL.createObjectURL(new Blob([buf], { type: "audio/wav" }));
  }
  var SRC = silentWav(10);
  var DURATION = 180;

  function Player(el, opts) {
    var self = this;
    var host = typeof el === "string" ? document.getElementById(el) : el;
    this._ev = (opts && opts.events) || {};
    this._state = -1;
    this._id = null;
    this._elapsed = 0;
    this._mark = 0;
    this._audio = document.createElement("audio");
    this._audio.src = SRC;
    this._audio.loop = true;
    this._audio.volume = 1; // the WAV is silent; a muted element would be paused when invisible
    this._audio.addEventListener("timeupdate", function () { self._tick(); });
    host.innerHTML = '<div class="fake" id="fakebox">test video</div>';
    host.appendChild(this._audio);
    this._box = host.querySelector("#fakebox");
    setTimeout(function () {
      self._fire("onReady", {});
      if (opts && opts.videoId) self.loadVideoById(opts.videoId);
    }, 50);
  }
  Player.prototype._fire = function (name, data) { var f = this._ev[name]; if (f) f(data); };
  Player.prototype._set = function (s) { this._state = s; this._fire("onStateChange", { data: s }); };
  Player.prototype._tick = function () {
    var t = this._audio.currentTime;
    var d = t - this._mark;
    if (d < 0) d += this._audio.duration || 10; // the loop wrapped
    if (this._state === 1 && d < 9) this._elapsed += d;
    this._mark = t;
  };
  Player.prototype.loadVideoById = function (id) {
    var self = this;
    this._id = String(id);
    this._elapsed = 0;
    this._mark = 0;
    this._box.textContent = "test video " + this._id;
    if (/^blocked/.test(this._id)) {
      this._audio.pause();
      setTimeout(function () { self._set(-1); self._fire("onError", { data: 150 }); }, 120);
      return;
    }
    this._set(3);
    this._audio.currentTime = 0;
    this._audio.play().then(function () { self._set(1); }, function (e) { self._box.textContent = "autoplay refused: " + e; self._set(2); });
  };
  Player.prototype.playVideo = function () { var self = this; if (this._id) this._audio.play().then(function () { self._set(1); }); };
  Player.prototype.pauseVideo = function () { this._audio.pause(); this._tick(); this._set(2); };
  Player.prototype.getPlayerState = function () { return this._state; };
  Player.prototype.getCurrentTime = function () { this._tick(); return this._elapsed; };
  Player.prototype.getDuration = function () { return this._id ? DURATION : 0; };
  Player.prototype.getVideoData = function () { return { video_id: this._id, title: this._id ? "Test video " + this._id : "" }; };

  window.YT = { Player: Player, PlayerState: { UNSTARTED: -1, ENDED: 0, PLAYING: 1, PAUSED: 2, BUFFERING: 3, CUED: 5 } };
  setTimeout(function () { if (window.onYouTubeIframeAPIReady) window.onYouTubeIframeAPIReady(); }, 0);
})();
