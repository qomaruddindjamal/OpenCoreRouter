// OpenCR WebUI Core — chrome modul gaya WebFig: tab modul di ujung kiri tab
// strip, toolbar aksi berikon, Find/Filter, dan bacaan hasil. Baris serta angka
// tetap datang dari endpoint kernel; berkas ini hanya menata rupa. Pengurutan
// header dan menu kolom ada di /js/core/tblcols.js (gerbang respons per aset).
(function (tbl) {
    "use strict";

    var ACT = {
        "new": ["plus", "New"], edit: ["pencil", "Edit"],
        enable: ["check", "Enable"], disable: ["minus", "Disable"],
        remove: ["cross", "Remove"], comment: ["bubble", "Comment"]
    };

    // Pembantu DOM bersama: tblcols.js memakainya juga.
    tbl.each = function (l, fn) { Array.prototype.slice.call(l || []).forEach(fn); };
    tbl.txt = function (n, s) { n.appendChild(document.createTextNode(s)); return n; };

    tbl.el = function (tag, cls, ico) {
        var n = document.createElement(tag);
        if (cls) n.setAttribute("class", cls);
        if (ico) n.appendChild(icon(ico));
        return n;
    };

    // Bacaan toolbar: keterangan gagal tidak pernah disembunyikan.
    tbl.say = function (scope, msg) {
        var r = scope.querySelector(".wf-readout");
        if (r) {
            r.textContent = msg;
            r.className = "wf-readout warn";
        }
    };

    function icon(ico) {
        var g = document.createElement("span");
        g.setAttribute("class", "act-ico");
        g.setAttribute("data-ico", ico);
        return g;
    }

    // Nama + ikon modul dari data-module / data-module-ico pada parsial. Modul
    // tanpa tab strip (mis. Service List) tetap dapat satu tab modul.
    function modTab(scope) {
        var root = scope.matches && scope.matches("[data-module]")
            ? scope : scope.querySelector("[data-module]");
        if (!root) return;
        var tabs = scope.querySelector(".wf-tabs");
        var bar = scope.querySelector(".wf-toolbar");
        if (!tabs && bar) {
            tabs = tbl.el("div", "wf-tabs");
            bar.parentNode.insertBefore(tabs, bar);
        }
        if (!tabs || tabs._mod) return;
        tabs._mod = true;
        var b = tbl.el("span", "wf-tab wf-tab-mod",
            root.getAttribute("data-module-ico") || "dash");
        tbl.txt(b, root.getAttribute("data-module"));
        tabs.insertBefore(b, tabs.firstChild);
    }

    // ----------------------------------------------------------- toolbar
    function toolButton(scope, key) {
        var a = ACT[key];
        if (!a) return null;
        var b = tbl.el("button", "wf-tool", a[0]);
        b.setAttribute("type", "button");
        tbl.txt(b, a[1]);
        b.addEventListener("click", function () {
            var fn = (OPCR.wf && OPCR.wf.actions[key]) || tbl.actions[key];
            if (fn) fn(scope, b);
        });
        return b;
    }

    function refreshBtn() {
        var r = tbl.el("button", "wf-tool wf-tool-icon", "refresh");
        r.setAttribute("type", "button");
        r.setAttribute("title", "Segarkan bacaan dari kernel");
        r.addEventListener("click", function () {
            if (OPCR.app && OPCR.app.reload) OPCR.app.reload();
        });
        return r;
    }

    function findBox(scope, bar) {
        var box = tbl.el("label", "wf-find", "search");
        box.setAttribute("title", "Saring baris yang tampil di layar");
        var i = document.createElement("input");
        i.setAttribute("type", "search");
        i.setAttribute("placeholder", "Find");
        i.addEventListener("input", function () { applyFilter(scope, bar); });
        box.appendChild(i);
        return box;
    }

    function filterBtn(scope, bar) {
        var b = tbl.el("button", "wf-tool", "funnel");
        b.setAttribute("type", "button");
        tbl.txt(b, "Filter");
        b.setAttribute("title", "Hanya tampilkan baris yang up/aktif");
        b._on = false;
        b.addEventListener("click", function () {
            b._on = !b._on;
            b.className = "wf-tool" + (b._on ? " on" : "");
            applyFilter(scope, bar);
        });
        return b;
    }

    // Susunan akhir: [aksi] | pendorong | bacaan | find | filter | refresh.
    // Bacaan boleh sudah ada di markup karena mengikat endpoint nyata (mis.
    // uptime Dashboard); node itu dipindah, bukan dibuat baru.
    function buildToolbar(scope, bar) {
        bar._built = true;
        var read = bar.querySelector(".wf-readout");
        if (read) bar.removeChild(read);
        else {
            read = tbl.el("span", "wf-readout");
            tbl.txt(read, bar.getAttribute("data-hint") || "");
        }
        tbl.each((bar.getAttribute("data-actions") || "").split(","), function (k) {
            var b = toolButton(scope, k.trim());
            if (b) bar.appendChild(b);
        });
        bar.appendChild(tbl.el("span", "wf-sep"));
        bar.appendChild(tbl.el("span", "wf-spacer"));
        bar.appendChild(read);
        tbl.each((bar.getAttribute("data-tools") || "").split(","), function (k) {
            k = k.trim();
            if (k === "find") bar.appendChild(findBox(scope, bar));
            else if (k === "filter") bar.appendChild(filterBtn(scope, bar));
            else if (k === "refresh") bar.appendChild(refreshBtn());
        });
    }

    tbl.isUp = function (row) {
        var d = row._data || {};
        var s = d.status || d.state || d.enabled || d.active;
        return s === "up" || s === "bound" || s === "operational" ||
            s === true || s === "YA";
    };

    // Satu baris [data-list] boleh muncul di beberapa panel; hitungan dibaca
    // dari panel aktif saja supaya angka tidak dihitung dua kali.
    function applyFilter(scope, bar) {
        var box = bar.querySelector(".wf-find input");
        var only = !!bar.querySelector(".wf-tool.on");
        var q = box ? box.value.trim().toLowerCase() : "";
        tbl.each(scope.querySelectorAll("tbody tr"), function (tr) {
            if (!tr._data) return;
            var hit = !q || (tr.textContent || "").toLowerCase().indexOf(q) !== -1;
            var ok = !only || tbl.isUp(tr);
            tr.className = (tr.className || "").replace(/ ?row-hidden/g, "") +
                (hit && ok ? "" : " row-hidden");
        });
        var r = bar.querySelector(".wf-readout");
        if (!r) return;
        var tb = scope.querySelector(".wf-panel.active tbody[data-list]") ||
            scope.querySelector("tbody[data-list]");
        var all = tb ? tb.querySelectorAll("tr").length : 0;
        var hid = tb ? tb.querySelectorAll("tr.row-hidden").length : 0;
        r.className = "wf-readout";
        r.textContent = all
            ? (all - hid) + "/" + all + " baris tampil" +
              (q ? ' untuk "' + q + '"' : "")
            : "tidak ada baris kernel untuk disaring";
    }

    // Aksi yang belum punya makna di kernel dilaporkan, tidak dibuat-buat.
    tbl.actions = {
        edit: function (scope) {
            var d = OPCR.wf.selectedData(scope);
            if (!d) return tbl.say(scope, "Edit: pilih satu baris lebih dulu.");
            if (OPCR.ifedit && d.name) return OPCR.ifedit.open(d);
            return tbl.say(scope, "Edit: modul ini belum punya modal pengeditan.");
        },
        comment: function (scope) {
            var d = OPCR.wf.selectedData(scope);
            tbl.say(scope, "Comment: kernel tidak menyimpan kolom komentar" +
                (d ? " pada " + d.name : "") + ".");
        }
    };

    tbl.setup = function (scope) {
        tbl.each(scope.querySelectorAll(
            ".wf-toolbar[data-actions],.wf-toolbar[data-tools]"), function (bar) {
            if (!bar._built) buildToolbar(scope, bar);
        });
        modTab(scope);
        if (tbl.tableOps) tbl.tableOps(scope);
    };
})(((window.OPCR = window.OPCR || {}).tbl = {}));
