// OpenCR WebUI Core — perilaku tabel untuk chrome modul: pengurutan header,
// menu tampil/sembunyi kolom, glif status + ikon tipe baris kernel. Pendamping
// /js/core/table.js (gerbang respons kernel mengukur tiap berkas). Rupa saja:
// urutan disusun dari objek data milik baris, kolom hanya disembunyikan di
// layar, dan glif hanyalah rupa dari status kernel.
(function (tbl) {
    "use strict";

    // Ikon tipe baris: memakai set ikon sidebar yang sudah ada di icons.css.
    var TYPE_ICO = { ethernet: "iface", wireless: "wifi", loopback: "dash",
        bridge: "svc", vlan: "svc", dns: "svc", tunnel: "vpn", wireguard: "wg",
        olt: "olt", qos: "qos", firewall: "fw", nat: "fw", static: "svc",
        dynamic: "check" };

    function icoName(d) {
        return TYPE_ICO[String(d.type || "").toLowerCase()] || "dash";
    }

    function clearSort(table, keep) {
        tbl.each(table.querySelectorAll("th.sortable"), function (o) {
            if (o !== keep) o.className = o.className.replace(/ ?(sorted|desc)/g, "");
        });
    }

    // Kunci urut sebuah kolom dibaca dari atribut data-bind sel baris templat
    // pada kolom yang sama, jadi parsial view tidak perlu mengulang nama bidang
    // di header (hemat byte dan tidak bisa berbeda dari sel datanya).
    function keyOf(table, idx) {
        var tb = table.querySelector("tbody[data-list]");
        var row = tb && (tb._tpl || tb.querySelector("tr"));
        var cell = row && row.children[idx];
        return cell ? cell.getAttribute("data-bind") : null;
    }

    // Klik header mengurutkan baris berdata; baris tanpa data (mis. pesan
    // "0 baris dari kernel") dibiarkan di paling bawah.
    function wireSort(th, table, field) {
        if (th._wired) return;
        th._wired = true;
        th.className += " sortable";
        th.setAttribute("title", "Urutkan menurut " + field);
        th.addEventListener("click", function () {
            var tb = table.tBodies[0];
            if (!tb) return;
            // Klik pertama naik, klik berikutnya turun, lalu kembali naik.
            var desc = th.className.indexOf(" sorted") !== -1 &&
                th.className.indexOf(" desc") === -1;
            clearSort(table, th);
            th.className = th.className.replace(/ ?(sorted|desc)/g, "") +
                " sorted" + (desc ? " desc" : "");
            var rows = Array.prototype.slice.call(tb.rows).filter(function (r) {
                return !!r._data;
            });
            rows.sort(function (a, b) {
                var x = a._data[field];
                var y = b._data[field];
                var c = (typeof x === "number" && typeof y === "number")
                    ? x - y
                    : String(x === undefined ? "" : x)
                        .localeCompare(String(y === undefined ? "" : y));
                return desc ? -c : c;
            });
            tbl.each(rows, function (r) { tb.appendChild(r); });
            tbl.each(tb.rows, function (r) { if (!r._data) tb.appendChild(r); });
        });
    }

    function toggleCol(table, idx, on) {
        tbl.each(table.rows, function (r) {
            var c = r.children[idx];
            if (!c) return;
            c.className = (c.className || "").replace(/ ?col-hide/g, "") +
                (on ? "" : " col-hide");
        });
    }

    function colItem(table, idx, label) {
        var l = document.createElement("label");
        var cb = document.createElement("input");
        cb.setAttribute("type", "checkbox");
        cb.checked = true;
        cb.addEventListener("change", function () {
            toggleCol(table, idx, cb.checked);
        });
        l.appendChild(cb);
        tbl.txt(l, label);
        return l;
    }

    // Menu kolom di sel terakhir header (ikon hamburger gaya WebFig).
    function wireCols(table) {
        if (table._colswired) return;
        table._colswired = true;
        var head = table.querySelector("thead tr");
        if (!head) return;
        var th = tbl.el("th", "th-cols th-cols-show");
        var btn = tbl.el("button", "wf-tool wf-tool-icon", "cols");
        btn.setAttribute("type", "button");
        btn.setAttribute("title", "Sembunyikan atau tampilkan kolom");
        th.appendChild(btn);
        head.appendChild(th);

        var pop = tbl.el("div", "wf-cols");
        pop.appendChild(tbl.txt(tbl.el("p", "wf-cols-note"),
            "Kolom ini hanya tampil di layar; isi kernel tidak berubah."));
        tbl.each(head.children, function (c, i) {
            if (c === th) return;
            var label = (c.textContent || "").trim();
            if (label) pop.appendChild(colItem(table, i, label));
        });
        table.parentNode.appendChild(pop);

        btn.addEventListener("click", function (ev) {
            ev.stopPropagation();
            var open = pop.className.indexOf("open") !== -1;
            tbl.each(document.querySelectorAll(".wf-cols.open"), function (o) {
                o.className = "wf-cols";
            });
            pop.className = "wf-cols" + (open ? "" : " open");
            if (!open) {
                pop.style.top = Math.round(btn.getBoundingClientRect().bottom +
                    window.pageYOffset) + "px";
            }
        });
    }

    // Glif status (kolom .col-st) dan ikon tipe di depan Nama memakai ulang
    // ikon yang sudah ada: check = up/aktif, cross = down/error, minus = netral.
    function decorateRow(tr) {
        var d = tr._data;
        // data-row-ico pada tbody menjadi ikon bagi baris yang tidak membawa
        // field tipe (mis. registri layanan yang namanya sudah di parsial).
        var forced = tr.parentNode.getAttribute("data-row-ico");
        if (!d && !forced) return;
        var st = tr.querySelector(".col-st");
        if (d && st && !st.firstChild) {
            var s = d.status || d.state;
            var cls = tbl.isUp(tr) ? "st-ok"
                : (s === "down" || s === "error" ? "st-down" : "st-dim");
            var g = document.createElement("span");
            g.setAttribute("class", "act-ico " + cls);
            g.setAttribute("data-ico", cls === "st-ok" ? "check"
                : cls === "st-down" ? "cross" : "minus");
            g.setAttribute("title", String(s === undefined ? "tanpa status" : s));
            st.appendChild(g);
        }
        var nm = tr.querySelector(".row-name");
        // Sel Nama sudah berisi data kernel saat dilukis, jadi penanda node
        // (bukan kekosongan sel) yang menahan ikon ganda.
        if (nm && !nm._ico) {
            nm._ico = true;
            var box = document.createElement("span");
            box.setAttribute("class", "act-ico");
            box.setAttribute("data-ico", forced || icoName(d || {}));
            nm.insertBefore(box, nm.firstChild);
        }
    }

    // Header yang bisa diurut didaftarkan lebih dulu, sebelum tombol menu kolom
    // menambah sel di ujung baris header yang sama.
    function wireHead(table) {
        var head = table.querySelector("thead tr");
        if (!head) return;
        tbl.each(head.children, function (c, i) {
            if (c.tagName !== "TH" || c.className.indexOf("col-st") !== -1) return;
            var field = keyOf(table, i);
            if (field) wireSort(c, table, field);
        });
    }

    tbl.tableOps = function (scope) {
        tbl.each(scope.querySelectorAll("table.wf-table"), function (table) {
            wireHead(table);
            wireCols(table);
        });
    };

    tbl.onPaint = function (root) {
        tbl.each(root.querySelectorAll("tbody tr"), decorateRow);
    };
    // Namespace .tbl milik /js/core/table.js (setup, el, say, isUp) dipertahankan:
    // menimpanya dengan objek baru menghapus OPCR.tbl.setup dan menjatuhkan app.js.
})(((window.OPCR = window.OPCR || {}).tbl = OPCR.tbl || {}));
