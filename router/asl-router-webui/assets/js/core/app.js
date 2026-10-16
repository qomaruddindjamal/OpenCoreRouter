// OpenCR WebUI Core — router modul: memuat parsial view, mengikat data kernel,
// dan menyegarkan berkala. Chrome navigasi (rail/laci/submenu/aksi daya)
// hidup di /js/core/nav.js; berkas ini sengaja tetap kecil karena gerbang
// WEB_RESP_MAX mengukur tiap aset secara individual.
(function () {
    "use strict";

    // Tabel modul dan relasi induk-anak ada di views.js.
    var VIEWS = (window.OPCR && OPCR.views ? OPCR.views.views : {}) || {};
    var PARENT_OF = (window.OPCR && OPCR.views ? OPCR.views.parents : {}) || {};
    var REFRESH_MS = 5000;

    var mount = null;
    var foot = null;
    var current = null;
    var timer = null;

    (window.OPCR = window.OPCR || {});
    OPCR.config = { refreshMs: REFRESH_MS };

    function panel(title, bodyNode) {
        var p = document.createElement("div");
        p.setAttribute("class", "panel");
        var h = document.createElement("div");
        h.setAttribute("class", "panel-head");
        var t = document.createElement("h2");
        t.textContent = title;
        h.appendChild(t);
        p.appendChild(h);
        p.appendChild(bodyNode);
        return p;
    }

    function message(cls, text) {
        var n = document.createElement("p");
        n.setAttribute("class", cls);
        n.textContent = text;
        return n;
    }

    function parsePartial(text) {
        var doc = new DOMParser().parseFromString(text, "text/html");
        return doc.body ? doc.body.firstElementChild : null;
    }

    function stamp(ok, why) {
        var now = new Date();
        function two(v) { return v < 10 ? "0" + v : String(v); }
        var hh = two(now.getHours()) + ":" + two(now.getMinutes()) + ":" + two(now.getSeconds());
        foot.textContent = ok ? "sinkron " + hh + " (segar " + (REFRESH_MS / 1000) + " s)"
            : "gagal membaca kernel: " + why;
        foot.setAttribute("class", "mono" + (ok ? "" : " error-inline"));
    }

    function navTo(name) {
        var items = document.querySelectorAll("[data-view]");
        for (var i = 0; i < items.length; i++) {
            var view = items[i].getAttribute("data-view");
            var on = view === name || PARENT_OF[name] === view;
            var base = items[i].getAttribute("data-base-class");
            if (base === null) {
                base = items[i].className || "";
                items[i].setAttribute("data-base-class", base);
            }
            items[i].className = base + (on ? " active" : "");
        }
    }

    function refreshView() {
        var node = mount.firstElementChild;
        if (!node || !current) return;
        OPCR.ui.refresh(node, function (ok, why) { stamp(ok, why); });
    }

    function loadChips() {
        OPCR.api.get("/api/v1/system", function (ok, d) {
            var c = document.getElementById("chip-hostname");
            c.textContent = ok ? d.hostname : "hostname tidak terbaca";
        });
        OPCR.api.get("/api/v1/services", function (ok, d) {
            var c = document.getElementById("chip-bind");
            c.textContent = ok
                ? "bind " + d.http_bind_ip + ":" + d.http_port
                : "bind: tidak terbaca";
        });
    }

    function show(name) {
        var view = VIEWS[name];
        if (!view) return;
        current = name;
        navTo(name);
        // Setelah tujuan dipilih, chrome navigasi kembali ke keadaan diam.
        OPCR.nav.close();
        OPCR.nav.closeSubs();
        if (timer) clearInterval(timer);
        OPCR.plot.reset();
        mount.textContent = "";
        mount.appendChild(panel(view.title, message("loading", "memuat " + view.src + " ...")));
        OPCR.api.getText(view.src, function (ok, text, why) {
            if (!ok || !parsePartial(text)) {
                mount.textContent = "";
                mount.appendChild(panel(view.title,
                    message("error", "Parsial view " + view.src + " tidak terlayani kernel (HTTP " + why + ").")));
                stamp(false, view.src);
                return;
            }
            mount.textContent = "";
            var node = parsePartial(text);
            mount.appendChild(node);
            OPCR.wf.setup(node);
            OPCR.tbl.setup(node);
            OPCR.system.setup(node);
            refreshView();
            loadChips();
            timer = setInterval(function () { refreshView(); loadChips(); }, REFRESH_MS);
        });
    }

    function fromHash() {
        // Tautan masuk boleh memakai `#nama` (bentuk kanonis yang ditulis
        // router ini sendiri) atau `#/nama`; keduanya menunjuk modul yang sama.
        var h = (location.hash || "#system").replace(/^#\/?/, "");
        return VIEWS[h] ? h : "system";
    }

    document.addEventListener("DOMContentLoaded", function () {
        mount = document.getElementById("view-mount");
        foot = document.getElementById("foot-status");
        // Chrome tabel (glif status, ikon tipe) harus melihat baris yang sudah
        // terisi data, jadi ia ikut dilukis setelah plot.
        OPCR.ui.onPaint = function (root, cache) {
            OPCR.wf.onPaint(root, cache);
            OPCR.tbl.onPaint(root);
        };
        // Canvas menggambar warna sendiri dari token, jadi dilukis ulang setiap
        // kali tema berganti (mis. pita Auto berpindah saat halaman terbuka).
        OPCR.theme.onChange = function () { OPCR.plot.draw(); };
        OPCR.nav.init();
        var items = document.querySelectorAll("[data-view]");
        for (var i = 0; i < items.length; i++) {
            items[i].addEventListener("click", function (ev) {
                ev.preventDefault();
                var name = this.getAttribute("data-view");
                if (location.hash !== "#" + name) {
                    location.hash = name;
                } else {
                    show(name);
                }
            });
        }
        window.addEventListener("hashchange", function () { show(fromHash()); });
        show(fromHash());
    });

    OPCR.app = {
        reload: function () { refreshView(); loadChips(); },
        view: function () { return current; }
    };
})();
