// OpenCR WebUI Core — menu System: pilihan tema + dialog aksi daya nyata.
// Dialog konfirmasi gaya SweetAlert ditulis ulang sendiri tanpa library: kernel
// melarang dependensi pihak ketiga dan binder proyek dilarang memakai innerHTML.
(function (sys) {
    "use strict";

    var THEME = {
        "default": ["Default Themes", "putih cerah dengan aksen biru aqua, rupa bawaan konsol."],
        dark: ["Darkmode Themes", "seluruh token gelap."],
        auto: ["Auto Themes", "terang 06.00-16.59, senja 17.00-17.59, gelap 18.00-05.59."]
    };
    var THEME_NAME = { "default": "putih aqua", dark: "darkmode", dusk: "senja oranye" };

    function mk(tag, cls, text) {
        var n = document.createElement(tag);
        if (cls) n.setAttribute("class", cls);
        if (text) n.textContent = text;
        return n;
    }

    function readout(scope, msg, bad) {
        var r = scope.querySelector(".wf-readout");
        if (!r) return;
        r.textContent = msg;
        r.className = "wf-readout" + (bad ? " warn" : "");
    }

    function wireTheme(scope) {
        var sel = scope.querySelector("#f-theme");
        if (!sel || sel._wired) return;
        sel._wired = true;
        function describe(why) {
            var md = OPCR.theme.mode(), cur = OPCR.theme.resolved();
            var picked = scope.querySelector(".th-prev");
            if (picked) picked.setAttribute("data-theme-picked", md);
            var now = scope.querySelector("#theme-now");
            if (now) {
                now.textContent = THEME_NAME[cur] || cur;
                if (md === "auto") now.textContent += " (auto " + OPCR.theme.bands[cur === "dusk"
                    ? "dusk" : cur === "dark" ? "night" : "day"] + ")";
            }
            var hint = scope.querySelector("#theme-hint");
            var t = THEME[md] || [md, ""];
            if (hint) hint.textContent = t[0] + ": " + t[1];
            readout(scope, why || "Tema " + (THEME_NAME[cur] || cur) + " aktif di browser ini; tersimpan di localStorage, tidak dikirim ke kernel.");
        }
        sel.value = OPCR.theme.mode();
        describe();
        sel.addEventListener("change", function () {
            OPCR.theme.set(sel.value);
            describe();
        });
    }

    // ------------------------------------------------- dialog SweetAlert
    var box = null;
    var busy = false;
    var kinds = {
        restart: {
            glyph: "\u27F3",
            title: "Restart",
            text: "ASL Core Kernel dijalankan ulang dari awal: lease DHCP diulang, tabel NAT/conntrack dikosongkan, dan konfigurasi WAN dibaca kembali dari jurnal RJCFG01 di disk.",
            confirm: "Restart",
            warn: false,
            done: "Diterima kernel: restart dijadwalkan "
        },
        shutdown: {
            glyph: "\u23FB",
            title: "Shutdown",
            text: "ACPI power-off diminta lewat PM1_CNT. Jurnal RJCFG01 yang sudah diverifikasi tetap tersimpan di disk. Setelah mesin mati, VM hanya bisa dinyalakan lagi dari host.",
            confirm: "Shutdown",
            warn: true,
            done: "Diterima kernel: power-off dijadwalkan "
        }
    };

    function setStatus(text, cls) {
        box.status.textContent = text;
        box.status.className = "swal-status show" + (cls ? " " + cls : "");
    }

    // Aksi daya hanya sah bila kernel lebih dulu memberi hak arm (slot 20 s).
    // Dua POST terpisah: arm lalu aksi; kegagalan di salah satunya dilaporkan
    // apa adanya beserta konsekuensinya pada state kernel.
    function send(kind) {
        if (busy) return;
        busy = true;
        box.ok.disabled = true;
        box.cancel.disabled = true;
        setStatus("Langkah 1/2 \u2014 meminta hak arm ke kernel\u2026", "");
        OPCR.api.post("/api/v1/power", "action=arm", function (ok, d, why) {
            if (!ok) {
                busy = false;
                box.ok.disabled = false;
                box.cancel.disabled = false;
                setStatus("Aksi tidak dikirim kernel (" + why + "). " +
                    "State kernel tidak berubah; gunakan tombol Batal untuk menutup.", "bad");
                return;
            }
            setStatus("Ter-arm " + d.valid_for_s + " s. Langkah 2/2 \u2014 mengirim '" +
                kind + "'\u2026", "");
            OPCR.api.post("/api/v1/power", "action=" + kind, function (ok2, d2, why2) {
                if (!ok2) {
                    busy = false;
                    box.ok.disabled = false;
                    box.cancel.disabled = false;
                    setStatus("Kernel menolak aksi (" + why2 + "). Hak arm telah dihabiskan, " +
                        "tekan lagi " + kinds[kind].confirm + " untuk mengarm ulang.", "bad");
                    return;
                }
                if (!d2 || d2.status !== "scheduled") {
                    setStatus("Respons kernel tak terduga (" + d2.status + ") \u2014 " +
                        "anggap aksi TIDAK dijalankan.", "bad");
                    return;
                }
                // Tombol sengaja tetap mati: kernel sudah menjadwalkan, dan
                // menutup dialog tidak membatalkan aksi daya.
                setStatus(kinds[kind].done + d2.exec_in_ms + " ms lagi. Halaman ini akan " +
                    "berhenti merespons.", "good");
            });
        });
    }

    function build() {
        var overlay = mk("div", "swal-overlay");
        var card = mk("div", "swal");
        card.setAttribute("role", "dialog");
        card.setAttribute("aria-modal", "true");
        var icon = mk("div", "swal-icon", "\u27F3");
        var title = mk("h3", "swal-title", "Restart");
        var text = mk("p", "swal-text", "");
        var status = mk("p", "swal-status", "");
        var actions = mk("div", "swal-actions");
        var cancel = mk("button", "wf-btn", "Batal");
        var ok = mk("button", "wf-btn wf-btn-danger", "Restart");
        cancel.type = "button";
        ok.type = "button";
        cancel.addEventListener("click", close);
        ok.addEventListener("click", function () {
            send(box._kind);
        });
        actions.appendChild(cancel);
        actions.appendChild(ok);
        card.appendChild(icon);
        card.appendChild(title);
        card.appendChild(text);
        card.appendChild(status);
        card.appendChild(actions);
        overlay.appendChild(card);
        document.body.appendChild(overlay);
        // Overlay TANPA handler klik: klik di luar dialog tidak menutup, sama
        // seperti aturan modal edit antarmuka.
        return { overlay: overlay, card: card, icon: icon, title: title, text: text, status: status, ok: ok, cancel: cancel };
    }

    function close() {
        if (box) {
            box.overlay.className = "swal-overlay";
            box.ok.disabled = false;
            box.cancel.disabled = false;
            busy = false;
        }
    }

    function open(kind) {
        var k = kinds[kind];
        if (!k) return;
        if (!box) box = build();
        box._kind = kind;
        busy = false;
        box.icon.textContent = k.glyph;
        box.icon.className = "swal-icon" + (k.warn ? " swal-icon-warn" : "");
        box.title.textContent = k.title;
        box.text.textContent = k.text;
        box.ok.textContent = k.confirm;
        box.ok.disabled = false;
        box.cancel.disabled = false;
        box.status.textContent = "";
        box.status.className = "swal-status";
        box.overlay.className = "swal-overlay open";
    }

    sys.ask = open;
    sys.close = close;
    sys.setup = function (scope) {
        wireTheme(scope);
    };
})(((window.OPCR = window.OPCR || {}).system = {}));
