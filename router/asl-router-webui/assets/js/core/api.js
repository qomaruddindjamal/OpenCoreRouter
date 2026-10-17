// OpenCR WebUI Core — klien API kernel (XHR + unwrap sampul {ok,data}) dan
// pemegang token sesi. Gerbang autentikasi/otorisasi berada di lapisan server:
// berkas ini hanya membawa token bila ada, dan setiap penolakan 401/403 tetap
// ditentukan kernel. Tidak ada library pihak ketiga, tidak ada fetch/promise,
// dan semua nilai masuk ke DOM lewat textContent.
window.OPCR = window.OPCR || {};

(function (api) {
    "use strict";

    var TKEY = "rinjani-session-token";
    var AKEY = "rinjani-session-role";
    // Hanya kode "identitas belum cukup" yang membuka gerbang; 429 atau 404
    // tetap menjadi tanggung jawab pemanggil.
    var LOCKED = {
        unauthenticated: 1,
        invalid_token: 1,
        session_expired: 1,
        no_credentials: 1,
        insufficient_role: 1
    };

    var token = null;
    var role = null;

    function store(k, v) {
        try {
            if (v === null) window.sessionStorage.removeItem(k);
            else window.sessionStorage.setItem(k, v);
        } catch (e) {
            /* sessionStorage mati (mode privat): token tetap hidup di memori tab. */
        }
    }

    function read(k) {
        try {
            return window.sessionStorage.getItem(k);
        } catch (e) {
            return null;
        }
    }

    function setSession(tk, rl) {
        token = tk || null;
        role = rl || null;
        store(TKEY, token);
        store(AKEY, role);
    }

    token = read(TKEY);
    role = read(AKEY);

    function apply(req) {
        if (token) req.setRequestHeader("Authorization", "Bearer " + token);
    }

    function gate(status, why) {
        if ((status === 401 || status === 403) && LOCKED[why] && api.onGate) api.onGate(why);
    }

    // Satu permintaan per panggilan: server kernel memakai Connection: close.
    api.get = function (path, done) {
        var xhr = new XMLHttpRequest();
        xhr.open("GET", path, true);
        xhr.timeout = 8000;
        apply(xhr);
        xhr.onreadystatechange = function () {
            if (xhr.readyState !== 4) return;
            var parsed = null;
            try {
                parsed = JSON.parse(xhr.responseText);
            } catch (e) {
                parsed = null;
            }
            if (xhr.status === 200 && parsed && parsed.ok === true) {
                done(true, parsed.data, 200);
                return;
            }
            // Kode dari kernel (mis. session_expired) lebih jujur dilaporkan
            // daripada angka status telanjang.
            var why = parsed && parsed.error ? parsed.error : ("http_" + (xhr.status || 0));
            gate(xhr.status, why);
            done(false, null, why);
        };
        xhr.ontimeout = function () { done(false, null, "timeout"); };
        xhr.onerror = function () { done(false, null, "network_error"); };
        xhr.send();
    };

    // Tulis konfigurasi: hanya form-urlencoded, sesuai kontrak dispatch_post
    // kernel (content-type lain ditolak 400, bukan 200 kosong).
    api.post = function (path, form, done) {
        var xhr = new XMLHttpRequest();
        xhr.open("POST", path, true);
        xhr.timeout = 8000;
        xhr.setRequestHeader("Content-Type", "application/x-www-form-urlencoded");
        apply(xhr);
        xhr.onreadystatechange = function () {
            if (xhr.readyState !== 4) return;
            var parsed = null;
            try {
                parsed = JSON.parse(xhr.responseText);
            } catch (e) {
                parsed = null;
            }
            if (xhr.status === 200 && parsed && parsed.ok === true) {
                done(true, parsed.data, 200);
                return;
            }
            var why = parsed && parsed.error ? parsed.error : ("http_" + (xhr.status || 0));
            gate(xhr.status, why);
            done(false, null, why);
        };
        xhr.ontimeout = function () { done(false, null, "timeout"); };
        xhr.onerror = function () { done(false, null, "network_error"); };
        xhr.send(form);
    };

    // Ambil markup parsial view (bukan JSON) apa adanya. Aset statis tidak
    // memerlukan sesi, jadi Authorization sengaja tidak ditempel.
    api.getText = function (path, done) {
        var xhr = new XMLHttpRequest();
        xhr.open("GET", path, true);
        xhr.timeout = 8000;
        xhr.onreadystatechange = function () {
            if (xhr.readyState !== 4) return;
            done(xhr.status === 200, xhr.status === 200 ? xhr.responseText : "", xhr.status || 0);
        };
        xhr.ontimeout = function () { done(false, "", "timeout"); };
        xhr.onerror = function () { done(false, "", "network_error"); };
        xhr.send();
    };

    // Permukaan sesi untuk chrome navigasi dan kartu masuk.
    api.auth = {
        role: function () { return role; },
        signedIn: function () { return !!token; },
        // Probe tanpa efek samping di sisi server; tidak memuat kredensial.
        status: function (done) { api.get("/api/v1/auth/status", done); },
        login: function (user, password, done) {
            api.post(
                "/api/v1/auth/login",
                "user=" + encodeURIComponent(user) + "&password=" + encodeURIComponent(password),
                function (ok, d, why) {
                    if (ok && d && d.token) {
                        setSession(d.token, d.role);
                        done(true, d, 200);
                        return;
                    }
                    // Token lama tidak dipakai lagi setelah percobaan gagal.
                    setSession(null, null);
                    done(false, null, why);
                }
            );
        },
        logout: function (done) {
            api.post("/api/v1/auth/logout", "", function (ok, d, why) {
                setSession(null, null);
                done(ok, d, why);
            });
        },
        clear: function () { setSession(null, null); }
    };
})((OPCR.api = OPCR.api || {}));
