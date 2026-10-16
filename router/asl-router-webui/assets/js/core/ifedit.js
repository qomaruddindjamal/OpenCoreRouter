// OpenCR WebUI Core — modal pengeditan antarmuka.
// Markup modal dipecah ke /views/iface-modal.html (gerbang ukuran aset) dan
// diminta sekali saat baris pertama kali diklik. Nilai diisi lewat .value /
// .textContent, bukan innerHTML, sehingga data kernel tidak pernah menjadi
// markup. Aturan dhcp vs static dijaga di sini untuk pengalaman pakai dan
// sekali lagi divalidasi kernel (satu-satunya pemegang keadaan sebenarnya).
(function (ed) {
    "use strict";

    var SRC = "/views/iface-modal.html";
    var NAME_RE = /^[a-z0-9._-]{1,16}$/;
    var st = { row: null, busy: false, bound: false, loading: false, queue: [] };

    function $(id) { return document.getElementById(id); }

    function each(list, fn) {
        Array.prototype.slice.call(list).forEach(fn);
    }

    function blank(v) {
        return (!v || v === "0.0.0.0") ? "" : String(v);
    }

    function setStatus(text, kind) {
        var s = $("modal-status");
        if (!s) return;
        s.textContent = text || "";
        s.className = "wf-status" + (kind ? " " + kind : "");
    }

    // Kegagalan memuat markup dilaporkan ke readout modul, tidak didiamkan dan
    // tidak disamarkan sebagai error kernel.
    function reportFail(why) {
        var r = document.querySelector(".wf-readout");
        if (!r) return;
        r.textContent = "modal editor gagal dimuat dari " + SRC + " (" + why + ")";
        r.className = "wf-readout warn";
    }

    function applyMode() {
        var mode = $("f-mode");
        if (!mode) return;
        var dhcp = mode.value === "dhcp";
        each([$("f-ip"), $("f-mask"), $("f-gw")], function (n) {
            if (n) n.disabled = dhcp;
        });
        var hint = $("mode-hint");
        if (hint) {
            hint.textContent = dhcp
                ? "DHCP Client: alamat diberikan server; permintaan dhcp yang membawa IP tulisan tangan ditolak kernel."
                : "IP Static: IP + netmask wajib, prefix /8../30. Gateway harus satu subnet, bukan alamat network/broadcast/loopback.";
        }
        setStatus("");
    }

    function bind() {
        if (st.bound) return;
        st.bound = true;
        $("f-mode").addEventListener("change", applyMode);
        // Sengaja TIDAK ada handler pada overlay: klik di luar modal tidak
        // pernah menutup, supaya isian operator tidak hilang karena salah
        // klik. Penutupan hanya lewat tombol Tutup atau tanda silang.
        $("modal-close").addEventListener("click", ed.close);
        $("modal-x").addEventListener("click", ed.close);
        $("modal-save").addEventListener("click", function () { ed.save(); });
    }

    function ensure(cb) {
        if ($("iface-overlay")) { cb(true); return; }
        st.queue.push(cb);
        if (st.loading) return;
        st.loading = true;
        OPCR.api.getText(SRC, function (ok, text, why) {
            st.loading = false;
            var queued = st.queue.slice();
            st.queue.length = 0;
            if (!ok) {
                reportFail("HTTP " + why);
                each(queued, function (f) { f(false); });
                return;
            }
            var node = new DOMParser().parseFromString(text, "text/html").body.firstElementChild;
            if (!node || node.id !== "iface-overlay") {
                reportFail("parsial tidak dikenali");
                each(queued, function (f) { f(false); });
                return;
            }
            document.body.appendChild(node);
            bind();
            each(queued, function (f) { f(true); });
        });
    }

    function render(row) {
        var editable = row.editable === true;
        $("modal-title").textContent = editable ? "Edit Interface" : "Interface (baca saja)";
        $("modal-tag").textContent = (row.name || "?") + " · " + (row.role || "") +
            " · " + (row.status || "");
        var lock = $("modal-locked");
        lock.style.display = editable ? "none" : "";
        if (!editable) {
            lock.textContent = "Baris " + (row.name || "?") +
                " dilaporkan kernel read-only (mode " + (row.config_mode || "local") +
                "). Jalur tulis WebUI saat ini hanya untuk sisi WAN; alamat sisi LAN diatur di host luar, bukan dari router.";
        }
        $("iface-form").style.display = editable ? "" : "none";
        $("modal-save").style.display = editable ? "" : "none";
        if (editable) {
            $("f-name").value = row.name || "";
            $("f-mode").value = row.config_mode === "static" ? "static" : "dhcp";
            $("f-ip").value = blank(row.ip);
            $("f-mask").value = blank(row.netmask);
            $("f-gw").value = blank(row.gateway);
            $("f-persist").checked = true;
            applyMode();
        }
        $("iface-overlay").className = "wf-overlay open";
    }

    ed.open = function (row) {
        st.row = row;
        st.busy = false;
        ensure(function (ok) {
            if (ok && st.row === row) render(row);
        });
    };

    ed.close = function () {
        var o = $("iface-overlay");
        if (o) o.className = "wf-overlay";
        st.row = null;
        st.busy = false;
    };

    ed.isOpen = function () {
        var o = $("iface-overlay");
        return !!o && o.classList.contains("open");
    };

    // Verdict kernel baru ada setelah poll loop mengambil antrean, jadi dibaca
    // ulang sekali setelah jeda. Status di modal selalu kutipan laporan kernel,
    // bukan kesimpulan dari respons POST yang hanya menjanjikan antrean.
    function pollVerdict() {
        OPCR.api.get("/api/v1/writes/last", function (ok, data) {
            if (!ok || !data || !ed.isOpen()) return;
            if (Number(data.queue_depth) > 0) {
                setStatus("antrean kernel masih memegang " + data.queue_depth + " perintah", "");
                return;
            }
            if (data.applied !== true) {
                setStatus("kernel belum melaporkan hasil", "");
                return;
            }
            setStatus("kernel: " + (data.report || "tanpa laporan"),
                data.ok === true ? "good" : "bad");
            if (OPCR.wf && OPCR.wf.scheduleReload) OPCR.wf.scheduleReload(400);
        });
    }

    ed.save = function () {
        if (!st.row || st.busy) return;
        var name = $("f-name").value;
        var mode = $("f-mode").value;
        if (!NAME_RE.test(name)) {
            setStatus("nama ditolak browser: huruf kecil, angka, - _ . , maks 16", "bad");
            return;
        }
        var parts = ["name=" + encodeURIComponent(name), "mode=" + mode,
            "persist=" + ($("f-persist").checked ? "1" : "0")];
        if (mode === "static") {
            parts.push("ip=" + encodeURIComponent($("f-ip").value.trim()));
            parts.push("netmask=" + encodeURIComponent($("f-mask").value.trim()));
            parts.push("gateway=" + encodeURIComponent($("f-gw").value.trim()));
        }
        st.busy = true;
        $("modal-save").disabled = true;
        setStatus("mengirim ke kernel\u2026");
        OPCR.api.post("/api/v1/interfaces/" + encodeURIComponent(st.row.name),
            parts.join("&"), function (ok, data, why) {
                st.busy = false;
                $("modal-save").disabled = false;
                if (ok) {
                    setStatus("masuk antrean kernel (" + (data.status || "queued") +
                        ", kedalaman " + data.queue_depth + "); berlaku pada tick poll", "good");
                    if (OPCR.wf && OPCR.wf.scheduleReload) OPCR.wf.scheduleReload(1500);
                    setTimeout(pollVerdict, 1600);
                } else {
                    setStatus("ditolak kernel: " + why, "bad");
                }
            });
    };
})(((window.OPCR = window.OPCR || {}).ifedit = {}));
