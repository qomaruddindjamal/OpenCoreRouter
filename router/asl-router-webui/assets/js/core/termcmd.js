// OpenCR WebUI Core — isi terminal HTTP: tabel perintah, sebab penolakan, dan
// pencetak JSON generik. Sengaja tidak menyentuh DOM supaya mesin tampilannya
// (term.js) tetap kecil; gerbang WEB_RESP_MAX mengukur tiap berkas tersendiri.
(function (data) {
    "use strict";

    data.pad = function (s, n) {
        s = String(s);
        while (s.length < n) s += " ";
        return s;
    };

    data.fmt = function (k, v) {
        if (v === true) return "yes";
        if (v === false) return "no";
        if (v === null || v === undefined) return "-";
        if (k === "uptime_ms") {
            return Math.floor(v / 60000) + " menit " + Math.floor(v % 60000 / 1000) + " detik";
        }
        return String(v);
    };

    // [perintah, endpoint, keterangan] — jalur bacanya ada di dispatch_get kernel.
    data.GET = [
        ["/system resource print", "/api/v1/system", "identitas distro, kernel, uptime, status WAN"],
        ["/interface print", "/api/v1/interfaces", "antarmuka yang benar-benar terdeteksi"],
        ["/ip firewall address-list print", "/api/v1/firewall/rules", "kebijakan default + jumlah aturan TCAM"],
        ["/ip firewall nat print", "/api/v1/nat", "ringkasan masquerade, sesi conntrack, kapasitas"],
        ["/ip firewall nat rules print", "/api/v1/nat/rules", "daftar aturan DNAT"],
        ["/ip firewall nat filter print", "/api/v1/nat/conntrack", "sesi conntrack aktif"],
        ["/interface wireless print", "/api/v1/wireless", "radio dan stasiun nirkabel"],
        ["/interface wireless scan", "/api/v1/wireless/devices", "perangkat nirkabel terdeteksi"],
        ["/interface olt print", "/api/v1/olt", "status vOlt/GPON"],
        ["/interface olt ont print", "/api/v1/olt/ons", "daftar ONT"],
        ["/ip service print", "/api/v1/services", "flag layanan yang hidup di kernel"],
        ["/system write-status print", "/api/v1/writes/last", "hasil jalur tulis terakhir"],
        ["/ip arp print", "/api/v1/arp", "tetangga yang dipelajari dari frame ARP masuk"],
        ["/ip route print", "/api/v1/routes", "isi FIB kernel: prefiks, gerbang, antarmuka, tipe"],
        ["/ip dns print", "/api/v1/dns", "resolver dari lease DHCP + upstream yang dipegang proksi"]
    ];

    // Ditolak dengan sebab teknis, bukan dengan keluaran kosong atau contoh.
    data.NO = [
        ["/tool ping", "kernel hanya menyusun balasan gema ICMP untuk permintaan yang masuk; tidak ada pemancar echo keluar"],
        ["/tool traceroute", "penerusan IPv4 dan penurunan TTL sudah ada, tetapi kernel belum memancar balasan ICMP Time Exceeded"],
        ["/tool bandwidth-test", "tidak ada mesin penghasil beban; kernel hanya mencatat rx_packets/tx_packets"],
        ["/telnet", "TCP kernel hanya menerima koneksi, tidak ada klien maupun parser NVT"],
        ["/ip dhcp-server print", "yang dibangun hanya klien DHCP; server DHCP belum ada di kernel"]
    ];

    data.ART = [
        ".###. ####. ##### #...#   .#### .###. ####. #####",
        "#...# #...# #.... ##..#   #.... #...# #...# #....",
        "#...# ####. ####. #.#.#   #.... #...# ####. ####.",
        "#.... #.... #.... #..##   #.... #...# #...# #....",
        ".###. #.... ##### #...#   .#### .###. #..#. #..#.",
        "####. .###. #...# ##### ##### ####.",
        "#...# #...# #...# ..#.. #.... #...#",
        "####. #...# #...# ..#.. ####. ####.",
        "#...# #...# #...# ..#.. #.... #...#",
        "#..#. .###. .###. ..#.. ##### #..#."
    ];

    data.hello = function () {
        var out = [], i;
        for (i = 0; i < data.ART.length; i++) out.push(["tl-art", data.ART[i]]);
        out.push(["tl-info", "Rinjani 1.0.0 - Open Core Router, Powered by ASL Core Kernel v1.0.0"]);
        out.push(["tl-info", "CLI atas HTTP: keluaran yang tercetak adalah balasan kernel apa adanya. Ketik help."]);
        return out;
    };

    data.help = function () {
        var out = [], i;
        out.push(["tl-head", "Perintah baca - satu baris = satu panggilan HTTP nyata:"]);
        for (i = 0; i < data.GET.length; i++) {
            out.push(["tl-out", data.pad(data.GET[i][0], 34) + data.pad(data.GET[i][1], 27) + data.GET[i][2]]);
        }
        out.push(["tl-head", "Perintah tulis - hanya dua permukaan yang dimiliki kernel:"]);
        out.push(["tl-out", data.pad("/interface set <nama> mode=dhcp", 36) + "melepas alamat statis ke DHCP"]);
        out.push(["tl-out", data.pad("/interface set <nama> mode=static ip=.. netmask=.. gateway=..", 36) + "ketiganya wajib"]);
        out.push(["tl-out", data.pad("/system power arm", 36) + "mengizinkan satu aksi daya selama 20 detik"]);
        out.push(["tl-out", data.pad("/system reboot | /system shutdown", 36) + "tanpa arm dibalas 403 power_not_armed"]);
        out.push(["tl-head", "Lainnya:"]);
        out.push(["tl-out", data.pad("help", 36) + "daftar ini"]);
        out.push(["tl-out", data.pad("clear", 36) + "kosongkan layar sesi ini"]);
        out.push(["tl-out", data.pad("history", 36) + "baris yang pernah diketik di sesi ini"]);
        out.push(["tl-out", data.pad("exit", 36) + "tutup sesi, sama dengan tombol Close"]);
        out.push(["tl-head", "Ditolak kernel dengan sebab teknis:"]);
        for (i = 0; i < data.NO.length; i++) out.push(["tl-out", data.pad(data.NO[i][0], 36) + data.NO[i][1]]);
        out.push(["tl-info", "Perintah lain tidak dikenal - terminal ini tidak berpura-pura punya mesin yang belum ada."]);
        return out;
    };

    // Pencetak generik: balasan kernel dicetak apa adanya, kolom disejajarkan.
    data.print = function (d) {
        var out = [], i, j, k;
        if (d === null || typeof d !== "object") {
            return [["tl-out", data.fmt("", d)]];
        }
        if (!Array.isArray(d)) {
            for (k in d) out.push(["tl-out", data.pad(k, 20) + ": " + data.fmt(k, d[k])]);
            return out;
        }
        if (!d.length) {
            return [["tl-info", "(0 baris - kernel memang tidak punya isinya, bukan baris yang dihilangkan)"]];
        }
        var keys = [];
        for (i = 0; i < d.length; i++) for (k in d[i]) if (keys.indexOf(k) < 0) keys.push(k);
        var w = [];
        for (j = 0; j < keys.length; j++) w[j] = keys[j].length;
        var rows = [];
        for (i = 0; i < d.length; i++) {
            var r = [];
            for (j = 0; j < keys.length; j++) {
                var t = data.fmt(keys[j], d[i][keys[j]]);
                if (t.length > w[j]) w[j] = t.length;
                r.push(t);
            }
            rows.push(r);
        }
        var head = "";
        for (j = 0; j < keys.length; j++) head += data.pad(keys[j], w[j]) + "  ";
        out.push(["tl-head", head]);
        for (i = 0; i < rows.length; i++) {
            var ln = "";
            for (j = 0; j < rows[i].length; j++) ln += data.pad(rows[i][j], w[j]) + "  ";
            out.push(["tl-out", ln]);
        }
        return out;
    };
})(((window.OPCR = window.OPCR || {}).termCmd = {}));
