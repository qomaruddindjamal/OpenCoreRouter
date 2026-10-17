// OpenCR WebUI Core — chrome sesi: kartu masuk, pesan penolakan kernel, dan
// tombol Keluar. Gerbang autentikasi/otorisasi yang sebenarnya ada di lapisan
// server (`asl-router-api`): berkas ini hanya menampilkan alasan yang
// dikembalikan kernel lewat 401/403, dan tidak pernah menyimpulkan sendiri
// bahwa seseorang boleh menulis. Berkas terpisah karena gerbang WEB_RESP_MAX
// mengukur tiap aset individual dan shell.html sudah mepet batas.
(function (login) {
    "use strict";

    // Kunci pesan = kode error kernel (lihat AuthError::code). Tidak ada
    // penerjemahan ulang status: pemanggil tahu kalau kernel bilang sesuatu
    // yang belum terdaftar di sini.
    var MSG = {
        unauthenticated: "Belum ada sesi. Masuk untuk membaca data kernel.",
        invalid_token: "Token sesi tidak dikenali kernel.",
        session_expired: "Sesi kedaluwarsa. Masuk kembali.",
        insufficient_role: "Role sesi ini tidak berwenang melakukan aksi tersebut.",
        no_credentials: "Kernel belum menyimpan akun. Buat lewat konsol serial: auth set <nama> <admin|viewer>"
    };

    var ov = null, fUser = null, fPw = null, fNote = null, fBtn = null;

    function el(tag, cls, text) {
        var n = document.createElement(tag);
        if (cls) n.setAttribute("class", cls);
        if (text !== undefined) n.textContent = text;
        return n;
    }

    function note(cls, text) {
        fNote.setAttribute("class", "wf-status" + (cls ? " " + cls : ""));
        fNote.textContent = text;
    }

    function row(labelText, type, into) {
        var r = el("div", "f-row");
        var lab = el("label", null, labelText);
        lab.setAttribute("for", "lg-" + type);
        var inp = el("input");
        inp.setAttribute("type", type);
        inp.setAttribute("id", "lg-" + type);
        inp.setAttribute("autocomplete", "off");
        inp.setAttribute("spellcheck", "false");
        r.appendChild(lab);
        r.appendChild(inp);
        into.appendChild(r);
        return inp;
    }

    function build() {
        ov = el("div", "wf-overlay");
        ov.setAttribute("id", "login-overlay");
        var modal = el("div", "wf-modal");
        var head = el("div", "wf-modal-head");
        head.appendChild(el("h3", null, "Masuk OpenCR"));
        var form = el("div", "wf-f");
        fUser = row("Pengguna", "text", form);
        fPw = row("Kata sandi", "password", form);
        var body = el("div", "wf-modal-body");
        body.appendChild(form);
        var foot = el("div", "wf-modal-foot");
        fNote = el("span", "wf-status");
        fBtn = el("button", "wf-btn wf-btn-primary", "Masuk");
        fBtn.setAttribute("type", "button");
        foot.appendChild(fNote);
        foot.appendChild(fBtn);
        modal.appendChild(head);
        modal.appendChild(body);
        modal.appendChild(foot);
        ov.appendChild(modal);
        document.body.appendChild(ov);
        fBtn.addEventListener("click", submit);
        function key(ev) { if (ev.key === "Enter") submit(); }
        fUser.addEventListener("keydown", key);
        fPw.addEventListener("keydown", key);
    }

    function open(why) {
        var was = !!ov && ov.getAttribute("class").indexOf("open") >= 0;
        if (!ov) build();
        note("bad", MSG[why] || "Kernel menolak permintaan (" + why + ").");
        ov.setAttribute("class", "wf-overlay open");
        if (!was) fUser.focus();
    }

    function submit() {
        var u = fUser.value, p = fPw.value;
        // Kolom sandi dikosongkan sebelum permintaan jalan: sandi tidak pernah
        // ditinggalkan di DOM, di URL, maupun di log kernel.
        fPw.value = "";
        if (!u || !p) {
            note("bad", "Nama pengguna dan kata sandi wajib diisi.");
            return;
        }
        fBtn.disabled = true;
        note("", "Memeriksa ke kernel...");
        OPCR.api.auth.login(u, p, function (ok, d, why) {
            fBtn.disabled = false;
            // Reload memakai klien API yang sama: semua modul menarik data
            // dengan token baru, jadi tidak ada halaman setengah masuk.
            if (ok) { location.reload(); return; }
            note("bad", "Masuk ditolak kernel: " + why);
            fUser.focus();
        });
    }

    // Tombol Keluar disisipkan ke grup System (induk dikenali dari tombol daya
    // di dalamnya) supaya shell.html tidak ikut menelepon batas byte.
    function addLogout(roleName) {
        var pw = document.querySelector("[data-power]");
        var sub = pw ? pw.parentNode : null;
        if (!sub || sub.querySelector("[data-logout]")) return;
        var b = el("button", "nav-action", "Keluar" + (roleName ? " (" + roleName + ")" : ""));
        b.setAttribute("type", "button");
        b.setAttribute("data-logout", "");
        b.addEventListener("click", function () {
            OPCR.nav.close();
            OPCR.nav.closeSubs();
            OPCR.api.auth.logout(function () { location.reload(); });
        });
        sub.appendChild(b);
    }

    login.init = function () {
        OPCR.api.onGate = open;
        // Satu probe tanpa efek samping di server: menentukan apakah sesi sudah
        // ada dan apakah kernel masih menunggu bootstrap kredensial.
        OPCR.api.auth.status(function (ok, d) {
            if (!ok) return;
            if (d.authenticated) { addLogout(d.role); return; }
            if (d.bootstrap_required) open("no_credentials");
        });
    };

    document.addEventListener("DOMContentLoaded", login.init);
})(((window.OPCR = window.OPCR || {}).login = {}));
