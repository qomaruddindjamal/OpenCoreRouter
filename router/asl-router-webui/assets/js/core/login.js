// OpenCR WebUI Core — chrome sesi di header: dropdown pengguna (Settings &
// Logout) dan pengalihan ke halaman login mandiri /login.html. Gerbang
// autentikasi/otorisasi yang sebenarnya ada di lapisan server: berkas ini
// hanya memindahkan alasan yang kernel kembalikan (401/403) ke halaman masuk,
// dan tidak pernah menyimpulkan sendiri bahwa seseorang boleh masuk atau
// menulis. Nama akun yang ditampilkan berasal dari tab ini sendiri; role dan
// hak selalu hasil jawaban kernel.
(function (login) {
    "use strict";

    var PAGE = "/login.html";
    var NKEY = "rinjani-login-note";
    var UKEY = "rinjani-session-user";
    // Kode yang berarti "identitas belum cukup": chrome menutup halaman dan
    // menyerahkannya ke /login.html. insufficient_role TIDAK termasuk: sesi itu
    // hidup, hanya aksinya ditolak, jadi pengguna tidak boleh ditendang keluar.
    var BOUNCE = {
        unauthenticated: 1,
        invalid_token: 1,
        session_expired: 1,
        no_credentials: 1
    };

    function el(tag, cls, text) {
        var n = document.createElement(tag);
        if (cls) n.setAttribute("class", cls);
        if (text !== undefined) n.textContent = text;
        return n;
    }

    function read(k) {
        try {
            return window.sessionStorage.getItem(k);
        } catch (e) {
            return null;
        }
    }

    function drop(k) {
        try {
            window.sessionStorage.removeItem(k);
        } catch (e) {
            /* sessionStorage mati: tidak ada yang perlu dibersihkan di memori */
        }
    }

    function bounce(why) {
        try {
            window.sessionStorage.setItem(NKEY, why);
        } catch (e) {
            /* halaman login tetap membuka tanpa alasan; kernel tidak berubah */
        }
        location.replace(PAGE);
    }

    var chip = null;

    // Penolakan 403 dilaporkan pada chrome, bukan dengan mengusir sesi:
    // operator perlu melihat kode kernel, bukan halaman masuk.
    function deny(why) {
        if (!chip) return;
        chip.setAttribute("class", "chip-user deny");
        chip.setAttribute("title", "kernel menolak: " + why);
    }

    function item(label, hint, danger, fn) {
        var b = el("button", "um-item" + (danger ? " danger" : ""));
        b.setAttribute("type", "button");
        b.appendChild(el("span", null, label));
        if (hint) b.appendChild(el("span", "um-hint", hint));
        b.addEventListener("click", fn);
        return b;
    }

    function build(user, role) {
        var host = document.querySelector(".header-right");
        if (!host || chip) return;
        var wrap = el("div", "user-wrap");
        chip = el("button", "chip-user");
        chip.setAttribute("type", "button");
        chip.setAttribute("id", "chip-user");
        chip.setAttribute("aria-expanded", "false");
        chip.setAttribute("aria-haspopup", "true");
        chip.appendChild(el("span", "um-ava", user ? user.slice(0, 2) : "??"));
        chip.appendChild(el("span", "um-label", user || "sesi aktif"));
        chip.appendChild(el("span", "um-role", role ? "· " + role : ""));
        chip.appendChild(el("span", "um-caret", "\u25be"));

        var menu = el("div", "user-menu");
        menu.setAttribute("id", "user-menu");
        var head = el("div", "um-head");
        head.appendChild(el("span", "um-name", user || "nama akun tidak tercatat di tab ini"));
        head.appendChild(el("span", "um-role", "role sesi: " + (role || "?") + " (dinyatakan kernel)"));
        menu.appendChild(head);
        menu.appendChild(item(
            "Settings",
            "tema & preferensi antarmuka",
            false,
            function () {
                close();
                location.hash = "#appearance";
            }
        ));
        menu.appendChild(item(
            "Logout",
            "cabut sesi tab ini di kernel",
            true,
            function () {
                close();
                chip.disabled = true;
                OPCR.api.auth.logout(function () {
                    drop(UKEY);
                    bounce("unauthenticated");
                });
            }
        ));

        wrap.appendChild(chip);
        wrap.appendChild(menu);
        host.appendChild(wrap);

        chip.addEventListener("click", function (ev) {
            ev.stopPropagation();
            var on = menu.getAttribute("class").indexOf("open") >= 0;
            menu.setAttribute("class", "user-menu" + (on ? "" : " open"));
            chip.setAttribute("aria-expanded", on ? "false" : "true");
        });
        // Klik di luar menutup; tanpa itu menu menggantung di atas tabel.
        document.addEventListener("click", function (ev) {
            if (menu.contains(ev.target)) return;
            menu.setAttribute("class", "user-menu");
            chip.setAttribute("aria-expanded", "false");
        });
    }

    function close() {
        var m = document.getElementById("user-menu");
        if (m) m.setAttribute("class", "user-menu");
        if (chip) chip.setAttribute("aria-expanded", "false");
    }

    login.init = function () {
        OPCR.api.onGate = function (why) {
            if (BOUNCE[why]) bounce(why);
            else deny(why);
        };
        // Satu probe tanpa efek samping di server: menentukan apakah chrome
        // menampilkan dropdown pengguna atau menyerahkan halaman ke /login.html.
        OPCR.api.auth.status(function (ok, d) {
            if (!ok) return;
            if (!d.authenticated) {
                bounce(d.bootstrap_required ? "no_credentials" : "unauthenticated");
                return;
            }
            build(read(UKEY), d.role);
        });
    };

    document.addEventListener("DOMContentLoaded", login.init);
})(((window.OPCR = window.OPCR || {}).login = {}));
