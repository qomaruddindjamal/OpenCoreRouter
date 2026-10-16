// OpenCR WebUI Core — klien API kernel (XHR + unwrap sampul {ok,data}).
// Tidak memakai library pihak ketiga dan tidak memakai fetch/promise agar
// tetap jalan pada browser lama; semua nilai masuk ke DOM lewat textContent.
window.OPCR = window.OPCR || {};

(function (api) {
    "use strict";

    // Satu permintaan per panggilan: server kernel memakai Connection: close.
    api.get = function (path, done) {
        var xhr = new XMLHttpRequest();
        xhr.open("GET", path, true);
        xhr.timeout = 8000;
        xhr.onreadystatechange = function () {
            if (xhr.readyState !== 4) return;
            if (xhr.status !== 200) {
                done(false, null, xhr.status || 0);
                return;
            }
            var parsed;
            try {
                parsed = JSON.parse(xhr.responseText);
            } catch (e) {
                done(false, null, "json_parse_error");
                return;
            }
            if (parsed && parsed.ok === true) {
                done(true, parsed.data, 200);
            } else {
                done(false, null, parsed && parsed.error ? parsed.error : "api_not_ok");
            }
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
        xhr.onreadystatechange = function () {
            if (xhr.readyState !== 4) return;
            var parsed = null;
            try {
                parsed = JSON.parse(xhr.responseText);
            } catch (e) {
                parsed = null;
            }
            var ok = xhr.status === 200 && parsed && parsed.ok === true;
            if (ok) {
                done(true, parsed.data, 200);
                return;
            }
            var why = parsed && parsed.error ? parsed.error : ("http_" + (xhr.status || 0));
            done(false, null, why);
        };
        xhr.ontimeout = function () { done(false, null, "timeout"); };
        xhr.onerror = function () { done(false, null, "network_error"); };
        xhr.send(form);
    };

    // Ambil markup parsial view (bukan JSON) apa adanya.
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
})((OPCR.api = OPCR.api || {}));
