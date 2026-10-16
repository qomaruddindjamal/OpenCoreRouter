// OpenCR WebUI Core — chrome navigasi: rail ikon-only (desktop), laci drawer
// (mobile), submenu untuk layar sentuh, dan pegangan aksi daya di sidebar.
// Dipisah dari app.js karena gerbang WEB_RESP_MAX mengukur tiap aset individual.
(function (nav) {
    "use strict";

    var MOBILE_Q = "(max-width: 900px)";
    var RAIL_KEY = "rinjani-rail";

    function isMobile() {
        return !!(window.matchMedia && window.matchMedia(MOBILE_Q).matches);
    }

    function close() {
        document.body.classList.remove("nav-open");
        var btn = document.getElementById("nav-toggle");
        if (btn) btn.setAttribute("aria-expanded", "false");
    }

    function closeSubs() {
        var open = document.querySelectorAll(".nav-parent.open");
        for (var i = 0; i < open.length; i++) open[i].classList.remove("open");
    }

    function initNav() {
        var btn = document.getElementById("nav-toggle");
        var scrim = document.getElementById("nav-scrim");
        if (!isMobile() && localStorage.getItem(RAIL_KEY) === "1") {
            document.body.classList.add("rail");
        }
        btn.addEventListener("click", function () {
            if (isMobile()) {
                // Layar sempit: tombol yang sama membuka/menutup laci navigasi.
                // Mode rail ikon-only tidak dipakai karena label harus terbaca.
                var opened = document.body.classList.toggle("nav-open");
                btn.setAttribute("aria-expanded", opened ? "true" : "false");
                return;
            }
            var rail = document.body.classList.toggle("rail");
            localStorage.setItem(RAIL_KEY, rail ? "1" : "0");
            OPCR.plot.draw();
        });
        if (scrim) scrim.addEventListener("click", close);
        if (window.matchMedia) {
            var mq = window.matchMedia(MOBILE_Q);
            // Berpindah ukuran jendela tidak boleh meninggalkan laci menggantung.
            if (mq.addEventListener) mq.addEventListener("change", close);
            else if (mq.addListener) mq.addListener(close);
        }
    }

    // Restart/Shutdown adalah aksi daya, bukan view: tidak ada hash yang diubah,
    // dialog langsung terbuka dan POST hanya jalan dua langkah (arm lalu aksi).
    function initPower() {
        var btns = document.querySelectorAll("[data-power]");
        for (var i = 0; i < btns.length; i++) {
            btns[i].addEventListener("click", function () {
                close();
                closeSubs();
                OPCR.system.ask(this.getAttribute("data-power"));
            });
        }
    }

    // Panel anak dipaku ke koordinat viewport, bukan ke kotak induknya: .sidebar
    // menggulung vertikal sehingga panel absolute ikut terpotong selebar sidebar.
    // Pada layar sentuh submenu justru dialirkan inline oleh mobile.css, jadi
    // sentuhan pada induk hanya membuka/menutup grup.
    function initSubmenus() {
        var parents = document.querySelectorAll(".nav-parent");
        var touch = !!(window.matchMedia && window.matchMedia("(hover: none)").matches);
        for (var i = 0; i < parents.length; i++) {
            (function (p) {
                var sub = p.querySelector(".nav-sub");
                if (!sub) return;
                function place() {
                    var r = p.querySelector(".nav-item").getBoundingClientRect();
                    sub.style.left = r.right + "px";
                    sub.style.top = (r.top - 1) + "px";
                    var h = sub.getBoundingClientRect().height;
                    if (h > 0 && r.top + h > innerHeight - 8) {
                        sub.style.top = Math.max(8, innerHeight - 8 - h) + "px";
                    }
                }
                if (touch) {
                    p.querySelector(".nav-item").addEventListener("click", function (ev) {
                        ev.preventDefault();
                        var was = p.classList.contains("open");
                        closeSubs();
                        if (!was) p.classList.add("open");
                    });
                    return;
                }
                p.addEventListener("mouseenter", place);
                p.addEventListener("focusin", place);
            })(parents[i]);
        }
    }

    nav.isMobile = isMobile;
    nav.close = close;
    nav.closeSubs = closeSubs;
    nav.init = function () {
        initNav();
        initSubmenus();
        initPower();
    };
})(((window.OPCR = window.OPCR || {}).nav = {}));
