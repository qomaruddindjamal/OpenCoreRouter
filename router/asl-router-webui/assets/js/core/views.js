// OpenCR WebUI Core — tabel modul: judul view, sumber parsial, dan relasi
// induk-anak untuk penyorotan sidebar. Dipisah dari app.js karena gerbang
// WEB_RESP_MAX mengukur tiap aset secara individual.
// Restart/Shutdown tidak ada di tabel ini: keduanya aksi daya pada sidebar
// (data-power) yang langsung membuka dialog, bukan halaman.
(function (v) {
    "use strict";

    v.views = {
        system: { title: "Dashboard", src: "/views/system.html" },
        olt: { title: "vOlt (GPON)", src: "/views/olt.html" },
        interfaces: { title: "Antarmuka Jaringan", src: "/views/interfaces.html" },
        wireless: { title: "Wireless", src: "/views/wireless.html" },
        firewall: { title: "Firewall", src: "/views/firewall.html" },
        nat: { title: "NAT & Port Forwarding", src: "/views/nat.html" },
        wireguard: { title: "WireGuard", src: "/views/wireguard.html" },
        vpn: { title: "VPN", src: "/views/vpn.html" },
        xray: { title: "XRay-Core", src: "/views/xray.html" },
        qos: { title: "QoS Manager", src: "/views/qos.html" },
        services: { title: "Registri Layanan", src: "/views/services.html" },
        arp: { title: "ARP Table", src: "/views/arp.html" },
        serviceslist: { title: "Service List", src: "/views/serviceslist.html" },
        dnsserver: { title: "DNS Server", src: "/views/dnsserver.html" },
        gateway: { title: "Gateway", src: "/views/gateway.html" },
        dhcpserver: { title: "DHCP Server", src: "/views/dhcpserver.html" },
        terminal: { title: "Terminal", src: "/views/terminal.html" },
        appearance: { title: "Appearance", src: "/views/appearance.html" },
        packages: { title: "Packages", src: "/views/packages.html" },
        usermanager: { title: "User Manager", src: "/views/usermanager.html" },
        speedtest: { title: "Speed Test", src: "/views/speedtest.html" },
        telnet: { title: "Telnet", src: "/views/telnet.html" },
        traceroute: { title: "Trace Route", src: "/views/traceroute.html" }
    };

    // Anak ikut disorot saat induknya dibuka. Induk sistem adalah anchor
    // #appearance (grup System) dan #services (grup Services).
    v.parents = {
        nat: "firewall",
        arp: "services",
        serviceslist: "services",
        dnsserver: "services",
        gateway: "services",
        dhcpserver: "services",
        packages: "appearance",
        usermanager: "appearance"
    };
})(((window.OPCR = window.OPCR || {}).views = {}));
