// OpenCR WebUI Core — router modul: memuat parsial view, mengikat data kernel,
// menyegarkan berkala, dan mempertahankan rupa chrome (sidebar ciut/lebar).
(function () {
    "use strict";

    var VIEWS = {
        system: { title: "System Overview", src: "/views/system.html" },
        interfaces: { title: "Antarmuka Jaringan", src: "/views/interfaces.html" },
        firewall: { title: "Kebijakan Firewall", src: "/views/firewall.html" },
        services: { title: "Registri Layanan", src: "/views/services.html" }
    };
    var REFRESH_MS = 5000;
    var RAIL_KEY = "rinjani-rail";

    var mount = null;
    var foot = null;
    var current = null;
    var timer = null;

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
        foot.setAttribute("class", ok ? "mono" : "mono error-inline");
    }

    function navTo(name) {
        var items = document.querySelectorAll(".nav-item[data-view]");
        for (var i = 0; i < items.length; i++) {
            var on = items[i].getAttribute("data-view") === name;
            items[i].className = "nav-item" + (on ? " active" : "");
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
        if (timer) clearInterval(timer);
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
            mount.appendChild(parsePartial(text));
            refreshView();
            loadChips();
            timer = setInterval(function () { refreshView(); loadChips(); }, REFRESH_MS);
        });
    }

    function fromHash() {
        var h = (location.hash || "#system").replace("#", "");
        return VIEWS[h] ? h : "system";
    }

    function initRail() {
        var btn = document.getElementById("nav-toggle");
        if (localStorage.getItem(RAIL_KEY) === "1") document.body.classList.add("rail");
        btn.addEventListener("click", function () {
            var rail = document.body.classList.toggle("rail");
            localStorage.setItem(RAIL_KEY, rail ? "1" : "0");
        });
    }

    document.addEventListener("DOMContentLoaded", function () {
        mount = document.getElementById("view-mount");
        foot = document.getElementById("foot-status");
        initRail();
        var items = document.querySelectorAll(".nav-item[data-view]");
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
})();
