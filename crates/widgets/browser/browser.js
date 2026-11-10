// The browser widget's top strip (design ## widgets): url, "the bear is using this page · Take
// over", the taint banner. The app pushes `state` messages; Take over / Hand back go back over
// the widget's own IPC. The page below is a separate webview with no IPC at all.
// `?mock=1` in a plain browser shows the prototype's #widget-browser state.
(function () {
  "use strict";
  var $ = function (id) { return document.getElementById(id); };
  var w = BuddyWidget.connect();
  function show(s) {
    s = s || {};
    $("url").textContent = s.url || "about:blank";
    $("lock").textContent = /^https:/.test(s.url || "") ? "🔒" : "◌";
    $("drive").hidden = !!s.takenOver;
    $("you").hidden = !s.takenOver;
    $("taint").hidden = !s.tainted;
  }
  w.on("state", show);
  $("hide").onclick = function () { w.requestHide(); };
  $("close").onclick = function () { w.requestClose(); };
  $("takeover").onclick = function () { w.control("take_over").catch(function () {}); };
  $("handback").onclick = function () { w.control("hand_back").catch(function () {}); };
  var mock = new URLSearchParams(location.search).get("mock");
  if (mock && !w.ipc()) {
    show({ url: "https://example-shop.test/orders/4411", tainted: mock !== "clean", takenOver: mock === "you" });
    $("mockpage").hidden = false;
  }
})();
