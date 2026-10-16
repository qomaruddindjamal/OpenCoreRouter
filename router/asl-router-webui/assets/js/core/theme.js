// OpenCR WebUI Core — mesin tema. Mode dipilih operator (default/dark/auto),
// tema yang berlaku dihitung dari mode + jam browser, lalu dipasang sebagai
// atribut data-theme pada <html>. Kernel tidak punya endpoint tema, jadi
// keadaan ini hidup di localStorage browser dan tidak pernah dikirim keluar.
(function (t) {
    "use strict";

    var KEY = "rinjani-theme-mode";
    var MODES = { default: 1, dark: 1, auto: 1 };
    // Pita waktu Auto Themes (menit sejak tengah hari):
    //   06:00-16.59 putih aqua, 17.00-17.59 senja, 18.00-05.59 gelap.
    var DUSK_START = 17 * 60, DUSK_END = 18 * 60, DAY_START = 6 * 60;

    t.clock = null;
    t.onChange = null;
    var mode = "default", resolved = "default", timer = null;

    function now() { return t.clock ? t.clock() : new Date(); }

    function bandOf(md, d) {
        if (md !== "auto") return md;
        var m = d.getHours() * 60 + d.getMinutes();
        if (m >= DUSK_START && m < DUSK_END) return "dusk";
        if (m >= DAY_START && m < DUSK_START) return "default";
        return "dark";
    }

    function resolve(md, d) { return bandOf(MODES[md] ? md : "default", d || now()); }

    function paint() {
        var next = resolve(mode);
        if (next === "default") document.documentElement.removeAttribute("data-theme");
        else document.documentElement.setAttribute("data-theme", next);
        if (next !== resolved && typeof t.onChange === "function") t.onChange(next, resolved);
        resolved = next;
    }

    // Auto hanya berganti di batas menit, jadi evaluasi dijadwalkan ke menit
    // berikutnya, bukan di-poll; mode manual tidak perlu timer.
    function schedule() {
        if (timer) { clearTimeout(timer); timer = null; }
        if (mode !== "auto") return;
        var d = now(), ms = (60 - d.getSeconds()) * 1000 - d.getMilliseconds() + 40;
        timer = setTimeout(function () { paint(); schedule(); }, ms);
    }

    function store(md) {
        try { localStorage.setItem(KEY, md); } catch (e) { /* mode tetap di RAM */ }
    }

    t.init = function () {
        var saved = null;
        try { saved = localStorage.getItem(KEY); } catch (e) { saved = null; }
        mode = MODES[saved] ? saved : "default";
        paint();
        schedule();
    };

    t.set = function (md) {
        mode = MODES[md] ? md : "default";
        store(mode);
        paint();
        schedule();
        return mode;
    };

    t.mode = function () { return mode; };
    t.resolved = function () { return resolved; };
    t.resolve = resolve;
    t.reval = paint;
    t.bands = { day: "06:00-16:59", dusk: "17:00-17:59", night: "18:00-05:59" };

    // Dieksekusi segera saat berkas dibaca, tanpa menunggu DOMContentLoaded,
    // supaya halaman tidak dibuka dalam rupa tema yang salah lalu berkedip.
    t.init();
})(((window.OPCR = window.OPCR || {}).theme = {}));
