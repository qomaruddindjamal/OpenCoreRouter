// OpenCR Status Module Driver — Rinjani 1.0.0
// Render panel dari API JSON kernel (/api/v1/*). Tidak memakai library pihak
// ketiga: semua node dibuat eksplisit, nilai disisipkan via textContent supaya
// tidak ada sink HTML yang bisa disuntik dari data kernel.
(function () {
    "use strict";

    var VIEWS = {
        system: { title: "System Overview", endpoint: "/api/v1/system", render: renderSystem },
        interfaces: { title: "Network Interfaces", endpoint: "/api/v1/interfaces", render: renderInterfaces },
        firewall: { title: "Firewall Policy", endpoint: "/api/v1/firewall/rules", render: renderFirewall },
        services: { title: "Service Registry", endpoint: "/api/v1/services", render: renderServices }
    };

    var current = "system";
    var timer = null;

    function el(tag, cls, text) {
        var n = document.createElement(tag);
        if (cls) n.className = cls;
        if (text !== undefined && text !== null) n.textContent = String(text);
        return n;
    }

    function statBox(label, value, highlight) {
        var box = el("div", "stat-box");
        box.appendChild(el("span", "label", label));
        box.appendChild(el("span", "val" + (highlight ? " highlight" : ""), value));
        return box;
    }

    function card(title) {
        var c = el("div", "card");
        c.appendChild(el("h2", null, title));
        return c;
    }

    function grid() {
        return el("div", "stat-grid");
    }

    function boolText(v) {
        return v ? "aktif" : "tidak aktif";
    }

    function fmtUptime(ms) {
        var s = Math.floor(ms / 1000);
        var h = Math.floor(s / 3600);
        var m = Math.floor((s % 3600) / 60);
        return h + "j " + m + "m " + (s % 60) + "d";
    }

    function renderSystem(root, d) {
        var g = grid();
        g.appendChild(statBox("Kernel Base", d.kernel));
        g.appendChild(statBox("Distribution", d.distro));
        g.appendChild(statBox("Hostname", d.hostname));
        g.appendChild(statBox("Architecture", d.arch));
        g.appendChild(statBox("Uptime (PIT 8254)", fmtUptime(d.uptime_ms)));
        g.appendChild(statBox("WAN Link", d.wan_up ? "UP" : "DOWN", !d.wan_up));
        g.appendChild(statBox("Alamat bersumber dari", d.wan_source));
        g.appendChild(statBox("HTTP dilayani", d.requests_served));
        g.appendChild(statBox("Status", d.status));
        root.appendChild(g);
    }

    function renderInterfaces(root, list) {
        list.forEach(function (i) {
            var c = card(i.name + " — " + i.mode);
            var g = grid();
            g.appendChild(statBox("Status", i.status, i.status !== "up"));
            g.appendChild(statBox("MAC", i.mac || "-"));
            g.appendChild(statBox("IPv4", i.ip || "UNCONFIGURED"));
            g.appendChild(statBox("Netmask", i.netmask || "-"));
            g.appendChild(statBox("Gateway", i.gateway || "-"));
            g.appendChild(statBox("Sumber alamat", i.source || "-"));
            g.appendChild(statBox("RX paket", i.rx_packets === undefined ? "-" : i.rx_packets));
            g.appendChild(statBox("TX paket", i.tx_packets === undefined ? "-" : i.tx_packets));
            c.appendChild(g);
            root.appendChild(c);
        });
    }

    function renderFirewall(root, d) {
        var g = grid();
        g.appendChild(statBox("Default policy", d.default_policy, true));
        g.appendChild(statBox("TCAM rules", d.tcam_rules));
        g.appendChild(statBox("DNAT rules", d.dnat_rules));
        g.appendChild(statBox("WAN terlindungi", boolText(d.wan_protected)));
        g.appendChild(statBox("Port " + d.http_port, d.http_port_open ? "terbuka untuk manajemen" : "tertutup"));
        root.appendChild(g);
    }

    function renderServices(root, d) {
        var g = grid();
        g.appendChild(statBox("HTTP server", boolText(d.http_server), !d.http_server));
        g.appendChild(statBox("Bind address", d.http_bind_ip + ":" + d.http_port));
        g.appendChild(statBox("Sesi TCP aktif", d.tcp_sessions_active));
        g.appendChild(statBox("DHCP client", boolText(d.dhcp_client)));
        g.appendChild(statBox("Serial CLI", boolText(d.serial_cli)));
        g.appendChild(statBox("DNS proxy", boolText(d.dns_proxy)));
        g.appendChild(statBox("mDNS responder", boolText(d.mdns_responder)));
        root.appendChild(g);
    }

    function loadManagementBind() {
        request("/api/v1/services", function (ok, d) {
            var node = document.getElementById("mgmt-bind");
            if (!node) return;
            // Alamat ditulis apa adanya dari kernel; tanpa asumsi nama interface.
            node.textContent = ok
                ? "bind " + d.http_bind_ip + ":" + d.http_port
                : "bind: gagal membaca /api/v1/services";
        });
        request("/api/v1/system", function (ok, d) {
            var node = document.getElementById("hostname-display");
            if (ok && node) node.textContent = d.hostname;
        });
    }

    function request(url, done) {
        var xhr = new XMLHttpRequest();
        xhr.open("GET", url, true);
        xhr.onreadystatechange = function () {
            if (xhr.readyState !== 4) return;
            if (xhr.status !== 200) {
                done(false, null);
                return;
            }
            var parsed;
            try {
                parsed = JSON.parse(xhr.responseText);
            } catch (e) {
                done(false, null);
                return;
            }
            done(parsed.ok === true, parsed.data);
        };
        xhr.send();
    }

    function show(name) {
        var view = VIEWS[name];
        if (!view) return;
        current = name;
        Array.prototype.forEach.call(document.querySelectorAll(".nav-item"), function (a) {
            a.classList.toggle("active", a.getAttribute("data-view") === name);
        });
        var panel = document.getElementById("main-panel");
        if (!panel) return;
        panel.textContent = "";
        var c = card(view.title);
        c.appendChild(el("p", "sub", "memuat " + view.endpoint + " ..."));
        panel.appendChild(c);
        request(view.endpoint, function (ok, d) {
            panel.textContent = "";
            var cc = card(view.title);
            if (!ok) {
                cc.appendChild(el("p", "sub", "Gagal membaca " + view.endpoint + " (kernel tidak mengirim JSON valid)."));
            } else {
                view.render(cc, d);
            }
            panel.appendChild(cc);
        });
    }

    document.addEventListener("DOMContentLoaded", function () {
        console.log("Rinjani 1.0.0 WebUI Status Driver Initialized");
        loadManagementBind();
        Array.prototype.forEach.call(document.querySelectorAll(".nav-item"), function (a) {
            a.addEventListener("click", function (ev) {
                ev.preventDefault();
                var name = a.getAttribute("data-view");
                show(name);
                if (timer) clearInterval(timer);
                timer = setInterval(function () { show(name); }, 5000);
            });
        });
        show("system");
        timer = setInterval(function () { show(current); }, 5000);
    });
})();
