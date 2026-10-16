// OpenCR WebUI Core — pengikat deklaratif.
// Partial HTML menentukan apa yang ditampilkan lewat atribut:
//   data-source="/api/v1/x"  -> akar yang bind-field-nya diisi objek JSON x
//   data-list="/api/v1/x"    -> tbody; satu baris <tr data-row> jadi templat
//   data-bind="nama_bidang"  -> selisih nilai ditulis lewat textContent
//   data-fmt="uptime|ip|..." -> formatter nilai
// Tidak ada innerHTML: nilai dari kernel tidak pernah jadi markup.
(function (ui) {
    "use strict";

    var DASH = "\u2014";

    function list(root, sel) {
        return Array.prototype.slice.call(root.querySelectorAll(sel));
    }

    // querySelectorAll hanya memuat keturunan; akar parsial (section[...]) harus
    // diperhitungkan sendiri, kalau tidak pengikat tingkat dokumen terlewat.
    function listIn(root, sel) {
        var nodes = list(root, sel);
        if (root.matches && root.matches(sel)) nodes.unshift(root);
        return nodes;
    }

    function empty(v) {
        return v === undefined || v === null || v === "" ||
            v === "0.0.0.0" || v === "00:00:00:00:00:00";
    }

    var F = {
        text: function (v) { return empty(v) ? DASH : String(v); },
        num: function (v) { return empty(v) ? DASH : String(v); },
        dash: function (v) { return empty(v) ? DASH : String(v); },
        ip: function (v) { return v === "0.0.0.0" ? "belum terikat" : (v || DASH); },
        bool: function (v) { return v === true ? "aktif" : v === false ? "tidak aktif" : DASH; },
        uptime: function (ms) {
            if (typeof ms !== "number") return DASH;
            var s = Math.floor(ms / 1000);
            var h = Math.floor(s / 3600);
            var m = Math.floor((s % 3600) / 60);
            var dtk = s % 60;
            // Singkatan satu huruf mudah tertukar (m = menit?, d = detik?), jadi
            // satuan ditulis jelas karena baris ini dibaca operator.
            return (h > 0 ? h + " jam " : "") + (h > 0 || m > 0 ? m + " menit " : "") + dtk + " detik";
        }
    };

    // Pill: {teks, kelas warna} — status buruk tidak pernah diberi warna aman.
    var P = {
        pill: function (v) {
            if (v === "up" || v === "operational" || v === "bound") return { t: v, c: "pill-ok" };
            if (v === "down" || v === "error") return { t: v, c: "pill-err" };
            if (v === "absent" || v === "none") return { t: v, c: "pill-neutral" };
            return { t: empty(v) ? DASH : String(v), c: "pill-neutral" };
        },
        "pill-up": function (v) {
            return v === true ? { t: "UP", c: "pill-ok" } : { t: "DOWN", c: "pill-err" };
        },
        "pill-yes": function (v) {
            return v === true ? { t: "YA", c: "pill-ok" } : { t: "TIDAK", c: "pill-err" };
        },
        "pill-bool": function (v) {
            return v === true ? { t: "AKTIF", c: "pill-ok" } : { t: "TIDAK AKTIF", c: "pill-neutral" };
        },
        "pill-policy": function (v) {
            if (v === "deny") return { t: "DENY (bawaan)", c: "pill-warn" };
            if (v === "permit") return { t: "PERMIT", c: "pill-ok" };
            return { t: empty(v) ? DASH : String(v), c: "pill-neutral" };
        },
        "num-pill": function (v) {
            return { t: empty(v) ? DASH : String(v), c: "pill-neutral" };
        }
    };

    function write(node, value, kind) {
        var base = node.getAttribute("data-base-class");
        if (base === null) {
            base = node.className || "";
            node.setAttribute("data-base-class", base);
        }
        var pill = P[kind];
        var out = pill ? pill(value) : { t: (F[kind] || F.text)(value), c: null };
        node.textContent = out.t;
        node.setAttribute("class", out.c ? base + " " + out.c : base);
    }

    function bindInto(scope, data) {
        list(scope, "[data-bind]").forEach(function (node) {
            if (node.getAttribute("data-bound")) return;
            var key = node.getAttribute("data-bind");
            var val = data && Object.prototype.hasOwnProperty.call(data, key) ? data[key] : undefined;
            write(node, val, node.getAttribute("data-fmt"));
            node.setAttribute("data-bound", "1");
        });
    }

    // Baris tabel: templat <tr data-row> disimpan sekali, lalu klon per item.
    function renderList(tbody, data) {
        if (!tbody._tpl) {
            var tpl = tbody.querySelector("[data-row]");
            if (!tpl) return;
            tbody._tpl = tpl.cloneNode(true);
            tbody.removeChild(tpl);
        }
        var rows = Array.isArray(data) ? data : (data ? [data] : []);
        tbody.textContent = "";
        if (rows.length === 0) {
            var none = document.createElement("tr");
            var cell = document.createElement("td");
            cell.setAttribute("class", "row-mode");
            cell.setAttribute("colspan", "10");
            cell.textContent = "tidak ada baris dari kernel";
            none.appendChild(cell);
            tbody.appendChild(none);
            return;
        }
        for (var i = 0; i < rows.length; i++) {
            var row = tbody._tpl.cloneNode(true);
            tbody.appendChild(row);
            bindInto(row, rows[i]);
        }
    }

    function sources(root) {
        var urls = [];
        listIn(root, "[data-source],[data-list]").forEach(function (n) {
            var u = n.getAttribute("data-source") || n.getAttribute("data-list");
            if (u && urls.indexOf(u) === -1) urls.push(u);
        });
        return urls;
    }

    // Ambil semua endpoint yang dibutuhkan akar, lalu tempelkan hasilnya.
    ui.refresh = function (root, done) {
        var urls = sources(root);
        if (urls.length === 0) {
            if (done) done(true, null);
            return;
        }
        var cache = {}, pending = urls.length, failed = null;
        urls.forEach(function (u) {
            OPCR.api.get(u, function (ok, data, why) {
                if (ok) {
                    cache[u] = data;
                } else if (!failed) {
                    failed = u + " (" + why + ")";
                }
                if (--pending === 0) {
                    if (!failed) paint(root, cache);
                    if (done) done(!failed, failed);
                }
            });
        });
    };

    function paint(root, cache) {
        ui.reset(root);
        listIn(root, "[data-list]").forEach(function (tb) {
            renderList(tb, cache[tb.getAttribute("data-list")]);
        });
        listIn(root, "[data-source]").forEach(function (sc) {
            bindInto(sc, cache[sc.getAttribute("data-source")]);
        });
    }

    ui.reset = function (root) {
        list(root, "[data-bound]").forEach(function (n) { n.removeAttribute("data-bound"); });
    };
})(((window.OPCR = window.OPCR || {}).ui = {}));
