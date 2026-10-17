// OpenCR WebUI Core — rupa WebFig: tab strip, toolbar, dan pemilihan baris
// tabel. Semua angka berasal dari endpoint kernel; tidak ada nilai yang
// disusun di browser.
(function (wf) {
    "use strict";

    function each(list, fn) {
        Array.prototype.slice.call(list || []).forEach(fn);
    }

    // ---------------------------------------------------------------- tab
    function activateTab(scope, name) {
        each(scope.querySelectorAll("[data-tab]"), function (t) {
            var on = t.getAttribute("data-tab") === name;
            t.className = "wf-tab" + (on ? " active" : "");
            t.setAttribute("aria-selected", on ? "true" : "false");
        });
        each(scope.querySelectorAll("[data-panel]"), function (p) {
            p.className = "wf-panel" + (p.getAttribute("data-panel") === name ? " active" : "");
        });
        // Gambar ulang setelah panel terlihat: clientWidth bernilai 0 saat hidden.
        OPCR.plot.draw();
    }

    function wireTabs(scope) {
        var first = scope.querySelector("[data-tab]");
        if (!first) return;
        each(scope.querySelectorAll("[data-tab]"), function (t) {
            if (t._wired) return;
            t._wired = true;
            t.addEventListener("click", function () {
                activateTab(scope, t.getAttribute("data-tab"));
            });
        });
        var active = scope.querySelector("[data-tab].active") || first;
        activateTab(scope, active.getAttribute("data-tab"));
    }

    // ------------------------------------------------------------ tabel
    function closestRow(node) {
        while (node && node.tagName !== "TR") node = node.parentNode;
        return node;
    }

    function selectedRow(scope) {
        return scope.querySelector("tbody tr.selected");
    }

    wf.selectedData = function (scope) {
        var tr = selectedRow(scope);
        return tr ? tr._data : null;
    };

    function wireTables(scope) {
        // Pemilihan baris berlaku untuk semua daftar nyata hasil kernel
        // (tbody[data-list]); hanya tabel antarmuka yang punya modal edit,
        // jadi dobel-klik pada tabel lain tidak boleh membuka modal itu.
        each(scope.querySelectorAll("tbody[data-list]"), function (tb) {
            if (tb._wired) return;
            tb._wired = true;
            tb.addEventListener("click", function (ev) {
                var tr = closestRow(ev.target);
                if (!tr || !tr._data) return;
                each(tb.querySelectorAll("tr.selected"), function (o) {
                    o.className = o.className.replace(/ ?selected/g, "");
                });
                tr.className += " selected";
            });
            // Dobel-klik baris nyata membuka modal milik tabel itu. Baris
            // "tidak ada baris dari kernel" tidak punya _data sehingga tidak
            // pernah bisa dibuka.
            tb.addEventListener("dblclick", function (ev) {
                var kind = tb.getAttribute("data-modal");
                if (!kind) return;
                var tr = closestRow(ev.target);
                if (!(tr && tr._data)) return;
                if (kind === "iface" && OPCR.ifedit) OPCR.ifedit.open(tr._data);
                else if (kind === "user" && OPCR.useredit) OPCR.useredit.open(tr._data);
            });
        });
    }

    // ---------------------------------------------------------- toolbar
    // Setelah tulis, modal tetap terbuka agar operator membaca hasil. Segar
    // berikutnya dijadwalkan lewat timer bersama app.js supaya kedua jalur
    // memakai satu hitungan mundur, tidak menumpuk interval.
    var pending = null;

    wf.scheduleReload = function (delayMs) {
        if (pending) clearTimeout(pending);
        pending = setTimeout(function () {
            pending = null;
            if (OPCR.app && OPCR.app.reload) OPCR.app.reload();
        }, delayMs);
    };

    function modulName(scope) {
        var n = (scope.matches && scope.matches("[data-module]"))
            ? scope : scope.querySelector("[data-module]");
        return n ? n.getAttribute("data-module") : "modul ini";
    }

    function say(scope, msg) {
        var r = scope.querySelector(".wf-readout");
        if (r) {
            r.textContent = msg;
            r.className = "wf-readout warn";
        }
    }

    function chosen(scope, kata) {
        var d = wf.selectedData(scope);
        if (!d) say(scope, kata + ": pilih satu baris lebih dulu.");
        return d;
    }

    function adminState(scope, kata) {
        var d = chosen(scope, kata);
        if (d) say(scope, kata + ": kernel tidak punya admin-state; " + d.name + " tetap " + d.status + ".");
    }

    // Aksi toolbar. Tabel yang mendeklarasikan data-modal punya jalur tulis
    // nyata di kernel dan modalnya yang memanggil; tabel lain tetap mendapat
    // laporan jujur bahwa mesinnya belum ada, bukan tombol yang mengubah
    // keadaan di browser saja.
    function modalKind(scope) {
        var tb = scope.querySelector(".wf-panel.active tbody[data-modal]") ||
            scope.querySelector("tbody[data-modal]");
        return tb ? tb.getAttribute("data-modal") : "";
    }

    wf.actions = {
        refresh: function () { if (OPCR.app && OPCR.app.reload) OPCR.app.reload(); },
        "new": function (scope) {
            var k = modalKind(scope);
            if (k === "user" && OPCR.useredit) return OPCR.useredit.open(null);
            say(scope, "New: kernel belum punya jalur pembuatan " + modulName(scope) + ".");
        },
        // Edit diperiksa di sini (sebelum table.js) karena hanya modul bertulis
        // yang boleh membuka modal; ifedit tetap dipakai untuk antarmuka.
        edit: function (scope) {
            var d = wf.selectedData(scope);
            if (!d) return say(scope, "Edit: pilih satu baris lebih dulu.");
            if (modalKind(scope) === "user" && OPCR.useredit) return OPCR.useredit.open(d);
            if (OPCR.ifedit && d.name) return OPCR.ifedit.open(d);
            return say(scope, "Edit: modul ini belum punya modal pengeditan.");
        },
        enable: function (scope) { adminState(scope, "Enable"); },
        disable: function (scope) { adminState(scope, "Disable"); },
        // Remove hanya sah untuk antarmuka virtual; NIC fisik hasil enumerasi
        // PCI tidak boleh dihapus dari inventaris. Penghapusan akun menuntut
        // peneguhan: modal dibuka, tombol "Hapus akun" yang mengirim.
        remove: function (scope) {
            var d = chosen(scope, "Remove");
            if (!d) return;
            if (modalKind(scope) === "user" && OPCR.useredit) {
                OPCR.useredit.open(d);
                return say(scope, "Remove: teguhkan dengan tombol Hapus akun di modal.");
            }
            say(scope, d.type === "ethernet"
                ? "Remove ditolak: " + d.name + " adalah NIC fisik ethernet."
                : "Remove: kernel belum punya penghancur baris " + modulName(scope) + ".");
        }
    };

    function wireToolbar(scope) {
        each(scope.querySelectorAll("[data-act]"), function (b) {
            if (b._wired) return;
            b._wired = true;
            b.addEventListener("click", function () {
                var fn = wf.actions[b.getAttribute("data-act")];
                if (fn) fn(scope);
            });
        });
    }

    // ------------------------------------------------------------- setup
    wf.onPaint = function (root, cache) {
        var rows = cache["/api/v1/interfaces"];
        if (Array.isArray(rows)) {
            OPCR.plot.paint(root, rows, (OPCR.config && OPCR.config.refreshMs) || 5000);
        }
    };

    wf.setup = function (scope) {
        wireTabs(scope);
        wireTables(scope);
        wireToolbar(scope);
    };
})(((window.OPCR = window.OPCR || {}).wf = {}));
