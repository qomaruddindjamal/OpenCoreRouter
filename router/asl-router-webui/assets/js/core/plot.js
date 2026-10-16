// OpenCR WebUI Core — plot trafik. Dipisah dari webfig.js karena tiap aset
// harus muat dalam satu buffer respons TCP kernel (gerbang WEB_RESP_MAX).
(function (p) {
    "use strict";

    var MAX = 40;
    var samples = [], prev = null, canvas = null, span = null;

    // Canvas melukis warna, bukan CSS, jadi warna tema dibaca dari token saat
    // menggambar; kalau tidak, garis kisi hilang di tema gelap.
    function tok(name, fb) {
        var v = getComputedStyle(document.documentElement).getPropertyValue(name);
        return v && v.trim ? (v.trim() || fb) : fb;
    }

    function sample(rows, ms) {
        var rx = 0, tx = 0, i;
        if (!rows || !rows.length) return;
        for (i = 0; i < rows.length; i++) {
            rx += Number(rows[i].rx_packets) || 0;
            tx += Number(rows[i].tx_packets) || 0;
        }
        var cur = { rx: rx, tx: tx };
        if (prev) {
            var drx = cur.rx - prev.rx, dtx = cur.tx - prev.tx;
            // Counter u32 bisa wrap; sampel lompatan negatif dibuang, bukan
            // dipaksakan nol, agar grafik tidak pernah mengarang trafik.
            if (drx >= 0 && dtx >= 0 && (drx > 0 || dtx > 0)) {
                samples.push({ rx: drx, tx: dtx, ms: ms });
                if (samples.length > MAX) samples.shift();
            }
        }
        prev = cur;
        p.draw();
    }

    p.draw = function () {
        if (!canvas) return;
        var c = canvas, w = c.clientWidth || 600, h = 150;
        if (c.width !== w) c.width = w;
        if (c.height !== h) c.height = h;
        var g = c.getContext("2d");
        g.clearRect(0, 0, w, h);
        g.strokeStyle = tok("--pf-plot-grid", "#ddf1f9");
        g.lineWidth = 1;
        var i;
        for (i = 1; i < 4; i++) {
            g.beginPath();
            g.moveTo(0, Math.round(h * i / 4) + 0.5);
            g.lineTo(w, Math.round(h * i / 4) + 0.5);
            g.stroke();
        }
        var peak = 0;
        for (i = 0; i < samples.length; i++) {
            peak = Math.max(peak, samples[i].rx, samples[i].tx);
        }
        if (span) {
            span.textContent = peak > 0
                ? "puncak " + peak + " paket per sampel"
                : "belum ada sampel (" + samples.length + "/" + MAX + ")";
        }
        if (peak === 0) return;
        var slot = w / MAX, bw = Math.max(3, slot * 0.42);
        var cRx = tok("--pf-plot-rx", "#337ab7"), cTx = tok("--pf-plot-tx", "#f57f17");
        for (i = 0; i < samples.length; i++) {
            var x = w - (samples.length - i) * slot;
            var hh = Math.max(1, Math.round(samples[i].rx * h / peak));
            var ht = Math.max(1, Math.round(samples[i].tx * h / peak));
            g.fillStyle = cRx;
            g.fillRect(Math.round(x), h - hh, Math.round(bw), hh);
            g.fillStyle = cTx;
            g.fillRect(Math.round(x + bw + 1), h - ht, Math.round(bw), ht);
        }
    };

    p.paint = function (root, rows, ms) {
        if (!canvas && root.querySelector) {
            var c = root.querySelector("#traffic-plot");
            if (c) {
                canvas = c;
                span = root.querySelector("#plot-span");
            }
        }
        if (canvas) sample(rows, ms);
    };

    // Dipanggil app.js saat modul berganti: deret lama bukan milik modul ini.
    p.reset = function () {
        samples = [];
        prev = null;
        canvas = null;
        span = null;
    };
})(((window.OPCR = window.OPCR || {}).plot = {}));
