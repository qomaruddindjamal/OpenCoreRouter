// OpenCR WebUI Core — modal akun: tulis nyata ke POST /api/v1/auth/users
// (action=set|del|revoke). Nilai selalu lewat .value/.textContent; kode
// penolakan kernel ditampilkan apa adanya.
(function (ed) {
    "use strict";

    var SRC = "/views/user-modal.html";
    var POST = "/api/v1/auth/users";
    var NAME_RE = /^[a-z0-9._-]{3,16}$/;
    var PW_MIN = 10, PW_MAX = 64;
    var st = { row: null, busy: false, bound: false, loading: false, queue: [] };

    function $(id) { return document.getElementById(id); }

    function isAdmin() {
        return !!(OPCR.api && OPCR.api.auth && OPCR.api.auth.role() === "admin");
    }

    function setStatus(text, kind) {
        var s = $("user-status");
        if (!s) return;
        s.textContent = text || "";
        s.className = "wf-status" + (kind ? " " + kind : "");
    }

    function reportFail(why) {
        var r = document.querySelector(".wf-readout");
        if (!r) return;
        r.textContent = "modal akun gagal dimuat dari " + SRC + " (" + why + ")";
        r.className = "wf-readout warn";
    }

    function bind() {
        if (st.bound) return;
        st.bound = true;
        // Tanpa handler overlay: klik di luar tidak menutup, isian tidak hilang.
        $("user-x").addEventListener("click", ed.close);
        $("u-close").addEventListener("click", ed.close);
        $("u-save").addEventListener("click", function () { ed.save(); });
        $("u-del").addEventListener("click", function () { ed.remove(); });
        $("u-revoke").addEventListener("click", function () { ed.revoke(); });
    }

    function ensure(cb) {
        if ($("user-overlay")) { cb(true); return; }
        st.queue.push(cb);
        if (st.loading) return;
        st.loading = true;
        OPCR.api.getText(SRC, function (ok, text, why) {
            st.loading = false;
            var queued = st.queue.slice();
            st.queue.length = 0;
            var node = null;
            if (ok) {
                node = new DOMParser().parseFromString(text, "text/html")
                    .body.firstElementChild;
                if (!node || node.id !== "user-overlay") node = null;
            }
            if (node) {
                document.body.appendChild(node);
                bind();
            } else {
                reportFail(ok ? "parsial tidak dikenali" : "HTTP " + why);
            }
            queued.forEach(function (f) { f(!!node); });
        });
    }

    function render(row) {
        var edit = !!row;
        var admin = isAdmin();
        $("user-title").textContent = edit ? "Ubah akun" : "Akun baru";
        $("user-tag").textContent = edit
            ? row.user + " · " + row.role + " · " + row.rounds + " iterasi PBKDF2"
            : "formulir kosong";
        // Nama kunci tabel kredensial: dikunci saat ubah agar set tidak membuat
        // akun kedua.
        $("u-name").value = edit ? (row.user || "") : "";
        $("u-role").value = edit && row.role === "viewer" ? "viewer" : "admin";
        $("u-pw").value = "";
        $("u-pw2").value = "";
        $("u-name").disabled = edit || !admin;
        $("u-role").disabled = !admin;
        $("u-pw").disabled = !admin;
        $("u-pw2").disabled = !admin;
        $("u-save").style.display = admin ? "" : "none";
        $("u-revoke").style.display = admin ? "" : "none";
        $("u-del").style.display = admin && edit ? "" : "none";
        var lock = $("user-locked");
        // Penjelasan 403 sudah statis di markup; hanya peran sesi yang ditulis.
        $("user-lockrole").textContent =
            "Sesi ini berperan " + (OPCR.api.auth.role() || "anonim") + ".";
        lock.style.display = admin ? "none" : "";
        // Dua hint sudah ada di markup, hanya dipindah tampilnya.
        $("user-pwhint").style.display = edit ? "none" : "";
        $("user-pwedit").style.display = edit ? "" : "none";
        setStatus("");
        $("user-overlay").className = "wf-overlay open";
    }

    ed.open = function (row) {
        st.row = row || null;
        st.busy = false;
        ensure(function (ok) {
            if (ok && st.row === (row || null)) render(row || null);
        });
    };

    ed.close = function () {
        var o = $("user-overlay");
        if (o) o.className = "wf-overlay";
        st.row = null;
        st.busy = false;
    };

    ed.isOpen = function () {
        var o = $("user-overlay");
        return !!o && o.classList.contains("open");
    };

    // set/del diterapkan AuthStore seketika, tetapi jurnal RJCFG01 baru ditulis
    // pada tick poll berikutnya ([R31]) — verdict dibaca ulang, bukan disimpulkan.
    function pollJournal() {
        OPCR.api.get("/api/v1/writes/last", function (ok, data) {
            if (!ok || !data || !ed.isOpen()) return;
            var rep = data.report || "";
            if (rep.indexOf("kredensial:") !== 0) return;
            setStatus("kernel: " + rep,
                data.applied === true && data.ok === true ? "good" : "bad");
        });
    }

    function send(parts, kata, done) {
        if (st.busy) return;
        st.busy = true;
        setStatus("mengirim ke kernel\u2026");
        OPCR.api.post(POST, parts.join("&"), function (ok, data, why) {
            st.busy = false;
            if (!ok) return setStatus("ditolak kernel: " + why, "bad");
            // Revoke tidak punya persist: aksi sesi bukan perubahan kredensial.
            var extra = data && data.persist ? ", jurnal " + data.persist
                : ", sesi cabut " + (data && data.sessions_revoked || "?");
            setStatus("kernel: " + kata + " (status " + (data && data.status || "?") +
                extra + ")", "good");
            if (OPCR.wf && OPCR.wf.scheduleReload) OPCR.wf.scheduleReload(700);
            setTimeout(pollJournal, 1200);
            if (done) done(data);
        });
    }

    ed.save = function () {
        if (st.busy || !isAdmin()) return;
        var name = ($("u-name").value || "").trim();
        var pw = $("u-pw").value;
        if (!NAME_RE.test(name)) {
            return setStatus("nama ditolak browser: 3-16 karakter huruf kecil/angka/-_.",
                "bad");
        }
        if (pw.length < PW_MIN || pw.length > PW_MAX) {
            return setStatus("kata sandi ditolak browser: " + PW_MIN + "-" +
                PW_MAX + " karakter", "bad");
        }
        if (pw !== $("u-pw2").value) {
            return setStatus("pengulangan tidak sama; permintaan tidak dikirim", "bad");
        }
        send(["action=set", "user=" + encodeURIComponent(name),
            "role=" + $("u-role").value, "password=" + encodeURIComponent(pw)],
            "memasang " + name, function () {
                // Sandi hilang dari field segera setelah masuk badan permintaan.
                $("u-pw").value = "";
                $("u-pw2").value = "";
            });
    };

    ed.remove = function () {
        if (st.busy || !isAdmin() || !st.row) {
            return setStatus("Hapus: pilih satu baris akun lebih dulu.", "bad");
        }
        // Modal tetap terbuka: "applied" belum berarti jurnal RJCFG01 tertulis.
        send(["action=del", "user=" + encodeURIComponent(st.row.user || "")],
            "menghapus " + (st.row.user || "?"));
    };

    ed.revoke = function () {
        if (st.busy || !isAdmin()) return;
        // Nilai user wajib ada di kontrak kernel meski revoke mencabut semua.
        send(["action=revoke", "user=" + encodeURIComponent(
            (st.row && st.row.user) || "admin")], "mencabut sesi", function () {
                // Sesi ini ikut mati: kernel tidak lagi mengenali tokennya.
                if (OPCR.api.auth.clear) OPCR.api.auth.clear();
            });
    };
})(((window.OPCR = window.OPCR || {}).useredit = {}));
