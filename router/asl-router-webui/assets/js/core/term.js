// OpenCR WebUI Core — mesin tampilan terminal: sesi, layar gulung, baris
// prompt, dan pemetaan satu baris perintah ke satu panggilan HTTP. Isi
// (tabel perintah, sebab penolakan, pencetak JSON) ada di termcmd.js; keduanya
// dimuat malas oleh app.js karena shell.html memegang gerbang ukuran aset.
(function (term) {
    "use strict";

    var D = null, G = null, SESS = [], cur = -1;

    function el(cls, text) {
        var d = document.createElement("div");
        d.setAttribute("class", cls);
        d.textContent = text;
        return d;
    }

    function say(cls, text) {
        if (cur < 0) return;
        var lines = String(text).split("\n");
        for (var i = 0; i < lines.length; i++) SESS[cur].lines.push([cls, lines[i]]);
        paint();
    }

    function emit(lines) {
        for (var i = 0; i < lines.length; i++) say(lines[i][0], lines[i][1]);
    }

    function paint() {
        if (!G) return;
        var s = G.screen;
        s.textContent = "";
        if (cur < 0) {
            s.appendChild(el("tl-info", "Tidak ada sesi terminal. Klik \"+ New\" untuk membuka satu."));
            return;
        }
        var l = SESS[cur].lines;
        for (var i = 0; i < l.length; i++) s.appendChild(el(l[i][0], l[i][1]));
        s.scrollTop = s.scrollHeight;
    }

    function norm(s) {
        s = s.replace(/^\s+|\s+$/g, "").replace(/\s+/g, " ");
        return (s.charAt(0) === "/" ? s : "/" + s).toLowerCase();
    }

    function get(path) {
        OPCR.api.get(path, function (ok, d, why) {
            if (ok) emit(D.print(d));
            else say("tl-err", "gagal membaca " + path + ": " + why);
        });
    }

    function post(path, form) {
        say("tl-info", "POST " + path + "  body=" + (form || "-"));
        OPCR.api.post(path, form, function (ok, d, why) {
            if (ok) {
                say("tl-ok", "kernel menerima");
                emit(D.print(d));
            } else {
                say("tl-err", "kernel menolak: " + why);
            }
        });
    }

    function greet(mine) {
        OPCR.api.get("/api/v1/system", function (ok, d) {
            if (!ok) {
                if (cur === mine) say("tl-err", "host tidak terbaca dari /api/v1/system");
                return;
            }
            var host = d.hostname || "rinjani";
            G.prompt.textContent = "[" + host + "@OpenCoreRouter] >";
            G.host.textContent = host + " - " + d.distro + " - " + d.status;
            if (cur === mine) {
                say("tl-info", "host " + host + ", " + D.fmt("uptime_ms", d.uptime_ms) + " aktif, " + d.kernel);
            }
        });
    }

    function tabs() {
        var t = G.tabs;
        t.textContent = "";
        t.className = "term-tabs" + (SESS.length > 1 ? " on" : "");
        for (var i = 0; i < SESS.length; i++) {
            var b = document.createElement("button");
            b.type = "button";
            b.className = "term-tab" + (i === cur ? " active" : "");
            b.appendChild(document.createTextNode("Terminal " + (i + 1)));
            var x = document.createElement("span");
            x.className = "term-tab-x";
            x.textContent = "×";
            b.appendChild(x);
            b.setAttribute("data-s", String(i));
            b.addEventListener("click", function () {
                cur = parseInt(this.getAttribute("data-s"), 10);
                tabs();
                paint();
                focus();
            });
            t.appendChild(b);
        }
    }

    function focus() {
        G.input.disabled = cur < 0;
        if (cur >= 0) G.input.focus();
    }

    function open() {
        SESS.push({ lines: [], hist: [], hi: 0 });
        cur = SESS.length - 1;
        tabs();
        paint();
        emit(D.hello());
        greet(cur);
        focus();
    }

    function close() {
        if (cur < 0) return;
        SESS.splice(cur, 1);
        cur = SESS.length ? Math.min(cur, SESS.length - 1) : -1;
        tabs();
        paint();
        focus();
    }

    function run(line) {
        say("tl-cmd", line);
        if (!line) return;
        var s = SESS[cur], i;
        s.hist.push(line);
        s.hi = s.hist.length;
        var sp = line.indexOf(" ");
        var head = sp < 0 ? norm(line) : norm(line.slice(0, sp));
        var rest = sp < 0 ? "" : line.slice(sp + 1).replace(/^\s+/, "");
        var c = norm(line);
        if (c === "/help") { emit(D.help()); return; }
        if (c === "/clear") { s.lines = []; paint(); return; }
        if (c === "/exit" || c === "/quit") { close(); return; }
        if (c === "/history") {
            if (!s.hist.length) say("tl-info", "(belum ada baris)");
            for (i = 0; i < s.hist.length; i++) say("tl-out", D.pad(i + 1, 4) + s.hist[i]);
            return;
        }
        for (i = 0; i < D.GET.length; i++) if (D.GET[i][0] === c) { get(D.GET[i][1]); return; }
        if (head === "/interface" && (rest === "set" || rest.indexOf("set ") === 0)) {
            var a = rest.split(" ");
            if (a.length < 3) { say("tl-err", "pemakaian: /interface set <nama> key=value ..."); return; }
            post("/api/v1/interfaces/" + encodeURIComponent(a[1]), a.slice(2).join("&"));
            return;
        }
        if (c === "/system power arm") { post("/api/v1/power", "action=arm"); return; }
        if (c === "/system reboot") { post("/api/v1/power", "action=restart"); return; }
        if (c === "/system shutdown") { post("/api/v1/power", "action=shutdown"); return; }
        var cw = c.split(" ");
        for (i = 0; i < D.NO.length; i++) {
            // NO tanpa argumen: padanan dibaca per kata dari depan supaya
            // "/tool ping 198.51.100.9" ikut mendapat alasan penolakan.
            var nw = D.NO[i][0].split(" "), k, hit = nw.length <= cw.length;
            for (k = 0; hit && k < nw.length; k++) hit = nw[k] === cw[k];
            if (hit) { say("tl-err", "ditolak: " + D.NO[i][1]); return; }
        }
        say("tl-err", "perintah tidak dikenal: \"" + line + "\" - ketik help untuk daftar yang punya jalur.");
    }

    term.setup = function (root) {
        D = D || OPCR.termCmd;
        var screen = root.querySelector(".term-screen");
        if (!screen || !D) return;
        G = {
            screen: screen,
            tabs: root.querySelector(".term-tabs"),
            host: root.querySelector(".term-host"),
            prompt: root.querySelector(".term-prompt"),
            input: root.querySelector(".term-input")
        };
        screen.addEventListener("click", function () { G.input.focus(); });
        G.input.addEventListener("keydown", function (ev) {
            var s = SESS[cur];
            if (ev.key === "Enter") {
                ev.preventDefault();
                var v = this.value.replace(/^\s+|\s+$/g, "");
                this.value = "";
                run(v);
            } else if (ev.key === "ArrowUp" && s && s.hist.length) {
                ev.preventDefault();
                s.hi = s.hi > 0 ? s.hi - 1 : 0;
                this.value = s.hist[s.hi];
            } else if (ev.key === "ArrowDown" && s && s.hist.length) {
                ev.preventDefault();
                s.hi = Math.min(s.hi + 1, s.hist.length);
                this.value = s.hi === s.hist.length ? "" : s.hist[s.hi];
            }
        });
        root.querySelector("#term-new").addEventListener("click", function () { open(); });
        root.querySelector("#term-close").addEventListener("click", function () { close(); });
        if (!SESS.length) {
            open();
        } else {
            if (cur < 0) cur = 0;
            tabs();
            paint();
            focus();
            // Label kepala sesi hidup di DOM parsial yang baru: baca ulang.
            greet(cur);
        }
    };
})(((window.OPCR = window.OPCR || {}).term = {}));
