//! `asl-router-api` — REST API v1 Dispatcher & Security Rate Limiter
//!
//! Bare-metal `#![no_std]` REST API endpoints returning strict JSON:
//! - GET `/api/v1/system` -> System metadata, kernel, OS, uptime, hostname
//! - GET `/api/v1/interfaces` -> HANYA antarmuka yang benar-benar terdeteksi
//! - GET `/api/v1/firewall/rules` -> TCAM / NAT filter rules
//! - GET `/api/v1/nat` + `/api/v1/nat/rules` + `/api/v1/nat/conntrack`
//! - GET `/api/v1/wireless` + `/api/v1/wireless/devices` (driver none => 0 baris)
//! - GET `/api/v1/olt` + `/api/v1/olt/ons` (marker crate => 0 baris)
//! - GET `/api/v1/services` -> Active daemons (dhcp, http, cli, dns/mdns)
//! - GET `/api/v1/arp` + `/api/v1/arp/stats` -> tabel tetangga ARP nyata yang
//!   dipelajari kernel dari frame ARP yang masuk (bukan daftar karangan)
//! - GET `/api/v1/routes` + `/api/v1/routes/stats` -> isi FIB kernel dan berapa
//!   kali rute benar-benar ditanya oleh jalur penerusan
//! - GET `/api/v1/dns` -> resolver yang dibawa lease DHCP terpisah dari
//!   upstream yang kini dipegang mesin proksi DNS
//! - POST `/api/v1/interfaces/<nama>` -> antrean tulis yang dieksekusi poll loop
//! - POST `/api/v1/auth/login|logout` + GET `/api/v1/auth/status` (P0.1)
//! - GET/POST `/api/v1/auth/users` -> User Manager: daftar/set/hapus/revoke
//!   (khusus admin; keluaran hanya nama, role, dan iterasi PBKDF2)
//! - Token-Bucket Rate Limiter per client / global (RFC 6585 HTTP 429)
//!
//! SELURUH nilai pada respons bersumber dari `WebRuntime` yang diisi kernel per
//! iterasi poll loop. Endpoint dilarang mengarang alamat IP atau status service.
//! Endpoint tulis hanya memvalidasi + menaruh perintah di antrean; eksekusi
//! (perubahan FIB/NAT/DHCP dan commit RJCFG01) selalu terjadi di sisi kernel.
//! Pengecualian yang dinyatakan jujur: perubahan kredensial langsung menyentuh
//! `AuthStore` milik dispatcher (satu-satunya pemegang tabel) dan menyalakan
//! bendera `auth_dirty`; yang menulis blok auth ke RJCFG01 tetap poll loop
//! kernel, jadi nasib persistensi dibaca klien dari `/api/v1/writes/last`.
//!
//! GERBANG AUTENTIKASI (P0.1): dispatcher menuntut token sesi sah untuk
//! SELURUH jalur `/api/v1/...` kecuali `POST /api/v1/auth/login` dan
//! `GET /api/v1/auth/status`. Aksi menulis, aksi daya, dan pembacaan daftar
//! akun menuntut role admin. Gerbang ada di lapisan server, bukan di peramban:
//! permintaan mentah dari klien apa pun melewati kode yang sama.

#![no_std]

pub mod auth;

use asl_router_webcore::{HttpRequest, HttpResponseBuilder, HttpStatus};

use asl_router_crypto::hex;
use asl_router_persistence::RJCFG_AUTH_USER_MAX;
use auth::{
    pct_decode, AuthError, AuthStore, Role, AUTH_DEFAULT_ROUNDS, AUTH_IDLE_TICKS,
    AUTH_PASSWORD_MAX, AUTH_PASSWORD_MIN, AUTH_TOKEN_BYTES, AUTH_USERS_MAX,
};

/// Tantangan skema pada respons 401 (RFC 6750 3). `realm` statis, tanpa data
/// pengguna.
const AUTH_CHALLENGE: &str = "Bearer realm=\"rinjani\"";

/// Panjang maksimum nama antarmuka pada jalur tulis WebUI. Sumber kebenarannya
/// skema RJCFG01, supaya aturan validasi UI tidak pernah lepas dari penyimpanan.
pub const IFACE_NAME_MAX: usize = asl_router_persistence::RJCFG_IFACE_NAME_MAX;
/// Jumlah perintah tulis yang dapat menganggu sebelum poll loop mengeksekusi.
pub const WEB_WRITE_QUEUE: usize = 4;
/// Batas baris snapshot conntrack yang dibawa ke satu respons JSON.
pub const NAT_CONN_SNAPSHOT_MAX: usize = 12;
/// Kode mode WAN pada perintah tulis (sama dengan konstanta journal).
pub const WAN_MODE_DHCP: u8 = asl_router_persistence::RJCFG_WAN_MODE_DHCP;
pub const WAN_MODE_STATIC: u8 = asl_router_persistence::RJCFG_WAN_MODE_STATIC;
/// Panjang maksimum body form yang diterima satu permintaan tulis.
pub const WEB_WRITE_BODY_MAX: usize = 512;
/// Kode aksi daya hasil POST `/api/v1/power` — sama dengan perintah CLI serial.
pub const POWER_RESTART: u8 = 0;
pub const POWER_SHUTDOWN: u8 = 1;
/// Jendela arming aksi daya: 20 s dihitung pada tick 100 ms kernel.
pub const POWER_ARM_TICKS: u64 = 200;
/// Sela antara ACK dan eksekusi daya, supaya respons HTTP sempat terkirim ke
/// browser sebelum kernel reboot/power-off.
pub const POWER_EXEC_MS: u64 = 1_500;

use asl_router_persistence::iface_name_ok;

/// Snapshot satu aturan DNAT (port forwarding) milik `NatEngine`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DnatSnapshot {
    pub wan_port: u16,
    pub protocol: &'static str,
    pub target_ip: [u8; 4],
    pub target_port: u16,
    pub active: bool,
}

/// Snapshot satu sesi conntrack (untuk tabel NAT hidup).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnSnapshot {
    pub protocol: &'static str,
    pub src_ip: [u8; 4],
    pub src_port: u16,
    pub dst_ip: [u8; 4],
    pub dst_port: u16,
    pub trans_port: u16,
    pub fwd_packets: u32,
    pub rev_packets: u32,
}

/// Batas baris snapshot tetangga ARP dalam satu respons.
pub const ARP_SNAPSHOT_MAX: usize = 16;

/// Snapshot satu entri tabel tetangga ARP kernel. Angka mentah (indeks
/// antarmuka, tick terakhir) diterjemahkan ke nama antarmuka dan umur detik di
/// `handle_arp`, sehingga endpoint hanya melaporkan apa yang diisi kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArpSnapshot {
    pub ip: [u8; 4],
    pub mac: [u8; 6],
    /// 0 = WAN, 1 = LAN (konstanta `ARP_IF_*` di asl-router-netstack).
    pub ifindex: u8,
    /// `true` bila frame terakhir yang menyegarkan adalah ARP reply.
    pub reply: bool,
    pub seen: u32,
    pub last_tick: u64,
}

/// Batas baris snapshot FIB dalam satu respons. Setara kapasitas tabel rute
/// kernel (`MAX_ROUTES` di asl-router-lpm) sehingga endpoint tidak pernah
/// menyembunyikan rute karena daftar dipotong.
pub const ROUTE_SNAPSHOT_MAX: usize = 64;

/// Terjemahan varian `NextHop` kernel menjadi label tipe rute. Nilai numerik
/// dipakai pada snapshot; label teks hanya muncul di respons.
pub const ROUTE_KIND_CONNECTED: u8 = 0;
pub const ROUTE_KIND_STATIC: u8 = 1;
pub const ROUTE_KIND_LOCAL: u8 = 2;
pub const ROUTE_KIND_DROP: u8 = 3;

/// Snapshot satu baris FIB kernel. Isi byte-per-byte dari `RouteEntry`; `handle_routes`
/// hanya menambahkan nama antarmuka, jadi tidak ada rute yang lahir di lapisan API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteSnapshot {
    pub network: [u8; 4],
    pub prefix_len: u8,
    /// Nomor antarmuka kernel: `IF_WAN` = 1 (eth0), `IF_LAN` = 2 (eth1),
    /// 0 bila rute tidak punya antarmuka keluar (local/drop).
    pub ifindex: u8,
    /// Alamat gateway.next_hop_ip; `[0,0,0,0]` untuk rute langsung/local.
    pub gateway: [u8; 4],
    pub metric: u32,
    pub kind: u8,
}

/// Satu perintah tulis hasil POST `/api/v1/interfaces/<nama>`. Struct ini murni
/// data: kernel yang memutuskan eksekusi, sehingga validasi API tidak pernah
/// bisa mengubah state sendiri.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebWriteCmd {
    pub target: [u8; IFACE_NAME_MAX],
    pub target_len: u8,
    pub new_name: [u8; IFACE_NAME_MAX],
    pub new_name_len: u8,
    pub mode: u8,
    pub ip: [u8; 4],
    pub netmask: [u8; 4],
    pub gw: [u8; 4],
    pub persist: bool,
}

impl WebWriteCmd {
    pub fn target_str(&self) -> &str {
        let n = core::cmp::min(self.target_len as usize, IFACE_NAME_MAX);
        core::str::from_utf8(&self.target[..n]).unwrap_or("")
    }

    pub fn new_name_str(&self) -> &str {
        let n = core::cmp::min(self.new_name_len as usize, IFACE_NAME_MAX);
        core::str::from_utf8(&self.new_name[..n]).unwrap_or("")
    }
}

/// Batas himpunan alamat lokal pada Tahap A: eth0 (WAN) dan eth1 (LAN).
/// Penambah antarmuka ke-3 dan seterusnya menuntut pendaftaran antarmuka nyata
/// di kernel, bukan sekadar menaikkan angka ini.
pub const WEB_LOCAL_MAX: usize = 2;

/// Satu alamat lokal router yang boleh melayani WebUI, terpasang pada satu
/// antarmuka nyata. Kernel hanya mengisi struct ini dari alamat yang sedang
/// dipegang antarmuka (`ifindex != 0` = entri hidup, `0` = slot kosong),
/// sehingga lapisan server tidak pernah menerima alamat karangan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebLocalAddr {
    pub ip: [u8; 4],
    pub mac: [u8; 6],
    /// Antarmuka pemilik alamat: 1 = eth0 (WAN), 2 = eth1 (LAN).
    pub ifindex: u8,
}

impl WebLocalAddr {
    pub const fn empty() -> Self {
        Self {
            ip: [0; 4],
            mac: [0; 6],
            ifindex: 0,
        }
    }
}

/// Snapshot state runtime router yang dilaporkan ke WebUI. Kernel mengisi
/// struktur ini setiap iterasi; `tick_100ms` adalah satuan waktu bucket limiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebRuntime<'a> {
    pub hostname: &'a str,
    pub uptime_ms: u64,
    pub wan_present: bool,
    pub wan_name: &'a str,
    pub lan_name: &'a str,
    pub wan_ip: [u8; 4],
    pub wan_netmask: [u8; 4],
    pub wan_gw: [u8; 4],
    /// Asal alamat `wan_gw`, dilaporkan apa adanya oleh klien DHCP: label rantai
    /// resolusi (`option-3-ack`, `option-3-offer`, `none`) atau nama permukaan
    /// konfigurasi statis. `siaddr` dan identifier server tidak pernah dijadikan
    /// gateway, dan endpoint tidak pernah menebak dari angka jaring.
    pub wan_gw_src: &'static str,
    /// Asal `wan_netmask`, dengan label rantai lease (`option-1-ack`,
    /// `option-1-offer`, `classful-turunan`, `none`) atau nama permukaan statik.
    /// `classful-turunan` berarti kernel menyimpulkan dari kelas alamat, BUKAN
    /// menerima opsi 1 dari server.
    pub wan_netmask_src: &'static str,
    /// Resolver pertama yang dibawa lease (opsi 6). `[0,0,0,0]` berarti server
    /// tidak mengirim opsi 6 sama sekali — bukan resolver cadangan karangan.
    pub wan_dns1: [u8; 4],
    /// Resolver kedua, hanya terisi bila server mengirim lebih dari satu.
    pub wan_dns2: [u8; 4],
    /// Asal resolver DNS: `option-6-ack`, `option-6-offer`, `none`, atau nama
    /// permukaan konfigurasi statik.
    pub wan_dns_src: &'static str,
    pub wan_up: bool,
    pub wan_source: &'static str,
    pub wan_mac: [u8; 6],
    pub lan_present: bool,
    pub lan_ip: [u8; 4],
    pub lan_mac: [u8; 6],
    /// Himpunan alamat lokal yang KINI boleh melayani WebUI. Kernel hanya
    /// mengisi dari alamat yang benar-benar terpasang pada antarmuka yang
    /// nyata-nyata ada; slot sisa wajib `ifindex == 0`.
    pub local: [WebLocalAddr; WEB_LOCAL_MAX],
    /// Banyak entri hidup di `local` (0 berarti tidak ada alamat yang melayani).
    pub local_count: u8,
    pub http_listening: bool,
    pub http_port: u16,
    pub dhcp_client_active: bool,
    /// `true` bila alamat WAN sedang dipegang operator (mode statik).
    pub wan_config_static: bool,
    pub dns_active: bool,
    /// Upstream yang KINI dipegang mesin proksi DNS, dibaca langsung dari
    /// strukturnya. Beda dengan `wan_dns1` (nilai dari lease): field inilah yang
    /// membuktikan lease benar-benar disalurkan ke mesin, bukan hanya disimpan.
    pub dns_upstream: [u8; 4],
    pub dns_cache_hits: u32,
    pub dns_cache_misses: u32,
    /// Rekaman cache yang masih aktif saat snapshot diambil.
    pub dns_cache_entries: u32,
    pub dns_cache_capacity: u32,
    /// Inang statik aktif di tabel mesin proksi.
    pub dns_host_count: u32,
    pub dns_host_capacity: u32,
    pub mdns_active: bool,
    pub serial_cli_active: bool,
    pub tcam_rules: u32,
    pub dnat_rules: u32,
    pub default_policy: &'static str,
    /// Salinan aturan DNAT nyata dari `NatEngine` (maks `MAX_DNAT_RULES`).
    pub dnat_list: &'a [DnatSnapshot],
    /// Salinan sesi conntrack aktif (terbatas `NAT_CONN_SNAPSHOT_MAX`).
    pub nat_sessions: &'a [ConnSnapshot],
    /// Total sesi conntrack aktif di tabel kernel (bisa > panjang snapshot).
    pub nat_session_count: u32,
    pub nat_capacity: u32,
    pub nat_table_full_drops: u32,
    /// Salinan entri tetangga ARP yang masih segar (maks `ARP_SNAPSHOT_MAX`).
    pub arp_list: &'a [ArpSnapshot],
    /// Kapasitas tabel ARP kernel; bisa lebih besar dari panjang snapshot.
    pub arp_count: u32,
    pub arp_capacity: u32,
    /// Berapa kali tabel ARP penuh sehingga tetangga tertua digusur.
    pub arp_evictions: u32,
    /// Frame ARP yang tidak dapat dijadikan tetangga (header tak wajar, milik
    /// sendiri, atau alamat nol/broadcast).
    pub arp_rejected: u32,
    /// Umur maksimum entri ARP dalam detik, seperti yang dikonfigurasi kernel.
    pub arp_timeout_s: u32,
    /// Salinan isi FIB kernel (maks `ROUTE_SNAPSHOT_MAX` baris).
    pub route_list: &'a [RouteSnapshot],
    /// Jumlah rute aktif pada tabel kernel saat snapshot diambil.
    pub route_count: u32,
    /// Kapasitas maksimum rute tabel FIB kernel.
    pub route_capacity: u32,
    /// Berapa kali datapath kernel benar-benar menanyakan keputusan rute ke FIB.
    /// Nol berarti `route_packet` belum pernah dipanggil pada jalur penerusan.
    pub route_lookups: u32,
    /// Paket transit yang benar-benar dikirim keluar lewat antarmuka lain.
    pub route_forwards: u32,
    /// Paket transit yang dibatalkan kernel (tidak ada rute, antarmuka tidak
    /// ada, tetangga belum diketahui, atau TTL habis).
    pub route_drops: u32,
    /// `true` hanya bila datapath kernel benar-benar memanggil mesin NAT.
    pub nat_forwarding_wired: bool,
    /// `true` hanya bila aturan TCAM benar-benar ditanyakan pada frame yang
    /// lewat (klasifikasi in-line). Mesin yang berdiri sendiri tanpa dipanggil
    /// jalur data tidak boleh dilaporkan sebagai perlindungan.
    pub firewall_wired: bool,
    /// Hasil penerapan perintah tulis terakhir oleh kernel (dipakai modal WebUI).
    pub last_write_ok: bool,
    pub last_write_report: &'a str,
    pub wan_rx: u32,
    pub wan_tx: u32,
    pub lan_rx: u32,
    pub lan_tx: u32,
    pub sessions_active: u32,
    pub requests_served: u32,
    pub tick_100ms: u64,
    /// Alamat IPv4 peer dari sesi TCP yang melayani permintaan ini. Diisi
    /// server web per permintaan (bukan fakta kernel) dan hanya dipakai untuk
    /// pemisahan kuota rate-limit serta ikatan sesi. Tidak pernah dibaca dari
    /// header yang dikendalikan klien.
    pub peer_ip: [u8; 4],
}

impl<'a> Default for WebRuntime<'a> {
    fn default() -> Self {
        Self {
            hostname: "",
            uptime_ms: 0,
            wan_present: false,
            wan_name: "",
            lan_name: "",
            wan_ip: [0, 0, 0, 0],
            wan_netmask: [0, 0, 0, 0],
            wan_gw: [0, 0, 0, 0],
            wan_gw_src: "none",
            wan_netmask_src: "none",
            wan_dns1: [0, 0, 0, 0],
            wan_dns2: [0, 0, 0, 0],
            wan_dns_src: "none",
            wan_up: false,
            wan_source: "none",
            wan_mac: [0; 6],
            lan_present: false,
            lan_ip: [0; 4],
            lan_mac: [0; 6],
            local: [WebLocalAddr::empty(); WEB_LOCAL_MAX],
            local_count: 0,
            http_listening: false,
            http_port: 0,
            dhcp_client_active: false,
            wan_config_static: false,
            dns_active: false,
            dns_upstream: [0, 0, 0, 0],
            dns_cache_hits: 0,
            dns_cache_misses: 0,
            dns_cache_entries: 0,
            dns_cache_capacity: 0,
            dns_host_count: 0,
            dns_host_capacity: 0,
            mdns_active: false,
            serial_cli_active: false,
            tcam_rules: 0,
            dnat_rules: 0,
            default_policy: "deny",
            dnat_list: &[],
            nat_sessions: &[],
            nat_session_count: 0,
            nat_capacity: 0,
            nat_table_full_drops: 0,
            arp_list: &[],
            arp_count: 0,
            arp_capacity: 0,
            arp_evictions: 0,
            arp_rejected: 0,
            arp_timeout_s: 0,
            route_list: &[],
            route_count: 0,
            route_capacity: 0,
            route_lookups: 0,
            route_forwards: 0,
            route_drops: 0,
            nat_forwarding_wired: false,
            firewall_wired: false,
            last_write_ok: false,
            last_write_report: "",
            wan_rx: 0,
            wan_tx: 0,
            lan_rx: 0,
            lan_tx: 0,
            sessions_active: 0,
            requests_served: 0,
            tick_100ms: 0,
            peer_ip: [0, 0, 0, 0],
        }
    }
}

impl<'a> WebRuntime<'a> {
    /// Cocokkan alamat tujuan IPv4 sebuah frame dengan himpunan alamat lokal.
    /// Hanya entri hidup (`ifindex != 0`) yang dihitung, jadi alamat nol atau
    /// slot kosong tidak pernah bisa membuka gerbang server.
    pub fn local_match(&self, dst: [u8; 4]) -> Option<usize> {
        let n = core::cmp::min(self.local_count as usize, WEB_LOCAL_MAX);
        let mut i = 0usize;
        while i < n {
            let e = self.local[i];
            if e.ifindex != 0 && e.ip == dst {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

// -----------------------------------------------------------------------------
// JSON WRITER BUFFER TETAP (zero heap)
// -----------------------------------------------------------------------------

struct JsonBuf<'a> {
    buf: &'a mut [u8],
    len: usize,
    overflow: bool,
}

impl<'a> JsonBuf<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self {
            buf,
            len: 0,
            overflow: false,
        }
    }

    fn raw(&mut self, s: &[u8]) -> &mut Self {
        if self.len + s.len() > self.buf.len() {
            self.overflow = true;
            return self;
        }
        self.buf[self.len..self.len + s.len()].copy_from_slice(s);
        self.len += s.len();
        self
    }

    fn str(&mut self, s: &str) -> &mut Self {
        self.raw(s.as_bytes())
    }

    fn u32(&mut self, mut v: u32) -> &mut Self {
        let mut tmp = [0u8; 10];
        let mut i = tmp.len();
        if v == 0 {
            i -= 1;
            tmp[i] = b'0';
        }
        while v > 0 {
            i -= 1;
            tmp[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        self.raw(&tmp[i..])
    }

    fn u64(&mut self, mut v: u64) -> &mut Self {
        let mut tmp = [0u8; 20];
        let mut i = tmp.len();
        if v == 0 {
            i -= 1;
            tmp[i] = b'0';
        }
        while v > 0 {
            i -= 1;
            tmp[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        self.raw(&tmp[i..])
    }

    fn bool(&mut self, v: bool) -> &mut Self {
        self.raw(if v { b"true" } else { b"false" })
    }

    fn ip(&mut self, ip: [u8; 4]) -> &mut Self {
        self.u32(ip[0] as u32)
            .raw(b".")
            .u32(ip[1] as u32)
            .raw(b".")
            .u32(ip[2] as u32)
            .raw(b".")
            .u32(ip[3] as u32)
    }

    fn mac_hex(&mut self, mac: [u8; 6]) -> &mut Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut b = [0u8; 17];
        let mut i = 0;
        for (n, o) in mac.iter().enumerate() {
            b[i] = HEX[(*o >> 4) as usize];
            i += 1;
            b[i] = HEX[(*o & 0x0F) as usize];
            i += 1;
            if n < 5 {
                b[i] = b':';
                i += 1;
            }
        }
        self.raw(&b[..i])
    }

    fn quoted_mac(&mut self, mac: [u8; 6]) -> &mut Self {
        self.raw(b"\"").mac_hex(mac).raw(b"\"")
    }

    fn quoted_ip(&mut self, ip: [u8; 4]) -> &mut Self {
        self.raw(b"\"").ip(ip).raw(b"\"")
    }

    fn quoted(&mut self, s: &str) -> &mut Self {
        self.raw(b"\"").str(s).raw(b"\"")
    }

    fn finish(&self) -> Option<usize> {
        if self.overflow {
            None
        } else {
            Some(self.len)
        }
    }
}

/// Ember token tunggal. `refill_every` memungkinkan pengisian lebih lambat dari
/// satu tick (dipakai jalur autentikasi agar proteksi brute-force tidak bisa
/// dinetralkan dengan menunggu tick berikutnya).
#[derive(Clone, Copy)]
pub struct TokenBucketRateLimiter {
    capacity: u32,
    tokens: u32,
    refill_rate: u32,
    refill_every: u64,
    last_refill: u64,
}

impl TokenBucketRateLimiter {
    pub const fn new(capacity: u32, refill_rate: u32) -> Self {
        Self::with_period(capacity, refill_rate, 1)
    }

    pub const fn with_period(capacity: u32, refill_rate: u32, refill_every: u64) -> Self {
        Self {
            capacity,
            tokens: capacity,
            refill_rate,
            refill_every: if refill_every == 0 { 1 } else { refill_every },
            last_refill: 0,
        }
    }

    pub fn check_and_consume(&mut self, current_tick: u64) -> bool {
        self.refill(current_tick);
        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Satu token tanpa mengisi ulang: dipakai pencatat kegagalan autentikasi.
    pub fn drain_one(&mut self) {
        if self.tokens > 0 {
            self.tokens -= 1;
        }
    }

    fn refill(&mut self, current_tick: u64) {
        if current_tick < self.last_refill {
            // Tick melingkar/mundur (mustahil pada kernel satu-klok, tetap aman):
            // geser dasar pengisian tanpa memberi token cuma-cuma.
            self.last_refill = current_tick;
            return;
        }
        let elapsed = current_tick - self.last_refill;
        let periods = elapsed / self.refill_every;
        if periods > 0 {
            let add = (periods as u32).saturating_mul(self.refill_rate);
            self.tokens = self.tokens.saturating_add(add).min(self.capacity);
            self.last_refill += periods * self.refill_every;
        }
    }
}

/// Kategori endpoint. Pemisahan ini menjawab DEFER-04: kuota autentikasi tidak
/// boleh habis dipakai pembacaan dashboard, dan sebaliknya.
pub const RATE_READ: u8 = 0;
pub const RATE_WRITE: u8 = 1;
pub const RATE_AUTH: u8 = 2;

/// Jumlah ember yang ditampung. Table statis (tidak ada heap): klien ke-17
/// menggantikan ember paling lama menganggur, bukan menolak layanan.
pub const RATE_BUCKETS: usize = 16;

/// Ember dianggur lebih dari 10 s (100 tick) dihapus supaya memori tidak
/// terisi oleh klien yang sudah pergi.
const RATE_IDLE_TICKS: u64 = 100;

const fn policy(category: u8) -> (u32, u32, u64) {
    match category {
        // Baca: 60 burst + 20 token/tick (200/menit per klien). Pola refresh UI
        // terukur 4 permintaan/5 s = 0,8/s, jadi ini longgar untuk operator sah
        // dan tetap membatasi banjir dari satu klien.
        RATE_READ => (60, 20, 1),
        // Tulis: 6 burst + 1 token per 2 tick (5/s).
        RATE_WRITE => (6, 1, 2),
        // Autentikasi: 4 burst + 1 token per 30 tick (0,33/s) -> percobaan kata
        // sandi ditekan tanpa mengunci administrator yang sah.
        _ => (4, 1, 30),
    }
}

/// Kumpulan ember per (alamat klien, kategori). Alamat dibaca dari header IPv4
/// sesi TCP — bukan dari header `X-Forwarded-For` atau header apa pun yang
/// dikendalikan klien, karena server ini tidak berada di belakang proxy.
pub struct ClientRateLimiter {
    buckets: [Option<(BucketKey, TokenBucketRateLimiter)>; RATE_BUCKETS],
    cursor: usize,
    pub denied: u32,
    pub evictions: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BucketKey {
    ip: [u8; 4],
    category: u8,
    last_seen: u64,
}

impl ClientRateLimiter {
    pub const fn new() -> Self {
        Self {
            buckets: [None; RATE_BUCKETS],
            cursor: 0,
            denied: 0,
            evictions: 0,
        }
    }

    /// `true` bila permintaan boleh dilayani; `false` wajib dibalas 429.
    pub fn allow(&mut self, ip: &[u8; 4], category: u8, tick: u64) -> bool {
        self.purge(tick);
        let mut free: Option<usize> = None;
        let mut oldest: Option<(u64, usize)> = None;
        for i in 0..RATE_BUCKETS {
            match &mut self.buckets[i] {
                None => {
                    if free.is_none() {
                        free = Some(i);
                    }
                }
                Some((k, lim)) => {
                    if k.ip == *ip && k.category == category {
                        k.last_seen = tick;
                        let ok = lim.check_and_consume(tick);
                        if !ok {
                            self.denied += 1;
                        }
                        return ok;
                    }
                    if oldest.map_or(true, |(t, _)| k.last_seen < t) {
                        oldest = Some((k.last_seen, i));
                    }
                }
            }
        }
        let (cap, rate, every) = policy(category);
        let slot = match free {
            Some(i) => i,
            None => {
                // Tidak ada slot kosong dan tidak ada yang menganggur: ganti
                // ember paling lama menganggur. Penggusuran dicatat, tidak
                // membuat klien lain ditolak.
                self.evictions += 1;
                oldest.map(|(_, i)| i).unwrap_or(self.cursor)
            }
        };
        self.cursor = (slot + 1) % RATE_BUCKETS;
        let mut lim = TokenBucketRateLimiter::with_period(cap, rate, every);
        // Permintaan pertama tetap mengonsumsi satu token. Tanpa baris ini,
        // setiap identitas baru mendapat satu permintaan cuma-cuma di luar
        // kuota, dan rotasi alamat bisa menetralkan batas atas sepenuhnya.
        let ok = lim.check_and_consume(tick);
        self.buckets[slot] = Some((
            BucketKey {
                ip: *ip,
                category,
                last_seen: tick,
            },
            lim,
        ));
        ok
    }

    /// Hapus ember klien yang sudah menganggur lama.
    fn purge(&mut self, tick: u64) {
        for i in 0..RATE_BUCKETS {
            if let Some((k, _)) = &self.buckets[i] {
                if tick.saturating_sub(k.last_seen) > RATE_IDLE_TICKS {
                    self.buckets[i] = None;
                }
            }
        }
    }

    /// Hukuman tambahan untuk kegagalan autentikasi: menguras token ember
    /// klien tersebut tanpa menyentuh klien lain.
    pub fn penalize(&mut self, ip: &[u8; 4], category: u8, tick: u64) {
        for i in 0..RATE_BUCKETS {
            if let Some((k, lim)) = &mut self.buckets[i] {
                if k.ip == *ip && k.category == category {
                    lim.drain_one();
                    k.last_seen = tick;
                    return;
                }
            }
        }
    }

    #[must_use]
    pub fn live_buckets(&self) -> u32 {
        self.buckets.iter().filter(|b| b.is_some()).count() as u32
    }
}

impl Default for ClientRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ApiDispatcher {
    pub rate_limiter: ClientRateLimiter,
    /// Tabel kredensial + sesi. Field publik karena bootstrap admin dan
    /// penyimpanan RJCFG01 terjadi di sisi kernel (konsol serial), bukan lewat
    /// HTTP: tidak ada jalur jaringan yang bisa membuat akun pertama.
    pub auth: AuthStore,
    /// Antrean perintah tulis hasil POST. Poll kernel mengambilnya lewat
    /// `take_write()`; dispatcher tidak pernah mengubah state router.
    write_queue: [Option<WebWriteCmd>; WEB_WRITE_QUEUE],
    write_queue_len: usize,
    /// Hitungan percobaan tulis yang ditolak (antrean penuh / validasi gagal).
    pub writes_rejected: u32,
    /// Penjaga aksi daya: `power_armed` dinyalakan oleh `action=arm` dan
    /// kedaluwarsa setelah `POWER_ARM_TICKS`. Satu arm melayani satu aksi.
    power_armed: bool,
    power_arm_tick: u64,
    /// Aksi daya tertunda yang sudah diterima kernel (dieksekusi poll loop).
    power_pending: Option<u8>,
    /// Kredensial berubah lewat HTTP: poll kernel wajib menyalin blok auth ke
    /// jurnal RJCFG01 lalu menghapus bendera ini. Dispatcher tidak punya akses
    /// blok, jadi persistensi selalu keputusan kernel.
    pub auth_dirty: bool,
}

impl ApiDispatcher {
    pub const fn new() -> Self {
        Self {
            // Ember terpisahkan per (klien, kategori); lihat DEFER-04.
            rate_limiter: ClientRateLimiter::new(),
            // Tabel kredensial kosong = sistem terkunci. Tidak ada akun bawaan.
            auth: AuthStore::new(),
            write_queue: [None, None, None, None],
            write_queue_len: 0,
            writes_rejected: 0,
            power_armed: false,
            power_arm_tick: 0,
            power_pending: None,
            auth_dirty: false,
        }
    }

    /// Sisa jendela arming dalam tick; `None` bila tidak ter-arm atau kedaluwarsa.
    fn arm_left_ticks(&self, tick: u64) -> Option<u64> {
        if !self.power_armed {
            return None;
        }
        let age = tick.wrapping_sub(self.power_arm_tick);
        if age >= POWER_ARM_TICKS {
            None
        } else {
            Some(POWER_ARM_TICKS - age)
        }
    }

    pub fn power_armed(&self, tick: u64) -> bool {
        self.arm_left_ticks(tick).is_some()
    }

    pub fn power_arm_left_s(&self, tick: u64) -> u32 {
        let ticks = self.arm_left_ticks(tick).unwrap_or(0);
        ((ticks + 9) / 10) as u32
    }

    /// Ambil aksi daya yang sudah diterima, untuk dieksekusi kernel.
    pub fn take_power(&mut self) -> Option<u8> {
        self.power_pending.take()
    }

    /// Ambil satu perintah tulis tertua untuk dieksekusi kernel.
    pub fn take_write(&mut self) -> Option<WebWriteCmd> {
        if self.write_queue_len == 0 {
            return None;
        }
        let cmd = self.write_queue[0].take();
        for i in 0..self.write_queue_len.saturating_sub(1) {
            self.write_queue[i] = self.write_queue[i + 1];
        }
        self.write_queue[self.write_queue_len - 1] = None;
        self.write_queue_len -= 1;
        cmd
    }

    /// Masukkan perintah ke antrean. `false` bila antrean penuh (kernel belum
    /// menyempatkan drain) — klien harus mencoba lagi, bukan diterima diam-diam.
    pub fn push_write(&mut self, cmd: WebWriteCmd) -> bool {
        if self.write_queue_len >= WEB_WRITE_QUEUE {
            self.writes_rejected = self.writes_rejected.wrapping_add(1);
            return false;
        }
        self.write_queue[self.write_queue_len] = Some(cmd);
        self.write_queue_len += 1;
        true
    }

    pub fn write_queue_depth(&self) -> usize {
        self.write_queue_len
    }

    pub fn dispatch(
        &mut self,
        req: &HttpRequest,
        current_tick: u64,
        out_buf: &mut [u8],
        rt: &WebRuntime,
    ) -> Result<usize, &'static str> {
        if !self
            .rate_limiter
            .allow(&rt.peer_ip, RATE_READ, current_tick)
        {
            let builder = HttpResponseBuilder::new(out_buf, HttpStatus::TooManyRequests)
                .with_content_type("application/json");
            return builder.finish_with_body(b"{\"ok\":false,\"error\":\"rate_limit_exceeded\"}");
        }

        // Satu jalur API boleh anonim: probe status untuk kartu masuk. Jalur itu
        // hanya melaporkan hasil probe header klien, tidak ada data akun.
        if req.path == "/api/v1/auth/status" {
            return self.handle_auth_status(req, out_buf, current_tick);
        }
        // Daftar akun adalah data priviledged (nama akun adalah setengah dari
        // usaha menebak sandi), sehingga jalur ini menuntut role admin.
        let users_list = req.path == "/api/v1/auth/users";
        // Seluruh jalur baca lain menuntut sesi sah. Gerbang ada sebelum pencocokan
        // rute, sehingga enumerasi jalur oleh klien anonim dijawab 401, bukan 404.
        if let Err(e) = self
            .auth
            .authorize(req.authorization, users_list, current_tick)
        {
            return Self::auth_err(out_buf, e);
        }

        match req.path {
            "/api/v1/system" => self.handle_system(out_buf, rt),
            "/api/v1/auth/users" => self.handle_users_list(out_buf),
            "/api/v1/interfaces" => self.handle_interfaces(out_buf, rt),
            "/api/v1/firewall/rules" => self.handle_firewall_rules(out_buf, rt),
            "/api/v1/nat" => self.handle_nat_summary(out_buf, rt),
            "/api/v1/nat/rules" => self.handle_nat_rules(out_buf, rt),
            "/api/v1/nat/conntrack" => self.handle_nat_conntrack(out_buf, rt),
            "/api/v1/wireless" => self.handle_wireless(out_buf),
            "/api/v1/wireless/devices" => self.handle_wireless_devices(out_buf),
            "/api/v1/olt" => self.handle_olt(out_buf),
            "/api/v1/olt/ons" => self.handle_olt_ons(out_buf),
            "/api/v1/services" => self.handle_services(out_buf, rt),
            "/api/v1/arp" => self.handle_arp(out_buf, rt, current_tick),
            "/api/v1/arp/stats" => self.handle_arp_stats(out_buf, rt),
            "/api/v1/routes" => self.handle_routes(out_buf, rt),
            "/api/v1/routes/stats" => self.handle_routes_stats(out_buf, rt),
            "/api/v1/dns" => self.handle_dns(out_buf, rt),
            "/api/v1/writes/last" => self.handle_write_status(out_buf, rt),
            _ => {
                let builder = HttpResponseBuilder::new(out_buf, HttpStatus::NotFound)
                    .with_content_type("application/json");
                builder.finish_with_body(b"{\"ok\":false,\"error\":\"not_found\"}")
            }
        }
    }

    /// Jalur tulis: `POST /api/v1/interfaces/<nama>` dengan body
    /// `application/x-www-form-urlencoded`. Respons hanya melaporkan perintah
    /// masuk antrean atau alasan penolakan; kernel yang mengeksekusi.
    pub fn dispatch_post(
        &mut self,
        req: &HttpRequest,
        out_buf: &mut [u8],
        rt: &WebRuntime,
    ) -> Result<usize, &'static str> {
        // Login adalah satu-satunya jalur tulis anonim; ember autentikasinya
        // sendiri yang membatasi laju tebak kata sandi.
        if req.path == "/api/v1/auth/login" {
            return self.handle_login(req, out_buf, rt);
        }
        if !self
            .rate_limiter
            .allow(&rt.peer_ip, RATE_WRITE, rt.tick_100ms)
        {
            let builder = HttpResponseBuilder::new(out_buf, HttpStatus::TooManyRequests)
                .with_content_type("application/json");
            return builder.finish_with_body(b"{\"ok\":false,\"error\":\"rate_limit_exceeded\"}");
        }
        if !self.same_origin(req, rt) {
            self.writes_rejected = self.writes_rejected.wrapping_add(1);
            return self.err_json(out_buf, HttpStatus::Forbidden, "host_mismatch");
        }
        // Aksi daya dan perubahan konfigurasi adalah operasi priviledged: role
        // admin wajib. Logout cukup sesi sah milik klien itu sendiri.
        let need_admin = req.path == "/api/v1/power"
            || req.path.starts_with("/api/v1/interfaces/")
            || req.path.starts_with("/api/v1/auth/users");
        if let Err(e) = self
            .auth
            .authorize(req.authorization, need_admin, rt.tick_100ms)
        {
            return Self::auth_err(out_buf, e);
        }
        if req.path == "/api/v1/auth/logout" {
            return self.handle_logout(req, out_buf);
        }
        if req.path == "/api/v1/auth/users" {
            return self.handle_users_post(req, out_buf);
        }
        if req.path == "/api/v1/power" {
            return self.handle_power(req, out_buf, rt);
        }
        let iface = match req.path.strip_prefix("/api/v1/interfaces/") {
            Some(rest) if !rest.is_empty() => rest,
            _ => return self.err_json(out_buf, HttpStatus::NotFound, "not_found"),
        };
        let body = &req.body[..core::cmp::min(req.content_length, req.body.len())];
        match self.build_write_cmd(iface, req.content_type, body) {
            Ok(cmd) => {
                if !self.push_write(cmd) {
                    return self.err_json(
                        out_buf,
                        HttpStatus::InternalServerError,
                        "write_queue_full",
                    );
                }
                let mut ack = [0u8; 128];
                let n = {
                    let mut j = JsonBuf::new(&mut ack);
                    j.raw(b"{\"ok\":true,\"data\":{\"status\":\"queued\",\"target\":")
                        .quoted(iface)
                        .raw(b",\"mode\":")
                        .quoted(if cmd.mode == WAN_MODE_STATIC {
                            "static"
                        } else {
                            "dhcp"
                        })
                        .raw(b",\"persist\":")
                        .bool(cmd.persist)
                        .raw(b",\"queue_depth\":")
                        .u32(self.write_queue_len as u32)
                        .raw(b"}}");
                    j.finish().ok_or("JSON write ack overflow")?
                };
                Self::ok_json(out_buf, &ack[..n])
            }
            Err(code) => {
                self.writes_rejected = self.writes_rejected.wrapping_add(1);
                self.err_json(out_buf, HttpStatus::BadRequest, code)
            }
        }
    }

    /// Jalur daya dua-langkah: `POST /api/v1/power` dengan body `action=`.
    /// `arm` menyalakan slot selama `POWER_ARM_TICKS`; `restart`/`shutdown`
    /// hanya diterima bila slot masih ter-arm dan langsung dihabiskan, sehingga
    /// satu permintaan liar dari jaringan tidak bisa mematikan router. Kernel
    /// yang mengeksekusi (poll loop), dispatcher hanya menampung.
    fn handle_power(
        &mut self,
        req: &HttpRequest,
        out_buf: &mut [u8],
        rt: &WebRuntime,
    ) -> Result<usize, &'static str> {
        let ct = match req.content_type {
            Some(c) => c,
            None => return self.err_json(out_buf, HttpStatus::BadRequest, "missing_content_type"),
        };
        if !starts_with_ci(ct, "application/x-www-form-urlencoded") {
            return self.err_json(out_buf, HttpStatus::BadRequest, "unsupported_content_type");
        }
        let body = &req.body[..core::cmp::min(req.content_length, req.body.len())];
        let action = match Form::parse(body).get("action") {
            Some(a) => a,
            None => {
                self.writes_rejected = self.writes_rejected.wrapping_add(1);
                return self.err_json(out_buf, HttpStatus::BadRequest, "missing_action");
            }
        };
        if action == "arm" {
            self.power_armed = true;
            self.power_arm_tick = rt.tick_100ms;
            let mut ack = [0u8; 96];
            let n = {
                let mut j = JsonBuf::new(&mut ack);
                j.raw(b"{\"ok\":true,\"data\":{\"status\":\"armed\",\"valid_for_s\":")
                    .u32((POWER_ARM_TICKS / 10) as u32)
                    .raw(b"}}");
                j.finish().ok_or("JSON power arm ack overflow")?
            };
            return Self::ok_json(out_buf, &ack[..n]);
        }
        let code = match action {
            "restart" => POWER_RESTART,
            "shutdown" => POWER_SHUTDOWN,
            _ => {
                self.writes_rejected = self.writes_rejected.wrapping_add(1);
                return self.err_json(out_buf, HttpStatus::BadRequest, "bad_power_action");
            }
        };
        if !self.power_armed(rt.tick_100ms) {
            self.writes_rejected = self.writes_rejected.wrapping_add(1);
            self.power_armed = false;
            return self.err_json(out_buf, HttpStatus::Forbidden, "power_not_armed");
        }
        self.power_armed = false;
        self.power_pending = Some(code);
        let mut ack = [0u8; 128];
        let n = {
            let mut j = JsonBuf::new(&mut ack);
            j.raw(b"{\"ok\":true,\"data\":{\"status\":\"scheduled\",\"action\":")
                .quoted(action)
                .raw(b",\"exec_in_ms\":")
                .u32(POWER_EXEC_MS as u32)
                .raw(b"}}");
            j.finish().ok_or("JSON power ack overflow")?
        };
        Self::ok_json(out_buf, &ack[..n])
    }

    /// `POST /api/v1/auth/login` dengan body form `user=` + `password=`.
    ///
    /// Kegagalan selalu menjawab dengan kode yang sama (`invalid_credentials`),
    /// tanpa membedakan nama atau sandi, dan tanpa pernah mengulang input.
    /// Penalti kuota hanya mengenai alamat sumber yang gagal, sehingga satu
    /// penebak tidak bisa mengunci administrator lain.
    fn handle_login(
        &mut self,
        req: &HttpRequest,
        out_buf: &mut [u8],
        rt: &WebRuntime,
    ) -> Result<usize, &'static str> {
        if !self
            .rate_limiter
            .allow(&rt.peer_ip, RATE_AUTH, rt.tick_100ms)
        {
            return self.err_json(out_buf, HttpStatus::TooManyRequests, "rate_limit_exceeded");
        }
        if !self.same_origin(req, rt) {
            return self.err_json(out_buf, HttpStatus::Forbidden, "host_mismatch");
        }
        let ct = match req.content_type {
            Some(c) => c,
            None => return self.err_json(out_buf, HttpStatus::BadRequest, "missing_content_type"),
        };
        if !starts_with_ci(ct, "application/x-www-form-urlencoded") {
            return self.err_json(out_buf, HttpStatus::BadRequest, "unsupported_content_type");
        }
        let body = &req.body[..core::cmp::min(req.content_length, req.body.len())];
        let form = Form::parse(body);
        let raw_user = match form.get("user") {
            Some(v) => v,
            None => return self.err_json(out_buf, HttpStatus::BadRequest, "missing_credentials"),
        };
        let raw_pw = match form.get("password") {
            Some(v) => v,
            None => return self.err_json(out_buf, HttpStatus::BadRequest, "missing_credentials"),
        };
        let mut ubuf = [0u8; RJCFG_AUTH_USER_MAX];
        let mut pbuf = [0u8; AUTH_PASSWORD_MAX];
        let user = match pct_decode(raw_user, &mut ubuf) {
            Some(u) => u,
            None => return self.err_json(out_buf, HttpStatus::BadRequest, "malformed_credentials"),
        };
        let pw = match pct_decode(raw_pw, &mut pbuf) {
            Some(p) => p,
            None => return self.err_json(out_buf, HttpStatus::BadRequest, "malformed_credentials"),
        };
        let tick = rt.tick_100ms;
        match self.auth.login(user, pw.as_bytes(), tick) {
            Ok((idx, token, role)) => {
                let (name_raw, name_len) = self.auth.user_name(idx);
                let name = core::str::from_utf8(&name_raw[..name_len]).unwrap_or("");
                let mut hexbuf = [0u8; AUTH_TOKEN_BYTES * 2];
                let token_hex = match hex::encode(&token, &mut hexbuf) {
                    Some(h) => h,
                    None => return Err("token hex overflow"),
                };
                // Batas umur yang dilaporkan = jendela idle kebijakan, dihitung
                // dari konstanta yang sama dipakai `AuthStore::purge`.
                let mut ack = [0u8; 256];
                let n = {
                    let mut j = JsonBuf::new(&mut ack);
                    j.raw(b"{\"ok\":true,\"data\":{\"token\":")
                        .quoted(token_hex)
                        .raw(b",\"role\":")
                        .quoted(role.name())
                        .raw(b",\"user\":")
                        .quoted(name)
                        .raw(b",\"session_expires_s\":")
                        .u32((AUTH_IDLE_TICKS / 10) as u32)
                        .raw(b"}}");
                    j.finish().ok_or("JSON login ack overflow")?
                };
                Self::ok_json(out_buf, &ack[..n])
            }
            Err(e) => {
                self.rate_limiter.penalize(&rt.peer_ip, RATE_AUTH, tick);
                let code = if e == AuthError::NoCredentials {
                    "no_credentials"
                } else {
                    "invalid_credentials"
                };
                self.err_json(out_buf, HttpStatus::Unauthorized, code)
            }
        }
    }

    /// `POST /api/v1/auth/logout`: cabut sesi pemanggil dan habiskan jendela
    /// arming daya, supaya aksi daya tidak pernah hidup lebih lama daripada
    /// sesi yang memicunya.
    fn handle_logout(
        &mut self,
        req: &HttpRequest,
        out_buf: &mut [u8],
    ) -> Result<usize, &'static str> {
        self.auth.logout(req.authorization);
        self.power_armed = false;
        let mut ack = [0u8; 96];
        let n = {
            let mut j = JsonBuf::new(&mut ack);
            j.raw(b"{\"ok\":true,\"data\":{\"status\":\"logged_out\"}}");
            j.finish().ok_or("JSON logout ack overflow")?
        };
        Self::ok_json(out_buf, &ack[..n])
    }

    /// `GET /api/v1/auth/status` — satu-satunya jalur API yang boleh anonim,
    /// khusus untuk memutuskan kartu masuk. Respons hanya memuat hasil probe
    /// header pengirim: tidak ada daftar akun, nama pengguna, maupun pencacah
    /// kegagalan.
    fn handle_auth_status(
        &self,
        req: &HttpRequest,
        out_buf: &mut [u8],
        tick: u64,
    ) -> Result<usize, &'static str> {
        let (authed, role, left) = match self.auth.peek(req.authorization, tick) {
            Some((_, r)) => (
                true,
                r.name(),
                self.auth.session_left_s(req.authorization, tick),
            ),
            None => (false, "-", 0),
        };
        let mut body = [0u8; 160];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"authenticated\":")
                .bool(authed)
                .raw(b",\"role\":")
                .quoted(role)
                .raw(b",\"bootstrap_required\":")
                .bool(!self.auth.has_users())
                .raw(b",\"session_expires_s\":")
                .u32(left)
                .raw(b"}}");
            j.finish().ok_or("JSON auth status overflow")?
        };
        Self::ok_json(out_buf, &body[..n])
    }

    /// `GET /api/v1/auth/users`: hanya nama, role, dan iterasi PBKDF2 per akun.
    /// Salt, hash, dan kata sandi tidak pernah meninggalkan lapisan ini, dan
    /// jalur ini menuntut role admin karena daftar nama adalah langkah awal
    /// menuju percobaan tebak sandi.
    fn handle_users_list(&self, out: &mut [u8]) -> Result<usize, &'static str> {
        let mut body = [0u8; 512];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":[");
            let mut first = true;
            for i in 0..AUTH_USERS_MAX {
                let (raw, len) = self.auth.user_name(i);
                if len == 0 {
                    continue;
                }
                let name = core::str::from_utf8(&raw[..len]).unwrap_or("?");
                let role = self.auth.user_role(i).map_or("?", |r| r.name());
                if !first {
                    j.raw(b",");
                }
                first = false;
                j.raw(b"{\"user\":")
                    .quoted(name)
                    .raw(b",\"role\":")
                    .quoted(role)
                    .raw(b",\"rounds\":")
                    .u32(self.auth.user_rounds(i))
                    .raw(b"}");
            }
            j.raw(b"]}");
            j.finish().ok_or("JSON users list overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// `POST /api/v1/auth/users` (khusus admin): `action=set` memasang atau
    /// mengganti akun, `action=del` menghapus akun, `action=revoke` memutus
    /// seluruh sesi tanpa menyentuh kredensial.
    ///
    /// Kata sandi hanya hidup di dalam fungsi ini: dipakai sekali untuk
    /// menurunkan PBKDF2 lalu buffer penampungnya dinolkan. Tidak ada jalur
    /// yang mengembalikan, mencatat, atau menyimpan teks jelas.
    fn handle_users_post(
        &mut self,
        req: &HttpRequest,
        out_buf: &mut [u8],
    ) -> Result<usize, &'static str> {
        let ct = match req.content_type {
            Some(c) => c,
            None => return self.err_json(out_buf, HttpStatus::BadRequest, "missing_content_type"),
        };
        if !starts_with_ci(ct, "application/x-www-form-urlencoded") {
            return self.err_json(out_buf, HttpStatus::BadRequest, "unsupported_content_type");
        }
        let body = &req.body[..core::cmp::min(req.content_length, req.body.len())];
        let form = Form::parse(body);
        let action = match form.get("action") {
            Some(a) => a,
            None => return self.err_json(out_buf, HttpStatus::BadRequest, "missing_action"),
        };
        let raw_user = match form.get("user") {
            Some(v) => v,
            None => return self.err_json(out_buf, HttpStatus::BadRequest, "missing_user"),
        };
        let mut ubuf = [0u8; RJCFG_AUTH_USER_MAX];
        let user = match pct_decode(raw_user, &mut ubuf) {
            Some(u) => u,
            None => return self.err_json(out_buf, HttpStatus::BadRequest, "malformed_user"),
        };
        if action == "set" {
            let role = match form.get("role") {
                Some("admin") => Role::Admin,
                Some("viewer") => Role::Viewer,
                _ => return self.err_json(out_buf, HttpStatus::BadRequest, "bad_role"),
            };
            let raw_pw = match form.get("password") {
                Some(v) => v,
                None => return self.err_json(out_buf, HttpStatus::BadRequest, "missing_password"),
            };
            let mut pbuf = [0u8; AUTH_PASSWORD_MAX];
            let pw = match pct_decode(raw_pw, &mut pbuf) {
                Some(p) => p,
                None => {
                    return self.err_json(out_buf, HttpStatus::BadRequest, "malformed_password")
                }
            };
            if pw.len() < AUTH_PASSWORD_MIN || pw.len() > AUTH_PASSWORD_MAX {
                // Kebijakan panjang disebut eksplisit karena operator sedang
                // menulis sandi barunya sendiri, bukan menebak milik orang lain.
                return self.err_json(out_buf, HttpStatus::BadRequest, "password_too_short");
            }
            let salt = self.auth.new_salt();
            let hash = AuthStore::derive(pw.as_bytes(), &salt, AUTH_DEFAULT_ROUNDS);
            let res = self
                .auth
                .set_user(user, role, &salt, AUTH_DEFAULT_ROUNDS, &hash);
            pbuf.fill(0);
            if res.is_err() {
                self.writes_rejected = self.writes_rejected.wrapping_add(1);
                return self.err_json(out_buf, HttpStatus::BadRequest, "bad_user_or_table_full");
            }
            self.auth_dirty = true;
            return Self::users_ack(out_buf, "set", user, role.name());
        }
        if action == "del" {
            let idx = match self.auth.find_user(user) {
                Some(i) => i,
                None => return self.err_json(out_buf, HttpStatus::BadRequest, "unknown_user"),
            };
            // Pemulihan lewat konsol serial selalu ada, tetapi menghapus akun
            // terakhir atau admin terakhir tetap ditolak: WebUI tidak boleh
            // mengunci pemiliknya keluar dari sistemnya sendiri.
            if self.auth.user_count() <= 1 {
                return self.err_json(out_buf, HttpStatus::BadRequest, "last_account");
            }
            if self.auth.user_role(idx) == Some(Role::Admin)
                && self.auth.count_role(Role::Admin) <= 1
            {
                return self.err_json(out_buf, HttpStatus::BadRequest, "last_admin");
            }
            if !self.auth.remove_user(user) {
                return self.err_json(out_buf, HttpStatus::BadRequest, "unknown_user");
            }
            self.auth_dirty = true;
            return Self::users_ack(out_buf, "del", user, "-");
        }
        if action == "revoke" {
            let left = self.auth.revoke_all();
            let mut ack = [0u8; 128];
            let n = {
                let mut j = JsonBuf::new(&mut ack);
                j.raw(b"{\"ok\":true,\"data\":{\"status\":\"applied\",\"action\":")
                    .quoted("revoke")
                    .raw(b",\"sessions_revoked\":")
                    .u32(left)
                    .raw(b"}}");
                j.finish().ok_or("JSON revoke ack overflow")?
            };
            return Self::ok_json(out_buf, &ack[..n]);
        }
        self.err_json(out_buf, HttpStatus::BadRequest, "bad_user_action")
    }

    /// ACK seragam perubahan akun: aksi, nama sasaran, role, dan janji
    /// persistensi. Nilai `persist` hanya mengklaim "masuk antrean" — nasib
    /// sebenarnya di RJCFG01 dibaca klien dari `/api/v1/writes/last`.
    fn users_ack(
        out: &mut [u8],
        action: &str,
        user: &str,
        role: &str,
    ) -> Result<usize, &'static str> {
        let mut ack = [0u8; 192];
        let n = {
            let mut j = JsonBuf::new(&mut ack);
            j.raw(b"{\"ok\":true,\"data\":{\"status\":\"applied\",\"action\":")
                .quoted(action)
                .raw(b",\"user\":")
                .quoted(user)
                .raw(b",\"role\":")
                .quoted(role)
                .raw(b",\"persist\":\"queued\"}}");
            j.finish().ok_or("JSON users ack overflow")?
        };
        Self::ok_json(out, &ack[..n])
    }

    /// Respons galat autentikasi: 401 untuk masalah identitas (dengan tantangan
    /// skema), 403 untuk kewenangan. Badan respons hanya memuat kode stabil.
    fn auth_err(out_buf: &mut [u8], e: AuthError) -> Result<usize, &'static str> {
        let status = if e.status_401() {
            HttpStatus::Unauthorized
        } else {
            HttpStatus::Forbidden
        };
        let mut body = [0u8; 64];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":false,\"error\":")
                .quoted(e.code())
                .raw(b"}");
            j.finish().ok_or("JSON auth error overflow")?
        };
        let builder =
            HttpResponseBuilder::new(out_buf, status).with_content_type("application/json");
        let builder = if status == HttpStatus::Unauthorized {
            builder.with_www_authenticate(AUTH_CHALLENGE)
        } else {
            builder
        };
        builder.finish_with_body(&body[..n])
    }

    fn err_json(
        &self,
        out_buf: &mut [u8],
        status: HttpStatus,
        code: &'static str,
    ) -> Result<usize, &'static str> {
        let mut body = [0u8; 96];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":false,\"error\":").quoted(code).raw(b"}");
            j.finish().ok_or("JSON error overflow")?
        };
        let builder =
            HttpResponseBuilder::new(out_buf, status).with_content_type("application/json");
        builder.finish_with_body(&body[..n])
    }

    /// Proteksi CSRF/same-origin sederhana: `Host` wajib sama dengan salah satu
    /// alamat lokal yang benar-benar melayani (IFACE-01: eth0 WAN dan eth1 LAN)
    /// atau hostname router (dengan atau tanpa port). Tanpa Host -> ditolak.
    /// Himpunan alamat diambil dari `rt.local`; entri mati (ifindex 0) tidak
    /// pernah diterima, jadi alamat yang belum dikonfigurasi tidak membuka
    /// jalur tulis.
    fn same_origin(&self, req: &HttpRequest, rt: &WebRuntime) -> bool {
        let host = match req.host {
            Some(h) => h,
            None => return false,
        };
        let (host_name, port) = split_host_port(host);
        if let Some(p) = port {
            if p != rt.http_port {
                return false;
            }
        }
        if !rt.hostname.is_empty() && host_name == rt.hostname {
            return true;
        }
        let n = core::cmp::min(rt.local_count as usize, WEB_LOCAL_MAX);
        let mut i = 0usize;
        while i < n {
            let entry = rt.local[i];
            if entry.ifindex != 0 {
                let mut ipbuf = [0u8; 15];
                let ip_len = {
                    let mut j = JsonBuf::new(&mut ipbuf);
                    j.ip(entry.ip);
                    j.finish().unwrap_or(0)
                };
                if ip_len > 0 && host_name.as_bytes() == &ipbuf[..ip_len] {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    fn ok_json(out_buf: &mut [u8], body: &[u8]) -> Result<usize, &'static str> {
        let builder =
            HttpResponseBuilder::new(out_buf, HttpStatus::Ok).with_content_type("application/json");
        builder.finish_with_body(body)
    }

    fn handle_system(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 512];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"distro\":\"Rinjani 1.0.0\",")
                .raw(b"\"kernel\":\"ASL Core Kernel v1.0.0\",\"hostname\":")
                .quoted(rt.hostname)
                .raw(b",\"arch\":\"x86_64\",\"uptime_ms\":")
                .u64(rt.uptime_ms)
                .raw(b",\"wan_up\":")
                .bool(rt.wan_up)
                .raw(b",\"wan_source\":")
                .quoted(rt.wan_source)
                .raw(b",\"requests_served\":")
                .u32(rt.requests_served)
                .raw(b",\"status\":");
            j.quoted(if rt.wan_up { "operational" } else { "wan-down" });
            j.raw(b"}}");
            j.finish().ok_or("JSON system overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// Inventaris antarmuka REALTIME: hanya NIC yang benar-benar terdeteksi
    /// kernel yang muncul sebagai baris. Tidak ada baris `absent` placeholder;
    /// satu NIC berarti satu baris.
    fn handle_interfaces(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 1280];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":[");
            if rt.wan_present {
                j.raw(b"{\"name\":").quoted(rt.wan_name);
                j.raw(b",\"role\":").quoted("wan");
                j.raw(b",\"type\":").quoted("ethernet");
                j.raw(b",\"mac\":").quoted_mac(rt.wan_mac);
                j.raw(b",\"ip\":").quoted_ip(rt.wan_ip);
                j.raw(b",\"netmask\":").quoted_ip(rt.wan_netmask);
                j.raw(b",\"netmask_src\":").quoted(rt.wan_netmask_src);
                j.raw(b",\"gateway\":").quoted_ip(rt.wan_gw);
                j.raw(b",\"gateway_src\":").quoted(rt.wan_gw_src);
                j.raw(b",\"dns1\":").quoted_ip(rt.wan_dns1);
                j.raw(b",\"dns2\":").quoted_ip(rt.wan_dns2);
                j.raw(b",\"dns_src\":").quoted(rt.wan_dns_src);
                j.raw(b",\"source\":").quoted(rt.wan_source);
                j.raw(b",\"config_mode\":");
                j.quoted(if rt.wan_config_static {
                    "static"
                } else {
                    "dhcp"
                });
                j.raw(b",\"status\":")
                    .quoted(if rt.wan_up { "up" } else { "down" });
                j.raw(b",\"running\":").bool(rt.wan_up);
                j.raw(b",\"rx_packets\":")
                    .u32(rt.wan_rx)
                    .raw(b",\"tx_packets\":")
                    .u32(rt.wan_tx)
                    .raw(b",\"editable\":")
                    .bool(true)
                    .raw(b"}");
            }
            if rt.lan_present {
                // Pemisah hanya bila baris WAN gerçekten tercetak.
                if rt.wan_present {
                    j.raw(b",");
                }
                // Kernel belum punya sensor carrier link, jadi status LAN dibaca
                // dari fakta yang memang ada: alamat IPv4 yang terpasang pada
                // interface itu. Literal "up" akan melaporkan interface tanpa
                // alamat sebagai berjalan.
                let lan_up = rt.lan_ip != [0, 0, 0, 0];
                j.raw(b"{\"name\":").quoted(rt.lan_name);
                j.raw(b",\"role\":").quoted("lan");
                j.raw(b",\"type\":").quoted("ethernet");
                j.raw(b",\"mac\":").quoted_mac(rt.lan_mac);
                j.raw(b",\"ip\":").quoted_ip(rt.lan_ip);
                // Netmask/gateway sisi LAN host-only tidak dilaporkan kernel:
                // field sengaja tidak ada, ditampilkan sebagai em-dash.
                j.raw(b",\"source\":").quoted("local");
                j.raw(b",\"config_mode\":").quoted("local");
                j.raw(b",\"status\":")
                    .quoted(if lan_up { "up" } else { "down" });
                j.raw(b",\"running\":").bool(lan_up);
                j.raw(b",\"rx_packets\":")
                    .u32(rt.lan_rx)
                    .raw(b",\"tx_packets\":")
                    .u32(rt.lan_tx)
                    .raw(b",\"editable\":")
                    .bool(false)
                    .raw(b"}");
            }
            j.raw(b"]}");
            j.finish().ok_or("JSON interfaces overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    fn handle_firewall_rules(
        &self,
        out: &mut [u8],
        rt: &WebRuntime,
    ) -> Result<usize, &'static str> {
        let mut body = [0u8; 512];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"default_policy\":")
                .quoted(rt.default_policy)
                .raw(b",\"tcam_rules\":")
                .u32(rt.tcam_rules)
                .raw(b",\"dnat_rules\":")
                .u32(rt.dnat_rules)
                // Perlindungan hanya boleh dilaporkan bila classifier benar-benar
                // ditanyakan pada frame yang lewat; mesin TCAM yang berdiri
                // sendiri bukan tembok.
                .raw(b",\"wan_protected\":")
                .bool(rt.wan_up && rt.firewall_wired)
                .raw(b",\"rules_enforced\":")
                .bool(rt.firewall_wired)
                .raw(b",\"datapath\":");
            j.quoted(if rt.firewall_wired {
                "wired"
            } else {
                "engine-only"
            });
            j.raw(b",\"http_port_open\":")
                .bool(rt.http_listening)
                .raw(b",\"http_port\":")
                .u32(rt.http_port as u32)
                .raw(b"}}");
            j.finish().ok_or("JSON firewall overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// Ringkasan NAT: angka dari `NatEngine` kernel, plus pengakuan jujur apakah
    /// mesin itu benar-benar dipanggil jalur forwarding.
    fn handle_nat_summary(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 512];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"masquerade_active\":")
                .bool(rt.wan_up && rt.wan_ip != [0, 0, 0, 0])
                .raw(b",\"masquerade_ip\":")
                .quoted_ip(rt.wan_ip)
                .raw(b",\"forwarding_wired\":")
                .bool(rt.nat_forwarding_wired)
                .raw(b",\"forwarding\":");
            j.quoted(if rt.nat_forwarding_wired {
                "wired"
            } else {
                "engine-only"
            });
            j.raw(b",\"active_sessions\":")
                .u32(rt.nat_session_count)
                .raw(b",\"snapshot_rows\":")
                .u32(rt.nat_sessions.len() as u32)
                .raw(b",\"capacity\":")
                .u32(rt.nat_capacity)
                .raw(b",\"table_full_drops\":")
                .u32(rt.nat_table_full_drops)
                .raw(b",\"dnat_rules\":")
                .u32(rt.dnat_rules)
                .raw(b"}}");
            j.finish().ok_or("JSON nat overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    fn handle_nat_rules(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 1024];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":[");
            for (i, r) in rt.dnat_list.iter().enumerate() {
                if i > 0 {
                    j.raw(b",");
                }
                j.raw(b"{\"wan_port\":")
                    .u32(r.wan_port as u32)
                    .raw(b",\"protocol\":")
                    .quoted(r.protocol)
                    .raw(b",\"target_ip\":")
                    .quoted_ip(r.target_ip)
                    .raw(b",\"target_port\":")
                    .u32(r.target_port as u32)
                    .raw(b",\"active\":")
                    .bool(r.active);
                j.raw(b"}");
            }
            j.raw(b"]}");
            j.finish().ok_or("JSON nat rules overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    fn handle_nat_conntrack(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 2048];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":[");
            for (i, c) in rt.nat_sessions.iter().enumerate() {
                if i > 0 {
                    j.raw(b",");
                }
                j.raw(b"{\"protocol\":").quoted(c.protocol);
                j.raw(b",\"src\":").quoted_ip(c.src_ip);
                j.raw(b",\"src_port\":").u32(c.src_port as u32);
                j.raw(b",\"dst\":").quoted_ip(c.dst_ip);
                j.raw(b",\"dst_port\":").u32(c.dst_port as u32);
                j.raw(b",\"translated_port\":")
                    .u32(c.trans_port as u32)
                    .raw(b",\"fwd_packets\":")
                    .u32(c.fwd_packets)
                    .raw(b",\"rev_packets\":")
                    .u32(c.rev_packets);
                j.raw(b"}");
            }
            j.raw(b"]}");
            j.finish().ok_or("JSON nat conntrack overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// `GET /api/v1/arp` — tabel tetangga ARP. Setiap baris adalah pasangan
    /// (IP, MAC) yang benar-benar terlihat pada field *sender* frame ARP yang
    /// masuk ke kernel; endpoint ini tidak pernah menambahkan baris sendiri.
    /// Umur dihitung dari tick terakhir penyegaran supaya klien bisa menilai
    /// kesegaran tanpa mengarang status "reachable".
    fn handle_arp(
        &self,
        out: &mut [u8],
        rt: &WebRuntime,
        current_tick: u64,
    ) -> Result<usize, &'static str> {
        let mut body = [0u8; 3072];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":[");
            for (i, a) in rt.arp_list.iter().enumerate() {
                if i > 0 {
                    j.raw(b",");
                }
                let iface = if a.ifindex == 1 {
                    rt.lan_name
                } else {
                    rt.wan_name
                };
                let age_ticks = current_tick.saturating_sub(a.last_tick);
                j.raw(b"{\"iface\":").quoted(iface);
                j.raw(b",\"ip\":").quoted_ip(a.ip);
                j.raw(b",\"mac\":").quoted_mac(a.mac);
                j.raw(b",\"type\":\"D\"");
                j.raw(b",\"source\":")
                    .quoted(if a.reply { "reply" } else { "request" });
                j.raw(b",\"seen\":").u32(a.seen);
                j.raw(b",\"age_s\":").u64(age_ticks / 10);
                j.raw(b"}");
            }
            j.raw(b"]}");
            j.finish().ok_or("JSON arp table overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// Statistik tabel ARP (kapasitas, penggusuran, frame yang ditolak).
    /// Dipisahkan dari daftar baris supaya kapasitas yang habis tidak bisa
    /// tersembunyi di balik tabel yang terlihat penuh-penuh wajar.
    fn handle_arp_stats(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 256];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"entries\":")
                .u32(rt.arp_count)
                .raw(b",\"capacity\":")
                .u32(rt.arp_capacity)
                .raw(b",\"timeout_s\":")
                .u32(rt.arp_timeout_s)
                .raw(b",\"evictions\":")
                .u32(rt.arp_evictions)
                .raw(b",\"rejected\":")
                .u32(rt.arp_rejected)
                .raw(b",\"snapshot\":")
                .u32(rt.arp_list.len() as u32)
                .raw(b"}}");
            j.finish().ok_or("JSON arp stats overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// `GET /api/v1/routes` — isi FIB kernel. Baris dihasilkan dari snapshot yang
    /// diisi kernel dari `RouteEntry` aktif; endpoint ini tidak menambah,
    /// menggabung, atau mengarang rute. Nama antarmuka diterjemahkan dari nomor
    /// antarmuka kernel (`IF_WAN`/`IF_LAN`) memakai nama yang dilaporkan kernel,
    /// dan rute tanpa antarmuka keluar tampil sebagai strip, bukan "eth0".
    fn handle_routes(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 3072];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":[");
            for (i, r) in rt.route_list.iter().enumerate() {
                if i > 0 {
                    j.raw(b",");
                }
                j.raw(b"{\"dst\":")
                    .quoted_ip(r.network)
                    .raw(b",\"prefix\":")
                    .u32(r.prefix_len as u32)
                    .raw(b",\"gw\":")
                    .quoted_ip(r.gateway)
                    .raw(b",\"iface\":")
                    .quoted(Self::route_iface(r.ifindex, rt))
                    .raw(b",\"metric\":")
                    .u32(r.metric)
                    .raw(b",\"type\":")
                    .quoted(Self::route_kind(r.kind));
                j.raw(b"}");
            }
            j.raw(b"]}");
            j.finish().ok_or("JSON routes table overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// `GET /api/v1/routes/stats` — kapasitas tabel dan berapa kali FIB benar-
    /// benar ditanya pada jalur penerusan. Angka nol sengaja ditampilkan: tanpa
    /// LOOKUP yang nyata, halaman Gateway tidak boleh tampak "aktif".
    fn handle_routes_stats(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 320];
        let mut default_active = false;
        let mut default_gw = [0u8; 4];
        for r in rt.route_list.iter() {
            if r.prefix_len == 0 && r.kind == ROUTE_KIND_STATIC {
                default_active = true;
                default_gw = r.gateway;
            }
        }
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"entries\":")
                .u32(rt.route_count)
                .raw(b",\"capacity\":")
                .u32(rt.route_capacity)
                .raw(b",\"lookups\":")
                .u32(rt.route_lookups)
                .raw(b",\"forwards\":")
                .u32(rt.route_forwards)
                .raw(b",\"drops\":")
                .u32(rt.route_drops)
                .raw(b",\"default_active\":")
                .bool(default_active)
                .raw(b",\"default_gw\":")
                .quoted_ip(default_gw)
                .raw(b",\"snapshot\":")
                .u32(rt.route_list.len() as u32)
                .raw(b"}}");
            j.finish().ok_or("JSON routes stats overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// `GET /api/v1/dns` — resolver yang dibawa lease DHCP DAN upstream yang
    /// kini dipegang mesin proksi. Keduanya dilaporkan terpisah supaya operator
    /// melihat penyaluran lease -> mesin, bukan hanya nilai yang tersimpan.
    /// Tidak ada resolver publik cadangan: tanpa opsi 6 keduanya keluar nol dan
    /// `resolver_src` berhenti pada "none". `serving` mengikuti kenyataan jalur
    /// terima query, jadi angka cache boleh nol meski mesinnya berdiri.
    fn handle_dns(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 768];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"config_mode\":");
            j.quoted(if rt.wan_config_static {
                "static"
            } else {
                "dhcp"
            });
            j.raw(b",\"lease_source\":")
                .quoted(rt.wan_source)
                .raw(b",\"resolver_1\":")
                .quoted_ip(rt.wan_dns1)
                .raw(b",\"resolver_2\":")
                .quoted_ip(rt.wan_dns2)
                .raw(b",\"resolver_src\":")
                .quoted(rt.wan_dns_src)
                .raw(b",\"from_dhcp\":")
                .bool(rt.wan_dns_src.starts_with("option-6-"))
                .raw(b",\"proxy_upstream\":")
                .quoted_ip(rt.dns_upstream)
                .raw(b",\"proxy_upstream_ready\":")
                .bool(rt.dns_upstream != [0, 0, 0, 0])
                .raw(b",\"serving\":")
                .bool(rt.dns_active)
                .raw(b",\"cache_hits\":")
                .u32(rt.dns_cache_hits)
                .raw(b",\"cache_misses\":")
                .u32(rt.dns_cache_misses)
                .raw(b",\"cache_entries\":")
                .u32(rt.dns_cache_entries)
                .raw(b",\"cache_capacity\":")
                .u32(rt.dns_cache_capacity)
                .raw(b",\"static_hosts\":")
                .u32(rt.dns_host_count)
                .raw(b",\"hosts_capacity\":")
                .u32(rt.dns_host_capacity);
            j.raw(b"}}");
            j.finish().ok_or("JSON dns overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// Nomor antarmuka kernel -> nama antarmuka yang dilaporkan kernel. Nomor
    /// tak dikenal (misalnya antarmuka yang tidak terikat) menjadi strip agar
    /// UI tidak pernah menampilkan nama yang tidak ada di mesin.
    fn route_iface<'a>(ifindex: u8, rt: &WebRuntime<'a>) -> &'a str {
        match ifindex {
            1 => rt.wan_name,
            2 => rt.lan_name,
            _ => "-",
        }
    }

    fn route_kind(kind: u8) -> &'static str {
        match kind {
            ROUTE_KIND_CONNECTED => "connected",
            ROUTE_KIND_STATIC => "static",
            ROUTE_KIND_LOCAL => "local",
            _ => "blackhole",
        }
    }

    fn handle_services(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 768];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"dhcp_client\":")
                .bool(rt.dhcp_client_active)
                .raw(b",\"wan_mode\":");
            j.quoted(if rt.wan_config_static {
                "static"
            } else {
                "dhcp"
            });
            j.raw(b",\"http_server\":")
                .bool(rt.http_listening)
                .raw(b",\"mdns_responder\":")
                .bool(rt.mdns_active)
                .raw(b",\"dns_proxy\":")
                .bool(rt.dns_active)
                .raw(b",\"serial_cli\":")
                .bool(rt.serial_cli_active)
                .raw(b",\"http_port\":")
                .u32(rt.http_port as u32)
                .raw(b",\"tcp_sessions_active\":")
                .u32(rt.sessions_active)
                .raw(b",\"write_queue_depth\":")
                .u32(self.write_queue_len as u32)
                .raw(b",\"writes_rejected\":")
                .u32(self.writes_rejected)
                .raw(b",\"power_armed\":")
                .bool(self.power_armed(rt.tick_100ms))
                .raw(b",\"power_arm_left_s\":")
                .u32(self.power_arm_left_s(rt.tick_100ms))
                .raw(b",\"power_pending\":")
                .bool(self.power_pending.is_some())
                .raw(b",\"http_bind_ip\":")
                .quoted_ip(rt.wan_ip)
                .raw(b"}}");
            j.finish().ok_or("JSON services overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// Status perintah tulis terakhir. POST hanya menjanjikan "masuk antrean";
    /// hasil sebenarnya (diterapkan/ditolak kernel, plus nasib persistensi
    /// RJCFG01) baru ada setelah poll loop mengambil antrean, jadi modal WebUI
    /// membaca endpoint ini alih-alih menebak dari respons POST.
    fn handle_write_status(&self, out: &mut [u8], rt: &WebRuntime) -> Result<usize, &'static str> {
        let mut body = [0u8; 256];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"applied\":")
                .bool(!rt.last_write_report.is_empty())
                .raw(b",\"queue_depth\":")
                .u32(self.write_queue_len as u32)
                .raw(b",\"ok\":")
                .bool(rt.last_write_ok)
                .raw(b",\"report\":")
                .quoted(rt.last_write_report)
                .raw(b"}}");
            j.finish().ok_or("JSON write status overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    /// Sisi nirkabel build ini: TIDAK ada driver radio. Endpoint mengembalikan
    /// nol perangkat secara eksplisit agar UI tidak pernah menampilkan baris
    /// karangan; status `operational` sengaja `false` (bukan klaim crate marker).
    fn handle_wireless(&self, out: &mut [u8]) -> Result<usize, &'static str> {
        let mut body = [0u8; 256];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"radios\":0,\"stations\":0,\"operational\":")
                .bool(false)
                .raw(b",\"reason\":")
                .quoted("no wireless driver in this build")
                .raw(b"}}");
            j.finish().ok_or("JSON wireless overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    fn handle_wireless_devices(&self, out: &mut [u8]) -> Result<usize, &'static str> {
        Self::ok_json(out, b"{\"ok\":true,\"data\":[]}")
    }

    /// vOlt: crate `asl-router-olt-carrier` / `asl-router-gpon-ont` hanyalah
    /// marker 40 baris dengan `is_operational()` bernilai tetap `true`. Laporan
    /// ini tidak menelan klaim itu: ONU = 0 dan scheduler belum terhubung.
    fn handle_olt(&self, out: &mut [u8]) -> Result<usize, &'static str> {
        let mut body = [0u8; 384];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ok\":true,\"data\":{\"ons\":0,\"ports\":0,\"operational\":")
                .bool(false)
                .raw(b",\"scheduler_connected\":")
                .bool(false)
                .raw(b",\"reason\":")
                .quoted("gpon/olt crates are markers without datapath")
                .raw(b",\"marker_crates\":")
                .quoted("asl-router-gpon-ont, asl-router-olt-carrier")
                .raw(b"}}");
            j.finish().ok_or("JSON olt overflow")?
        };
        Self::ok_json(out, &body[..n])
    }

    fn handle_olt_ons(&self, out: &mut [u8]) -> Result<usize, &'static str> {
        Self::ok_json(out, b"{\"ok\":true,\"data\":[]}")
    }

    fn build_write_cmd(
        &self,
        iface: &str,
        content_type: Option<&str>,
        body: &[u8],
    ) -> Result<WebWriteCmd, &'static str> {
        let ct = content_type.ok_or("missing_content_type")?;
        if !starts_with_ci(ct, "application/x-www-form-urlencoded") {
            return Err("unsupported_content_type");
        }
        if body.len() > WEB_WRITE_BODY_MAX {
            return Err("body_too_large");
        }
        if !iface_name_ok(iface) {
            return Err("target_invalid");
        }
        let form = Form::parse(body);
        let mode_s = form.get("mode").ok_or("missing_mode")?;
        let mode = match mode_s {
            "dhcp" => WAN_MODE_DHCP,
            "static" => WAN_MODE_STATIC,
            _ => return Err("mode_invalid"),
        };
        let new_name = form.get("name").unwrap_or(iface);
        if !iface_name_ok(new_name) {
            return Err("name_invalid");
        }

        let mut cmd = WebWriteCmd {
            target: [0u8; IFACE_NAME_MAX],
            target_len: 0,
            new_name: [0u8; IFACE_NAME_MAX],
            new_name_len: 0,
            mode,
            ip: [0; 4],
            netmask: [0; 4],
            gw: [0; 4],
            persist: form.get("persist") == Some("1"),
        };
        put_name(iface, &mut cmd.target, &mut cmd.target_len);
        put_name(new_name, &mut cmd.new_name, &mut cmd.new_name_len);

        let ip_s = form.get("ip").unwrap_or("");
        let mask_s = form.get("netmask").unwrap_or("");
        let gw_s = form.get("gateway").unwrap_or("");

        if mode == WAN_MODE_DHCP {
            // Aturan DHCP vs statik: mode DHCP tidak boleh membawa alamat
            // tulisan tangan; kalau operator ingin alamat tetap, mode statik.
            if !ip_s.is_empty() || !mask_s.is_empty() || !gw_s.is_empty() {
                return Err("dhcp_rejects_manual_address");
            }
            return Ok(cmd);
        }

        if ip_s.is_empty() {
            return Err("static_requires_ip");
        }
        if mask_s.is_empty() {
            return Err("static_requires_netmask");
        }
        let ip = parse_dotted(ip_s).ok_or("ip_invalid")?;
        let mask = parse_dotted(mask_s).ok_or("netmask_invalid")?;
        let prefix = mask_prefix(mask).ok_or("netmask_not_contiguous")?;
        if !(8..=30).contains(&prefix) {
            return Err("netmask_prefix_out_of_range");
        }
        if is_reserved(ip) {
            return Err("ip_reserved_or_loopback");
        }
        let net = subnet_of(ip, mask);
        if ip == net {
            return Err("ip_is_network_address");
        }
        if ip == broadcast_of(ip, mask) {
            return Err("ip_is_broadcast_address");
        }
        cmd.ip = ip;
        cmd.netmask = mask;
        if !gw_s.is_empty() {
            let gw = parse_dotted(gw_s).ok_or("gateway_invalid")?;
            if is_reserved(gw) {
                return Err("gateway_reserved_or_loopback");
            }
            if subnet_of(gw, mask) != net {
                return Err("gateway_outside_subnet");
            }
            if gw == ip {
                return Err("gateway_equals_own_ip");
            }
            if gw == broadcast_of(ip, mask) {
                return Err("gateway_is_broadcast_address");
            }
            cmd.gw = gw;
        }
        Ok(cmd)
    }
}

// -----------------------------------------------------------------------------
// VALIDATOR MURN (tanpa state, dipakai dispatcher maupun unit test)
// -----------------------------------------------------------------------------

/// Pisahkan `host:port` versi IPv4/label; `None` bila port tidak ada.
fn split_host_port(host: &str) -> (&str, Option<u16>) {
    match host.rsplit_once(':') {
        Some((name, port)) => {
            let mut v: u32 = 0;
            let mut digits = 0usize;
            for b in port.bytes() {
                if !b.is_ascii_digit() {
                    return (host, None);
                }
                v = v.saturating_mul(10).saturating_add((b - b'0') as u32);
                digits += 1;
            }
            if digits == 0 || v > 65535 {
                return (host, None);
            }
            (name, Some(v as u16))
        }
        None => (host, None),
    }
}

fn starts_with_ci(s: &str, prefix: &str) -> bool {
    let (a, b) = (s.as_bytes(), prefix.as_bytes());
    if a.len() < b.len() {
        return false;
    }
    for i in 0..b.len() {
        if a[i].to_ascii_lowercase() != b[i].to_ascii_lowercase() {
            return false;
        }
    }
    true
}

/// Urai "a.b.c.d" menjadi oktet; tolak digit kosong, >3 angka, dan nilai >255.
pub fn parse_dotted(s: &str) -> Option<[u8; 4]> {
    let mut oct = [0u8; 4];
    let mut idx = 0usize;
    let mut cur: u32 = 0;
    let mut digits = 0usize;
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    for i in 0..bytes.len() {
        let b = bytes[i];
        if b == b'.' {
            if digits == 0 || idx >= 4 {
                return None;
            }
            oct[idx] = cur as u8;
            idx += 1;
            cur = 0;
            digits = 0;
        } else if b.is_ascii_digit() {
            if digits == 3 {
                return None;
            }
            cur = cur * 10 + (b - b'0') as u32;
            digits += 1;
            if cur > 255 {
                return None;
            }
        } else {
            return None;
        }
    }
    if digits == 0 || idx != 3 {
        return None;
    }
    oct[3] = cur as u8;
    Some(oct)
}

/// Panjang prefix bila netmask kontigu (mis. 255.255.254.0 => 23), else None.
pub fn mask_prefix(mask: [u8; 4]) -> Option<u8> {
    let bits = u32::from(mask[0]) << 24
        | u32::from(mask[1]) << 16
        | u32::from(mask[2]) << 8
        | u32::from(mask[3]);
    let ones = bits.count_ones() as u8;
    if ones == 0 {
        return None;
    }
    let expected = if ones >= 32 {
        u32::MAX
    } else {
        !(u32::MAX >> ones)
    };
    if bits == expected {
        Some(ones)
    } else {
        None
    }
}

fn to_u32(ip: [u8; 4]) -> u32 {
    u32::from(ip[0]) << 24 | u32::from(ip[1]) << 16 | u32::from(ip[2]) << 8 | u32::from(ip[3])
}

fn from_u32(v: u32) -> [u8; 4] {
    [(v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn subnet_of(ip: [u8; 4], mask: [u8; 4]) -> [u8; 4] {
    from_u32(to_u32(ip) & to_u32(mask))
}

fn broadcast_of(ip: [u8; 4], mask: [u8; 4]) -> [u8; 4] {
    from_u32(to_u32(ip) | !to_u32(mask))
}

/// Alamat yang tidak boleh dipakai host WAN: this-net, loopback, link-local,
/// multicast, dan reserved.
fn is_reserved(ip: [u8; 4]) -> bool {
    ip[0] == 0 || ip[0] == 127 || ip[0] >= 224 || (ip[0] == 169 && ip[1] == 254)
}

fn put_name(src: &str, dst: &mut [u8; IFACE_NAME_MAX], len: &mut u8) {
    for b in dst.iter_mut() {
        *b = 0;
    }
    let n = core::cmp::min(src.len(), IFACE_NAME_MAX);
    dst[..n].copy_from_slice(&src.as_bytes()[..n]);
    *len = n as u8;
}

/// Pengurai body `application/x-www-form-urlencoded` dengan kapasitas tetap.
/// Nilai tidak didekode percent-encoding: karakter `%`/`+` akan ditolak
/// validator field terkait, sehingga kegagalan terdengar, tidak diam-diam.
struct Form<'a> {
    keys: [&'a str; 8],
    vals: [&'a str; 8],
    len: usize,
}

impl<'a> Form<'a> {
    fn parse(body: &'a [u8]) -> Self {
        let mut f = Form {
            keys: [""; 8],
            vals: [""; 8],
            len: 0,
        };
        let text = match core::str::from_utf8(body) {
            Ok(t) => t,
            Err(_) => return f,
        };
        for pair in text.split('&') {
            if pair.is_empty() || f.len >= f.keys.len() {
                continue;
            }
            if let Some((k, v)) = pair.split_once('=') {
                f.keys[f.len] = k;
                f.vals[f.len] = v;
                f.len += 1;
            }
        }
        f
    }

    fn get(&self, key: &str) -> Option<&'a str> {
        for i in 0..self.len {
            if self.keys[i] == key {
                return Some(self.vals[i]);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use auth::{Role, AUTH_DEFAULT_ROUNDS};

    // FIXTURE JALUR DATA. Kata sandi, salt dan token di bawah hanya ada di modul
    // tes dan tidak pernah menjadi kredensial produksi: tidak ada akun bawaan di
    // kode yang di-build (lihat `auth::test_auth_no_default_credentials`).
    //
    // Tokennya bukan angka karangan. `AuthStore::new()` memakai seed boot tetap,
    // dan pemerahan pertama pada tick 0 adalah `SHA-256(seed || 0)` — nilai yang
    // sama dihitung terpisah dengan CPython hashlib. `admin_sess` menuntut
    // kesamaannya, sehingga fixture ini tidak bisa diam-diam menyimpang dari
    // implementasi.
    const FIX_USER: &str = "auditor";
    const FIX_PW: &[u8] = b"rinjani-aud02-kata-sandi-tes";
    const FIX_SALT: &[u8; 16] = b"rj-boot-salt-001";
    const FIX_TOKEN_HEX: &str = "6b28ce8e57533bbca7a3f5ffea484eb65a528ab6796d6853614133a2cb60c23f";
    const FIX_SESSION: &str =
        "Bearer 6b28ce8e57533bbca7a3f5ffea484eb65a528ab6796d6853614133a2cb60c23f";

    /// Dispatcher dengan satu akun admin dan sesi aktif hasil login nyata.
    fn admin_sess() -> ApiDispatcher {
        let mut d = seed_store(Role::Admin);
        let (idx, tok, role) = d
            .auth
            .login(FIX_USER, FIX_PW, 0)
            .expect("login fixture harus sah");
        assert_eq!(role, Role::Admin);
        assert_eq!(d.auth.user_name(idx).1, FIX_USER.len());
        let mut hx = [0u8; 64];
        assert_eq!(
            hex::encode(&tok, &mut hx),
            Some(FIX_TOKEN_HEX),
            "token fixture tidak boleh menyimpang dari pemerahan seed boot"
        );
        d
    }

    /// Dispatcher dengan akun tersimpan tetapi BELUM ada sesi (tabel kosong).
    fn seed_store(role: Role) -> ApiDispatcher {
        let mut d = ApiDispatcher::new();
        let hash = AuthStore::derive(FIX_PW, FIX_SALT, AUTH_DEFAULT_ROUNDS);
        d.auth
            .set_user(FIX_USER, role, FIX_SALT, AUTH_DEFAULT_ROUNDS, &hash)
            .expect("fixture valid");
        d
    }

    /// Dispatcher dengan sesi viewer (role baca-saja).
    fn viewer_sess() -> ApiDispatcher {
        let mut d = seed_store(Role::Viewer);
        assert_eq!(
            d.auth.login(FIX_USER, FIX_PW, 0).map(|t| t.2),
            Ok(Role::Viewer)
        );
        d
    }

    #[test]
    fn test_01_rate_limiter_depletion_and_refill() {
        let mut limiter = TokenBucketRateLimiter::new(3, 1);
        assert!(limiter.check_and_consume(0));
        assert!(limiter.check_and_consume(0));
        assert!(limiter.check_and_consume(0));
        // Exhausted
        assert!(!limiter.check_and_consume(0));

        // Refill after 2 ticks
        assert!(limiter.check_and_consume(2));
    }

    static DNAT: [DnatSnapshot; 1] = [DnatSnapshot {
        wan_port: 7777,
        protocol: "udp",
        target_ip: [192, 0, 2, 11],
        target_port: 7777,
        active: true,
    }];

    /// Dua tetangga contoh, meniru isi `ArpTable` kernel: satu disegarkan oleh
    /// ARP reply di WAN, satu baru pernah terlihat sebagai ARP request di LAN.
    static ARP: [ArpSnapshot; 2] = [
        ArpSnapshot {
            ip: [198, 51, 100, 50],
            mac: [0x08, 0x00, 0x27, 0xAA, 0xBB, 0xCC],
            ifindex: 0,
            reply: true,
            seen: 3,
            last_tick: 990,
        },
        ArpSnapshot {
            ip: [192, 0, 2, 9],
            mac: [0x08, 0x00, 0x27, 0x99, 0x88, 0x77],
            ifindex: 1,
            reply: false,
            seen: 1,
            last_tick: 980,
        },
    ];

    /// Isi FIB seperti pada topologi satu NIC bridged: rute default via gateway
    /// WAN + rute connected subnet WAN. Baris ketiga sengaja tanpa antarmuka
    /// keluar untuk membuktikan penerjemahan nomor antarmuka tidak menebak.
    static ROUTES: [RouteSnapshot; 3] = [
        RouteSnapshot {
            network: [0, 0, 0, 0],
            prefix_len: 0,
            ifindex: 1,
            gateway: [198, 51, 100, 254],
            metric: 100,
            kind: ROUTE_KIND_STATIC,
        },
        RouteSnapshot {
            network: [198, 51, 100, 0],
            prefix_len: 24,
            ifindex: 1,
            gateway: [0, 0, 0, 0],
            metric: 0,
            kind: ROUTE_KIND_CONNECTED,
        },
        RouteSnapshot {
            network: [10, 99, 0, 0],
            prefix_len: 16,
            ifindex: 0,
            gateway: [0, 0, 0, 0],
            metric: 0,
            kind: ROUTE_KIND_DROP,
        },
    ];

    fn rt() -> WebRuntime<'static> {
        WebRuntime {
            hostname: "rinjani-wan01",
            uptime_ms: 123456,
            wan_present: true,
            wan_name: "eth0",
            lan_name: "eth1",
            wan_ip: [198, 51, 100, 77],
            wan_netmask: [255, 255, 255, 0],
            wan_gw: [198, 51, 100, 254],
            wan_gw_src: "option-3-ack",
            wan_netmask_src: "option-1-ack",
            wan_dns1: [198, 51, 100, 53],
            wan_dns2: [0, 0, 0, 0],
            wan_dns_src: "option-6-offer",
            wan_up: true,
            wan_source: "dhcp",
            wan_mac: [0x08, 0x00, 0x27, 0x11, 0x22, 0x33],
            lan_present: false,
            lan_ip: [0; 4],
            lan_mac: [0; 6],
            local: [
                WebLocalAddr {
                    ip: [198, 51, 100, 77],
                    mac: [0x08, 0x00, 0x27, 0x11, 0x22, 0x33],
                    ifindex: 1,
                },
                WebLocalAddr::empty(),
            ],
            local_count: 1,
            http_listening: true,
            http_port: 80,
            dhcp_client_active: true,
            wan_config_static: false,
            dns_active: false,
            dns_upstream: [198, 51, 100, 53],
            dns_cache_hits: 11,
            dns_cache_misses: 5,
            dns_cache_entries: 3,
            dns_cache_capacity: 16,
            dns_host_count: 1,
            dns_host_capacity: 8,
            mdns_active: false,
            serial_cli_active: true,
            tcam_rules: 16,
            dnat_rules: 1,
            default_policy: "deny",
            dnat_list: &DNAT,
            nat_sessions: &[],
            nat_session_count: 0,
            nat_capacity: 128,
            nat_table_full_drops: 0,
            arp_list: &ARP,
            arp_count: 2,
            arp_capacity: 16,
            arp_evictions: 0,
            arp_rejected: 4,
            arp_timeout_s: 20,
            route_list: &ROUTES,
            route_count: 3,
            route_capacity: 64,
            route_lookups: 7,
            route_forwards: 0,
            route_drops: 7,
            nat_forwarding_wired: false,
            firewall_wired: false,
            last_write_ok: false,
            last_write_report: "",
            wan_rx: 41,
            wan_tx: 39,
            lan_rx: 0,
            lan_tx: 0,
            sessions_active: 2,
            requests_served: 7,
            tick_100ms: 0,
            peer_ip: [0, 0, 0, 0],
        }
    }

    /// Leak disengaja: hanya di test host, umur data dibuat 'static supaya
    /// signature dispatcher tidak berubah demi tes.
    fn mk(raw: alloc::string::String) -> HttpRequest<'static> {
        let st: &'static mut str = Box::leak(raw.into_boxed_str());
        HttpRequest::parse(st.as_bytes()).unwrap()
    }

    /// Sisipkan header `Authorization` milik sesi fixture sesudah baris
    /// permintaan, lalu parse lewat jalur parser yang sama seperti klien nyata.
    /// Tes dengan ini menuntut sesi; tes gerbang memakai `req_anon`.
    fn req_sess(raw: &'static [u8]) -> HttpRequest<'static> {
        req_bearer(raw, FIX_TOKEN_HEX)
    }

    /// Permintaan dengan token bearer tertentu (mis. hasil login nyata).
    fn req_bearer(raw: &'static [u8], bearer: &str) -> HttpRequest<'static> {
        let s = core::str::from_utf8(raw).expect("fixture ASCII");
        let (line, rest) = s.split_once("\r\n").expect("baris permintaan wajib ada");
        mk(alloc::format!(
            "{}\r\nAuthorization: Bearer {}\r\n{}",
            line,
            bearer,
            rest
        ))
    }

    /// Permintaan tanpa header `Authorization` (klien anonim).
    fn req_anon(raw: &'static [u8]) -> HttpRequest<'static> {
        HttpRequest::parse(raw).unwrap()
    }

    #[test]
    fn test_02_api_system_endpoint() {
        let mut dispatcher = admin_sess();
        let req = req_sess(b"GET /api/v1/system HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"distro\":\"Rinjani 1.0.0\""));
        assert!(resp.contains("\"hostname\":\"rinjani-wan01\""));
        assert!(resp.contains("\"uptime_ms\":123456"));
        assert!(resp.contains("\"wan_source\":\"dhcp\""));
    }

    #[test]
    fn test_03_api_interfaces_reports_real_runtime_values() {
        let mut dispatcher = admin_sess();
        let req = req_sess(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"name\":\"eth0\""));
        assert!(resp.contains("\"role\":\"wan\""));
        assert!(resp.contains("\"ip\":\"198.51.100.77\""));
        assert!(resp.contains("\"gateway\":\"198.51.100.254\""));
        assert!(resp.contains("\"mac\":\"08:00:27:11:22:33\""));
        assert!(resp.contains("\"rx_packets\":41"));
        assert!(resp.contains("\"config_mode\":\"dhcp\""));
        // Provenance tiap parameter lease: tanpa label, operator tidak bisa
        // membedakan nilai kiriman server dengan simpulan kernel.
        assert!(resp.contains("\"netmask_src\":\"option-1-ack\""));
        assert!(resp.contains("\"gateway_src\":\"option-3-ack\""));
        assert!(resp.contains("\"dns1\":\"198.51.100.53\""));
        assert!(resp.contains("\"dns2\":\"0.0.0.0\""));
        assert!(resp.contains("\"dns_src\":\"option-6-offer\""));
    }

    #[test]
    fn test_03a_api_interfaces_never_invents_absent_nic() {
        // Kontrak baru: satu NIC terdeteksi => tepat satu baris. Tidak ada
        // placeholder eth1 berstatus `absent` lagi.
        let mut dispatcher = admin_sess();
        let mut r = rt();
        r.lan_present = false;
        let req = req_sess(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(
            !resp.contains("eth1"),
            "NIC tak terdeteksi tidak boleh muncul"
        );
        assert!(!resp.contains("absent"));
        assert_eq!(resp.matches("\"name\":").count(), 1);
    }

    #[test]
    fn test_03c_api_interfaces_empty_when_no_nic_present() {
        let mut dispatcher = admin_sess();
        let mut r = rt();
        r.wan_present = false;
        r.lan_present = false;
        let req = req_sess(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(
            resp.contains("\"data\":[]"),
            "tidak ada NIC => array kosong"
        );
    }

    #[test]
    fn test_03d_kernel_interface_names_are_used() {
        let mut dispatcher = admin_sess();
        let mut r = rt();
        r.wan_name = "wan-uplink";
        let req = req_sess(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        assert!(core::str::from_utf8(&buf[..len])
            .unwrap()
            .contains("\"name\":\"wan-uplink\""));
    }

    #[test]
    fn test_03b_api_interfaces_includes_lan_when_present() {
        let mut dispatcher = admin_sess();
        let mut r = rt();
        r.lan_present = true;
        r.lan_ip = [198, 51, 100, 1];
        r.lan_mac = [1, 2, 3, 4, 5, 6];
        let req = req_sess(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"name\":\"eth1\""));
        assert!(resp.contains("\"ip\":\"198.51.100.1\""));
    }

    // Jejak cacat yang ditemukan di browser nyata: kurung penutup objek baris
    // WAN tidak ditulis, JSON.parse browser gagal, dan tabel Interfaces diam
    // tanpa pesan. Semua body GET dijaga seimbang agar kelas cacat ini tidak
    // bisa kembali lolos.
    fn json_pairs_balanced(s: &str) -> Result<(), &'static str> {
        let mut stack = [0u8; 16];
        let mut depth = 0usize;
        let mut in_str = false;
        let mut esc = false;
        for &b in s.as_bytes() {
            if in_str {
                if esc {
                    esc = false;
                } else if b == b'\\' {
                    esc = true;
                } else if b == b'"' {
                    in_str = false;
                }
                continue;
            }
            match b {
                b'"' => in_str = true,
                b'{' | b'[' => {
                    if depth == stack.len() {
                        return Err("nested terlalu dalam");
                    }
                    stack[depth] = b;
                    depth += 1;
                }
                b'}' | b']' => {
                    if depth == 0 {
                        return Err("penutup tanpa pembuka");
                    }
                    depth -= 1;
                    let want = if b == b'}' { b'{' } else { b'[' };
                    if stack[depth] != want {
                        return Err("pasangan tidak cocok");
                    }
                }
                _ => {}
            }
        }
        if in_str || depth != 0 {
            return Err("terpotong");
        }
        Ok(())
    }

    static CONN1: [ConnSnapshot; 1] = [ConnSnapshot {
        protocol: "udp",
        src_ip: [192, 0, 2, 20],
        src_port: 34567,
        dst_ip: [198, 51, 100, 9],
        dst_port: 53,
        trans_port: 20480,
        fwd_packets: 4,
        rev_packets: 3,
    }];

    #[test]
    fn test_03e_every_get_body_is_parseable_json() {
        let mut dispatcher = admin_sess();
        let mut r = rt();
        r.lan_present = true;
        r.lan_ip = [198, 51, 100, 1];
        r.lan_mac = [1, 2, 3, 4, 5, 6];
        r.nat_sessions = &CONN1;
        r.nat_session_count = 1;
        r.last_write_ok = true;
        r.last_write_report = "wan0 applied";
        let reqs: [&'static [u8]; 12] = [
            b"GET /api/v1/system HTTP/1.1\r\n\r\n",
            b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n",
            b"GET /api/v1/firewall/rules HTTP/1.1\r\n\r\n",
            b"GET /api/v1/nat HTTP/1.1\r\n\r\n",
            b"GET /api/v1/nat/rules HTTP/1.1\r\n\r\n",
            b"GET /api/v1/nat/conntrack HTTP/1.1\r\n\r\n",
            b"GET /api/v1/wireless HTTP/1.1\r\n\r\n",
            b"GET /api/v1/wireless/devices HTTP/1.1\r\n\r\n",
            b"GET /api/v1/olt HTTP/1.1\r\n\r\n",
            b"GET /api/v1/olt/ons HTTP/1.1\r\n\r\n",
            b"GET /api/v1/services HTTP/1.1\r\n\r\n",
            b"GET /api/v1/writes/last HTTP/1.1\r\n\r\n",
        ];
        for (i, raw) in reqs.iter().enumerate() {
            let req = req_sess(raw);
            let mut buf = [0u8; 2400];
            let len = dispatcher
                .dispatch(&req, (i as u64) * 2, &mut buf, &r)
                .unwrap();
            let resp = core::str::from_utf8(&buf[..len]).unwrap();
            let body = resp.split_once("\r\n\r\n").map_or("", |x| x.1);
            assert!(
                body.starts_with("{\"ok\":true"),
                "{} tidak memulai sampul ok:true: {}",
                req.path,
                body
            );
            json_pairs_balanced(body).unwrap_or_else(|e| panic!("{}: {} — {}", req.path, e, body));
        }
    }

    #[test]
    fn test_03f_interfaces_rows_are_closed_objects() {
        let mut dispatcher = admin_sess();
        let mut r = rt();
        r.lan_present = true;
        let req = req_sess(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        // Setiap baris harus berakhir `true}` / `false}` sebelum pemisah atau
        // penutup array; ini persis bentuk yang dulu bocor satu kurung.
        assert!(resp.contains("\"editable\":true},"));
        assert!(resp.contains("\"editable\":false}]}"));
        assert_eq!(resp.matches("\"name\":").count(), 2);
    }

    #[test]
    fn test_04_api_firewall_rules_endpoint() {
        let mut dispatcher = admin_sess();
        let req = req_sess(b"GET /api/v1/firewall/rules HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"default_policy\":\"deny\""));
        assert!(resp.contains("\"tcam_rules\":16"));
        assert!(resp.contains("\"http_port\":80"));
    }

    #[test]
    fn test_04b_firewall_protected_requires_datapath() {
        // Jumlah aturan bukan bukti penegakan. Selama classifier tidak pernah
        // ditanya pada frame yang lewat, pelaporan "WAN terlindungi: YA" adalah
        // klaim keamanan yang salah.
        let mut dispatcher = admin_sess();
        let req = req_sess(b"GET /api/v1/firewall/rules HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];

        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"wan_protected\":false"));
        assert!(resp.contains("\"rules_enforced\":false"));
        assert!(resp.contains("\"datapath\":\"engine-only\""));

        let mut wired = rt();
        wired.firewall_wired = true;
        let len = dispatcher.dispatch(&req, 0, &mut buf, &wired).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"wan_protected\":true"));
        assert!(resp.contains("\"rules_enforced\":true"));
        assert!(resp.contains("\"datapath\":\"wired\""));
    }

    #[test]
    fn test_04c_lan_status_datang_dari_alamat_bukan_literal() {
        // Regresi §5: interface LAN tanpa alamat tidak boleh lagi dilaporkan
        // berjalan hanya karena barisnya tercetak.
        let mut dispatcher = admin_sess();
        let mut r = rt();
        r.lan_present = true;
        r.lan_ip = [0, 0, 0, 0];
        let req = req_sess(b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 2048];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("\"role\":\"lan\""));
        assert!(resp.contains("\"status\":\"down\""));
        assert!(resp.contains("\"running\":false"));

        r.lan_ip = [192, 168, 56, 100];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"status\":\"up\""));
        assert!(resp.contains("\"running\":true"));
    }

    #[test]
    fn test_05_api_services_reflects_actual_daemons() {
        let mut dispatcher = admin_sess();
        let req = req_sess(b"GET /api/v1/services HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"http_server\":true"));
        assert!(resp.contains("\"mdns_responder\":false"));
        assert!(resp.contains("\"dns_proxy\":false"));
        assert!(resp.contains("\"http_bind_ip\":\"198.51.100.77\""));
    }

    #[test]
    fn test_05b_api_services_reports_http_down_when_not_listening() {
        let mut dispatcher = admin_sess();
        let mut r = rt();
        r.http_listening = false;
        r.wan_up = false;
        r.wan_ip = [0, 0, 0, 0];
        let req = req_sess(b"GET /api/v1/services HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"http_server\":false"));
        assert!(resp.contains("\"http_bind_ip\":\"0.0.0.0\""));
    }

    #[test]
    fn test_06_api_not_found_endpoint() {
        let mut dispatcher = admin_sess();
        let req = req_sess(b"GET /api/v1/unknown HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(resp.contains("HTTP/1.1 404 Not Found"));
        assert!(resp.contains("\"error\":\"not_found\""));
    }

    /// Burst ember baca per klien. Dinyatakan di sini supaya tes memakai
    /// kuota secara eksplisit, bukan menebak-nebak.
    const READ_BURST: usize = 60;

    #[test]
    fn test_07_api_rate_limit_429_per_client() {
        let mut dispatcher = admin_sess();
        let req = req_sess(b"GET /api/v1/system HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let mut a = rt();
        a.peer_ip = [192, 0, 2, 10];
        let mut b = rt();
        b.peer_ip = [192, 0, 2, 11];

        // Habiskan burst baca klien A pada tick 0 (tidak ada pengisian ulang).
        for i in 0..READ_BURST {
            assert!(
                dispatcher.dispatch(&req, 0, &mut buf, &a).is_ok(),
                "permintaan ke-{i} dari A masih harus dilayani"
            );
        }
        let len = dispatcher.dispatch(&req, 0, &mut buf, &a).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 429 Too Many Requests"));
        assert!(resp.contains("\"error\":\"rate_limit_exceeded\""));

        // Klien B tidak ikut dihukum. Inilah akar 429 massal AUD-01: satu ember
        // global yang dihabiskan klien pertama.
        let len = dispatcher.dispatch(&req, 0, &mut buf, &b).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(
            resp.contains("HTTP/1.1 200 OK"),
            "klien lain wajib tetap terlayani"
        );

        // Jendela pulih: satu tick berikutnya menambah 20 token untuk A.
        let len = dispatcher.dispatch(&req, 1, &mut buf, &a).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(dispatcher.rate_limiter.denied >= 1);
    }

    #[test]
    fn test_07b_buckets_are_isolated_by_category() {
        let mut lim = ClientRateLimiter::new();
        let ip = [198, 51, 100, 5];
        for _ in 0..4 {
            assert!(lim.allow(&ip, RATE_AUTH, 0));
        }
        assert!(
            !lim.allow(&ip, RATE_AUTH, 0),
            "percobaan login ke-5 ditolak"
        );
        for _ in 0..READ_BURST {
            assert!(
                lim.allow(&ip, RATE_READ, 0),
                "ember autentikasi tidak boleh memakan kuota baca"
            );
        }
        for _ in 0..6 {
            assert!(lim.allow(&ip, RATE_WRITE, 0));
        }
        assert!(
            !lim.allow(&ip, RATE_WRITE, 0),
            "kuota tulis punya jendela sendiri dan tetap dibatasi"
        );
        assert!(
            lim.allow(&[198, 51, 100, 6], RATE_AUTH, 0),
            "login klien lain tidak ikut terkuras"
        );
    }

    #[test]
    fn test_07c_window_refill_uses_declared_period() {
        let mut lim = ClientRateLimiter::new();
        let ip = [203, 0, 113, 9];
        for _ in 0..4 {
            assert!(lim.allow(&ip, RATE_AUTH, 0));
        }
        assert!(
            !lim.allow(&ip, RATE_AUTH, 29),
            "periode auth 30 tick: token belum kembali"
        );
        assert!(
            lim.allow(&ip, RATE_AUTH, 30),
            "satu periode terlampaui -> satu token"
        );
        assert!(
            !lim.allow(&ip, RATE_AUTH, 30),
            "satu periode hanya memberi satu token"
        );

        for _ in 0..6 {
            assert!(lim.allow(&ip, RATE_WRITE, 30));
        }
        assert!(!lim.allow(&ip, RATE_WRITE, 30));
        assert!(
            !lim.allow(&ip, RATE_WRITE, 31),
            "periode tulis belum genap 2 tick"
        );
        assert!(lim.allow(&ip, RATE_WRITE, 32));
    }

    #[test]
    fn test_07d_idle_buckets_are_purged() {
        let mut lim = ClientRateLimiter::new();
        assert!(lim.allow(&[10, 0, 0, 1], RATE_READ, 0));
        assert!(lim.allow(&[10, 0, 0, 2], RATE_READ, 0));
        assert_eq!(lim.live_buckets(), 2);
        // Menganggur 101 tick (> RATE_IDLE_TICKS) -> ember klien lama dilepas.
        assert!(lim.allow(&[10, 0, 0, 3], RATE_READ, 101));
        assert_eq!(
            lim.live_buckets(),
            1,
            "ember klien yang sudah pergi harus dibersihkan"
        );
    }

    #[test]
    fn test_07e_full_table_evicts_instead_of_denying() {
        let mut lim = ClientRateLimiter::new();
        for i in 0..RATE_BUCKETS as u8 {
            assert!(lim.allow(&[192, 0, 2, i], RATE_READ, i as u64));
        }
        assert_eq!(lim.live_buckets(), RATE_BUCKETS as u32);
        let before = lim.evictions;
        assert!(
            lim.allow(&[192, 0, 2, 200], RATE_READ, 15),
            "klien setelah batas tabel tidak boleh ditolak hanya karena tabel penuh"
        );
        assert_eq!(lim.evictions, before + 1);
        assert_eq!(lim.live_buckets(), RATE_BUCKETS as u32);
        // Yang tergusur adalah ember paling lama menganggur; klien itu memulai
        // lagi dari burst penuh, bukan dari penolakan.
        assert!(lim.allow(&[192, 0, 2, 0], RATE_READ, 15));
    }

    #[test]
    fn test_07f_penalize_only_hits_offending_client() {
        let mut lim = ClientRateLimiter::new();
        let bad = [198, 51, 100, 20];
        let good = [198, 51, 100, 21];
        assert!(lim.allow(&bad, RATE_AUTH, 0));
        lim.penalize(&bad, RATE_AUTH, 0);
        lim.penalize(&bad, RATE_AUTH, 0);
        assert!(lim.allow(&bad, RATE_AUTH, 0));
        assert!(
            !lim.allow(&bad, RATE_AUTH, 0),
            "hukuman memperkecil ruang percobaan klien itu"
        );
        assert!(
            lim.allow(&good, RATE_AUTH, 0),
            "hukuman tidak merembet ke klien lain"
        );
        // Klien tanpa ember: tidak apa-apa, bukan panic.
        lim.penalize(&[240, 0, 0, 1], RATE_AUTH, 0);
    }

    #[test]
    fn test_07g_interleaved_clients_keep_independent_state() {
        let mut lim = ClientRateLimiter::new();
        let a = [192, 0, 2, 1];
        let b = [192, 0, 2, 2];
        for _ in 0..4 {
            assert!(lim.allow(&a, RATE_AUTH, 0));
            assert!(lim.allow(&b, RATE_AUTH, 0));
        }
        assert!(!lim.allow(&a, RATE_AUTH, 0));
        assert!(!lim.allow(&b, RATE_AUTH, 0));
        assert!(lim.allow(&a, RATE_READ, 0));
        assert!(lim.allow(&b, RATE_READ, 0));
        assert_eq!(lim.live_buckets(), 4, "dua ember auth + dua ember baca");
    }

    #[test]
    fn test_07h_operator_refresh_pattern_is_never_limited() {
        let mut dispatcher = admin_sess();
        let req = req_sess(b"GET /api/v1/system HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let mut r = rt();
        r.peer_ip = [192, 0, 2, 30];
        // Pola baca terukur AUD-01: 4 permintaan tiap 5 s (50 tick), selama 10 menit.
        let mut denied = 0u32;
        for group in 0..120u64 {
            let tick = group * 50;
            for _ in 0..4 {
                let len = dispatcher.dispatch(&req, tick, &mut buf, &r).unwrap();
                let resp = core::str::from_utf8(&buf[..len]).unwrap();
                if resp.contains("429") {
                    denied += 1;
                }
            }
        }
        assert_eq!(
            denied, 0,
            "pola pembacaan operator sah tidak boleh kena 429"
        );
        assert_eq!(dispatcher.rate_limiter.denied, 0);
    }

    #[test]
    fn test_08_api_envelope_format() {
        let mut dispatcher = admin_sess();
        let req = req_sess(b"GET /api/v1/system HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 1, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("{\"ok\":true,\"data\":"));
    }

    #[test]
    fn test_09_json_writer_ip_mac_and_overflow_guard() {
        let mut body = [0u8; 64];
        let n = {
            let mut j = JsonBuf::new(&mut body);
            j.raw(b"{\"ip\":").quoted_ip([10, 0, 0, 1]).raw(b",\"m\":");
            j.quoted_mac([0xab, 0xcd, 0xef, 0x00, 0x11, 0x22]);
            j.raw(b",\"u\":").u32(0).raw(b",\"b\":").bool(false);
            j.raw(b"}");
            j.finish().unwrap()
        };
        assert_eq!(
            core::str::from_utf8(&body[..n]).unwrap(),
            "{\"ip\":\"10.0.0.1\",\"m\":\"ab:cd:ef:00:11:22\",\"u\":0,\"b\":false}"
        );

        let mut tiny = [0u8; 8];
        let res = {
            let mut j = JsonBuf::new(&mut tiny);
            j.raw(b"0123456789abcdef");
            j.finish()
        };
        assert!(res.is_none(), "overflow wajib terdeteksi");
    }

    #[test]
    fn test_10_nat_summary_admits_engine_only_forwarding() {
        let mut dispatcher = admin_sess();
        let req = req_sess(b"GET /api/v1/nat HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        // rt() memakai alamat test-net, bukan IP produksi.
        assert!(resp.contains("\"masquerade_ip\":\"198.51.100.77\""));
        assert!(resp.contains("\"forwarding\":\"engine-only\""));
        assert!(resp.contains("\"forwarding_wired\":false"));
        assert!(resp.contains("\"capacity\":128"));
        assert!(resp.contains("\"active_sessions\":0"));
    }

    #[test]
    fn test_11_nat_rules_come_from_snapshot() {
        let mut dispatcher = admin_sess();
        let req = req_sess(b"GET /api/v1/nat/rules HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"wan_port\":7777"));
        assert!(resp.contains("\"target_ip\":\"192.0.2.11\""));
        assert!(resp.contains("\"protocol\":\"udp\""));
    }

    #[test]
    fn test_12_wireless_and_olt_report_zero_rows() {
        let mut dispatcher = admin_sess();
        let mut buf = [0u8; 512];
        let req = req_sess(b"GET /api/v1/wireless HTTP/1.1\r\n\r\n");
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"radios\":0"));
        assert!(resp.contains("\"operational\":false"));

        let req = req_sess(b"GET /api/v1/olt/ons HTTP/1.1\r\n\r\n");
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("{\"ok\":true,\"data\":[]}"));
    }

    extern crate alloc;
    use alloc::boxed::Box;
    use alloc::format;

    /// Susun permintaan POST utuh dengan Content-Length yang benar. Bocor
    /// (leak) disengaja: hanya terjadi di test host,umur data dibuat 'static.
    fn post(body: &str) -> HttpRequest<'static> {
        let head = format!(
            "POST /api/v1/interfaces/eth0 HTTP/1.1\r\nHost: 198.51.100.77\r\nAuthorization: {}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n",
            FIX_SESSION,
            body.len()
        );
        let raw: &'static mut str = Box::leak((head + body).into_boxed_str());
        HttpRequest::parse(raw.as_bytes()).unwrap()
    }

    #[test]
    fn test_13_post_static_queues_command() {
        let mut dispatcher = admin_sess();
        let req = post("mode=static&ip=198.51.100.30&netmask=255.255.255.0&gateway=198.51.100.1&name=eth0&persist=1");
        let mut buf = [0u8; 512];
        let len = dispatcher
            .dispatch_post(&req, &mut buf, &rt())
            .expect("POST harus terlayani");
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"status\":\"queued\""));
        assert!(resp.contains("\"persist\":true"));

        let cmd = dispatcher.take_write().expect("perintah masuk antrean");
        assert_eq!(cmd.target_str(), "eth0");
        assert_eq!(cmd.mode, WAN_MODE_STATIC);
        assert_eq!(cmd.ip, [198, 51, 100, 30]);
        assert_eq!(cmd.gw, [198, 51, 100, 1]);
        assert!(cmd.persist);
        assert!(dispatcher.take_write().is_none());
    }

    #[test]
    fn test_14_post_dhcp_mode_is_accepted_and_clears() {
        let mut dispatcher = admin_sess();
        let req = post("mode=dhcp");
        let mut buf = [0u8; 512];
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        assert!(core::str::from_utf8(&buf[..len])
            .unwrap()
            .contains("\"mode\":\"dhcp\""));
        let cmd = dispatcher.take_write().unwrap();
        assert_eq!(cmd.mode, WAN_MODE_DHCP);
        assert_eq!(cmd.ip, [0, 0, 0, 0]);
        // Nama tidak dikirim => tetap nama berjalan.
        assert_eq!(cmd.new_name_str(), "eth0");
    }

    #[test]
    fn test_15_post_rejects_dhcp_with_manual_address() {
        let mut dispatcher = admin_sess();
        let req = post("mode=dhcp&ip=198.51.100.30");
        let mut buf = [0u8; 512];
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 400 Bad Request"));
        assert!(resp.contains("\"error\":\"dhcp_rejects_manual_address\""));
        assert!(dispatcher.take_write().is_none());
        assert_eq!(dispatcher.writes_rejected, 1);
    }

    #[test]
    fn test_16_post_address_validation_matrix() {
        let cases: [(&str, &str); 8] = [
            ("mode=static", "static_requires_ip"),
            ("mode=static&ip=198.51.100.9", "static_requires_netmask"),
            (
                "mode=static&ip=198.51.100.9&netmask=255.0.255.0",
                "netmask_not_contiguous",
            ),
            (
                "mode=static&ip=198.51.100.9&netmask=255.255.255.0&gateway=10.9.8.7",
                "gateway_outside_subnet",
            ),
            (
                "mode=static&ip=198.51.100.0&netmask=255.255.255.0",
                "ip_is_network_address",
            ),
            (
                "mode=static&ip=198.51.100.255&netmask=255.255.255.0",
                "ip_is_broadcast_address",
            ),
            (
                "mode=static&ip=127.0.0.1&netmask=255.0.0.0",
                "ip_reserved_or_loopback",
            ),
            (
                "mode=static&ip=198.51.100.9&netmask=255.255.255.0&name=BAD%20NAME",
                "name_invalid",
            ),
        ];
        for (body, expected) in cases {
            let mut dispatcher = admin_sess();
            let req = post(body);
            let mut buf = [0u8; 512];
            let len = dispatcher
                .dispatch_post(&req, &mut buf, &rt())
                .expect("respons harus tersusun");
            let resp = core::str::from_utf8(&buf[..len]).unwrap();
            assert!(
                resp.contains(expected),
                "body `{}` harus ditolak `{}`, dapat: {}",
                body,
                expected,
                resp
            );
            assert!(dispatcher.take_write().is_none());
        }
    }

    #[test]
    fn test_17_post_requires_matching_host_and_content_type() {
        // Host beda asal => 403 (proteksi CSRF), tanpa menyentuh antrean.
        let mut dispatcher = admin_sess();
        let raw = b"POST /api/v1/interfaces/eth0 HTTP/1.1\r\nHost: 203.0.113.9\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 9\r\n\r\nmode=dhcp";
        let req = req_sess(raw);
        let mut buf = [0u8; 512];
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 403 Forbidden"));
        assert!(resp.contains("\"error\":\"host_mismatch\""));

        // Content-type salah => 400.
        let raw = b"POST /api/v1/interfaces/eth0 HTTP/1.1\r\nHost: 198.51.100.77\r\nContent-Type: application/json\r\nContent-Length: 9\r\n\r\nmode=dhcp";
        let req = req_sess(raw);
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"error\":\"unsupported_content_type\""));
    }

    #[test]
    fn test_18_write_queue_capacity_is_honest() {
        let mut dispatcher = admin_sess();
        let mut buf = [0u8; 512];
        for _ in 0..WEB_WRITE_QUEUE {
            let req = post("mode=dhcp");
            let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
            assert!(core::str::from_utf8(&buf[..len])
                .unwrap()
                .contains("HTTP/1.1 200 OK"));
        }
        let req = post("mode=dhcp");
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 500 Internal Server Error"));
        assert!(resp.contains("\"error\":\"write_queue_full\""));
    }

    #[test]
    fn test_19_pure_validators() {
        assert_eq!(parse_dotted("198.51.100.77"), Some([198, 51, 100, 77]));
        assert_eq!(parse_dotted("198.51.100.256"), None);
        assert_eq!(parse_dotted("198.51.100"), None);
        assert_eq!(parse_dotted("198.51.100.77.5"), None);
        assert_eq!(parse_dotted("1.2.3.4 "), None);
        assert_eq!(mask_prefix([255, 255, 254, 0]), Some(23));
        assert_eq!(mask_prefix([255, 0, 255, 0]), None);
        assert_eq!(mask_prefix([0, 0, 0, 0]), None);
        assert_eq!(split_host_port("10.0.0.1:8080"), ("10.0.0.1", Some(8080)));
        assert_eq!(split_host_port("rinjani"), ("rinjani", None));
        assert!(starts_with_ci(
            "Application/X-WWW-Form-Urlencoded; charset=utf-8",
            "application/x-www-form-urlencoded"
        ));
    }

    #[test]
    fn test_20_write_status_reports_kernel_verdict() {
        let mut dispatcher = admin_sess();
        let mut buf = [0u8; 512];
        let req = req_sess(b"GET /api/v1/writes/last HTTP/1.1\r\n\r\n");

        // Belum ada perintah yang pernah diterapkan kernel: tidak ada laporan,
        // jadi UI tidak boleh menampilkan pesan keberhasilan.
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"applied\":false"));
        assert!(resp.contains("\"report\":\"\""));

        // Setelah poll loop menerapkan perintah, verdict kernel terbaca apa
        // adanya termasuk alasan penolakannya.
        let mut r = rt();
        r.last_write_ok = true;
        r.last_write_report = "statik diterapkan, probe ARP DAD TX";
        let len = dispatcher.dispatch(&req, 1, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"applied\":true"));
        assert!(resp.contains("probe ARP DAD TX"));

        let mut r = rt();
        r.last_write_ok = false;
        r.last_write_report = "target 'eth9' bukan antarmuka WAN terdeteksi";
        let len = dispatcher.dispatch(&req, 2, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"ok\":false"));
        assert!(resp.contains("bukan antarmuka WAN"));
    }

    /// POST `/api/v1/power` dengan Content-Length yang benar.
    fn post_power(body: &str) -> HttpRequest<'static> {
        let head = format!(
            "POST /api/v1/power HTTP/1.1\r\nHost: 198.51.100.77\r\nAuthorization: {}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n",
            FIX_SESSION,
            body.len()
        );
        let raw: &'static mut str = Box::leak((head + body).into_boxed_str());
        HttpRequest::parse(raw.as_bytes()).unwrap()
    }

    fn power_post(dispatcher: &mut ApiDispatcher, body: &str, tick: u64) -> alloc::string::String {
        let req = post_power(body);
        let mut r = rt();
        r.tick_100ms = tick;
        let mut buf = [0u8; 512];
        let len = dispatcher
            .dispatch_post(&req, &mut buf, &r)
            .expect("POST /api/v1/power harus terlayani");
        alloc::string::String::from(core::str::from_utf8(&buf[..len]).unwrap())
    }

    #[test]
    fn test_21_power_requires_arm_token_before_action() {
        let mut dispatcher = admin_sess();

        // Aksi tanpa slot ter-arm => 403 dan tidak ada perintah tertunda.
        let resp = power_post(&mut dispatcher, "action=restart", 0);
        assert!(resp.contains("HTTP/1.1 403 Forbidden"));
        assert!(resp.contains("\"error\":\"power_not_armed"));
        assert!(dispatcher.take_power().is_none());
        assert!(!dispatcher.power_armed(0));

        // Arm menyalakan slot dan ACK menyebut masa berlakunya.
        let resp = power_post(&mut dispatcher, "action=arm", 10);
        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"status\":\"armed\""));
        assert!(resp.contains("\"valid_for_s\":20"));
        assert!(dispatcher.power_armed(10));
        assert!(dispatcher.power_armed(209));
        assert_eq!(dispatcher.power_arm_left_s(200), 1);

        // Satu aksi memakai slot sampai habis; aksi kedua harus ditolak.
        let resp = power_post(&mut dispatcher, "action=shutdown", 20);
        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"status\":\"scheduled\""));
        assert!(resp.contains("\"action\":\"shutdown\""));
        assert!(resp.contains("\"exec_in_ms\":1500"));
        assert_eq!(dispatcher.take_power(), Some(POWER_SHUTDOWN));
        assert!(!dispatcher.power_armed(21));
        assert!(dispatcher.take_power().is_none());

        let resp = power_post(&mut dispatcher, "action=restart", 22);
        assert!(resp.contains("\"error\":\"power_not_armed\""));
        assert!(dispatcher.take_power().is_none());
    }

    #[test]
    fn test_22_power_arm_expires_after_window() {
        let mut dispatcher = admin_sess();
        power_post(&mut dispatcher, "action=arm", 100);
        assert!(dispatcher.power_armed(299));
        assert!(!dispatcher.power_armed(100 + POWER_ARM_TICKS));

        // Kedaluwarsa bukan hanya penolakan: slot dibersihkan, jadi permintaan
        // susulan pada tick yang sama tidak bisa 'menumpang' arm lama.
        let resp = power_post(&mut dispatcher, "action=restart", 100 + POWER_ARM_TICKS);
        assert!(resp.contains("HTTP/1.1 403 Forbidden"));
        assert!(dispatcher.take_power().is_none());
        assert!(!dispatcher.power_armed(100 + POWER_ARM_TICKS + 1));
    }

    #[test]
    fn test_23_power_rejects_bad_input_without_touching_slot() {
        let mut dispatcher = admin_sess();
        power_post(&mut dispatcher, "action=arm", 0);

        let resp = power_post(&mut dispatcher, "action=hibernate", 1);
        assert!(resp.contains("HTTP/1.1 400"));
        assert!(resp.contains("\"error\":\"bad_power_action\""));

        let resp = power_post(&mut dispatcher, "mode=dhcp", 2);
        assert!(resp.contains("\"error\":\"missing_action\""));

        // Aksi liar tidak boleh menghabiskan hak arm yang sedang berlaku.
        assert!(dispatcher.power_armed(3));
        let resp = power_post(&mut dispatcher, "action=restart", 4);
        assert!(resp.contains("\"status\":\"scheduled\""));
        assert_eq!(dispatcher.take_power(), Some(POWER_RESTART));
    }

    #[test]
    fn test_24_power_post_content_type_and_host_are_enforced() {
        // Content-type salah => ditolak sebelum slot daya disentuh.
        let mut dispatcher = admin_sess();
        let raw = b"POST /api/v1/power HTTP/1.1\r\nHost: 198.51.100.77\r\nContent-Type: application/json\r\nContent-Length: 10\r\n\r\naction=arm";
        let req = req_sess(raw);
        let mut buf = [0u8; 512];
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"error\":\"unsupported_content_type\""));
        assert!(!dispatcher.power_armed(0));

        // Host asing => 403 host_mismatch; jalur daya tidak ditembus.
        let raw = b"POST /api/v1/power HTTP/1.1\r\nHost: 203.0.113.9\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 10\r\n\r\naction=arm";
        let req = req_sess(raw);
        let len = dispatcher.dispatch_post(&req, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 403 Forbidden"));
        assert!(resp.contains("\"error\":\"host_mismatch\""));
        assert!(!dispatcher.power_armed(0));
    }

    #[test]
    fn test_25_services_advertises_power_slot_state() {
        let mut dispatcher = admin_sess();
        let req = req_sess(b"GET /api/v1/services HTTP/1.1\r\n\r\n");
        let mut buf = [0u8; 1024];
        let len = dispatcher.dispatch(&req, 0, &mut buf, &rt()).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"power_armed\":false"));
        assert!(resp.contains("\"power_arm_left_s\":0"));
        assert!(resp.contains("\"power_pending\":false"));

        power_post(&mut dispatcher, "action=arm", 50);
        let mut r = rt();
        r.tick_100ms = 50;
        let len = dispatcher.dispatch(&req, 1, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"power_armed\":true"));
        assert!(resp.contains("\"power_arm_left_s\":20"));
    }

    // -- P0.1: gerbang autentikasi/otorisasi di lapisan server ----------------

    /// `WebRuntime` pada tick dan identitas peer tertentu. Identitas selalu
    /// berasal dari sesi TCP, tidak pernah dari header.
    fn rt_peer(tick: u64, ip: [u8; 4]) -> WebRuntime<'static> {
        let mut r = rt();
        r.tick_100ms = tick;
        r.peer_ip = ip;
        r
    }

    /// POST login anonim dengan body form-urlencoded.
    fn login_post(user: &str, pw: &str) -> HttpRequest<'static> {
        let body = alloc::format!("user={}&password={}", user, pw);
        let head = format!(
            "POST /api/v1/auth/login HTTP/1.1\r\nHost: 198.51.100.77\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        mk(head + &body)
    }

    /// POST logout dengan token bearer given.
    fn logout_post(bearer: &str) -> HttpRequest<'static> {
        mk(format!(
            "POST /api/v1/auth/logout HTTP/1.1\r\nHost: 198.51.100.77\r\nAuthorization: Bearer {}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 0\r\n\r\n",
            bearer
        ))
    }

    /// Ambil nilai `"token":"<64 hex>"` dari sampul JSON.
    fn token_of(resp: &str) -> alloc::string::String {
        const MARK: &str = "\"token\":\"";
        let i = resp.find(MARK).expect("respons memuat token");
        alloc::string::String::from(&resp[i + MARK.len()..][..64])
    }

    const READ_PATHS: [&'static [u8]; 17] = [
        b"GET /api/v1/system HTTP/1.1\r\n\r\n",
        b"GET /api/v1/interfaces HTTP/1.1\r\n\r\n",
        b"GET /api/v1/firewall/rules HTTP/1.1\r\n\r\n",
        b"GET /api/v1/nat HTTP/1.1\r\n\r\n",
        b"GET /api/v1/nat/rules HTTP/1.1\r\n\r\n",
        b"GET /api/v1/nat/conntrack HTTP/1.1\r\n\r\n",
        b"GET /api/v1/arp HTTP/1.1\r\n\r\n",
        b"GET /api/v1/arp/stats HTTP/1.1\r\n\r\n",
        b"GET /api/v1/routes HTTP/1.1\r\n\r\n",
        b"GET /api/v1/routes/stats HTTP/1.1\r\n\r\n",
        b"GET /api/v1/dns HTTP/1.1\r\n\r\n",
        b"GET /api/v1/wireless HTTP/1.1\r\n\r\n",
        b"GET /api/v1/wireless/devices HTTP/1.1\r\n\r\n",
        b"GET /api/v1/olt HTTP/1.1\r\n\r\n",
        b"GET /api/v1/olt/ons HTTP/1.1\r\n\r\n",
        b"GET /api/v1/services HTTP/1.1\r\n\r\n",
        b"GET /api/v1/writes/last HTTP/1.1\r\n\r\n",
    ];

    /// Klien anonim ditolak di SEMUA jalur baca, bukan hanya disembunyikan di UI.
    #[test]
    fn test_26_anonim_ditolak_di_semua_jalur_baca() {
        let mut d = admin_sess();
        let mut buf = [0u8; 512];
        for (i, raw) in READ_PATHS.iter().enumerate() {
            let req = req_anon(raw);
            let r = rt_peer(i as u64, [198, 51, 100, 9]);
            let len = d.dispatch(&req, i as u64, &mut buf, &r).unwrap();
            let resp = core::str::from_utf8(&buf[..len]).unwrap();
            assert!(
                resp.contains("HTTP/1.1 401 Unauthorized"),
                "{} => {}",
                req.path,
                resp
            );
            assert!(
                resp.contains("\"error\":\"unauthenticated\""),
                "{} => {}",
                req.path,
                resp
            );
            assert!(
                resp.contains("WWW-Authenticate: Bearer"),
                "{} => {}",
                req.path,
                resp
            );
        }
        assert_eq!(d.auth.rejects_anon, READ_PATHS.len() as u32);

        // Token berbetuk sah tetapi bukan sesi aktif: tetap 401, bukan data.
        let asing = "0000000000000000000000000000000000000000000000000000000000000000";
        let req = req_bearer(b"GET /api/v1/system HTTP/1.1\r\n\r\n", asing);
        let len = d
            .dispatch(&req, 20, &mut buf, &rt_peer(20, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 401 Unauthorized"));
        assert!(resp.contains("\"error\":\"session_expired\""));
        assert!(
            !resp.contains("\"distro\":\"Rinjani 1.0.0\""),
            "respons ditolak tidak boleh memuat data: {}",
            resp
        );

        // Header cacat: invalid_token.
        let req = req_bearer(b"GET /api/v1/system HTTP/1.1\r\n\r\n", "zz");
        let len = d
            .dispatch(&req, 40, &mut buf, &rt_peer(40, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"error\":\"invalid_token\""), "{}", resp);
    }

    /// Jalur tulis dan aksi daya tidak bisa disentuh tanpa sesi, dan penolakan
    /// tidak boleh mengubah state router.
    #[test]
    fn test_27_anonim_tidak_menembus_jalur_tulis_dan_daya() {
        let mut d = admin_sess();
        let mut buf = [0u8; 512];
        let cases: [&'static [u8]; 3] = [
            b"POST /api/v1/interfaces/eth0 HTTP/1.1\r\nHost: 198.51.100.77\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 9\r\n\r\nmode=dhcp",
            b"POST /api/v1/power HTTP/1.1\r\nHost: 198.51.100.77\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 10\r\n\r\naction=arm",
            b"POST /api/v1/auth/logout HTTP/1.1\r\nHost: 198.51.100.77\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 0\r\n\r\n",
        ];
        for (i, raw) in cases.iter().enumerate() {
            let req = req_anon(raw);
            let tick = (i as u64) * 30;
            let len = d
                .dispatch_post(&req, &mut buf, &rt_peer(tick, [198, 51, 100, 9]))
                .unwrap();
            let resp = core::str::from_utf8(&buf[..len]).unwrap();
            assert!(
                resp.contains("HTTP/1.1 401 Unauthorized"),
                "{} => {}",
                req.path,
                resp
            );
            assert!(resp.contains("\"error\":\"unauthenticated\""));
        }
        assert!(
            d.take_write().is_none(),
            "permintaan ditolak tidak mengantre"
        );
        assert!(
            d.take_power().is_none(),
            "permintaan ditolak tidak menjadwal daya"
        );
        assert!(
            !d.power_armed(0),
            "slot daya tidak boleh ter-arm oleh anonim"
        );
        assert_eq!(d.auth.sessions_active(), 1, "sesi fixture tetap hidup");
    }

    /// Tanpa kredensial, seluruh API terkunci dan tidak ada jalur jaring yang
    /// bisa membuat akun pertama.
    #[test]
    fn test_28_tanpa_kredensial_semua_jalur_terkunci() {
        let mut d = ApiDispatcher::new();
        let mut buf = [0u8; 512];
        assert!(!d.auth.has_users(), "tidak boleh ada akun bawaan");

        let req = req_sess(b"GET /api/v1/system HTTP/1.1\r\n\r\n");
        let len = d
            .dispatch(&req, 0, &mut buf, &rt_peer(0, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 401 Unauthorized"), "{}", resp);
        assert!(resp.contains("\"error\":\"no_credentials\""), "{}", resp);

        let req = post("mode=dhcp");
        let len = d
            .dispatch_post(&req, &mut buf, &rt_peer(30, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"error\":\"no_credentials\""), "{}", resp);
        assert!(d.take_write().is_none());

        // Login pun tidak menerima tebak-tebakan sebelum ada akun.
        let req = login_post("siapa-pun", "kata-sandi-sah-12345");
        let len = d
            .dispatch_post(&req, &mut buf, &rt_peer(60, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"error\":\"no_credentials\""), "{}", resp);
        assert_eq!(d.auth.sessions_active(), 0);

        // Probe status tetap menjawab dan menandai butuh bootstrap.
        let req = req_anon(b"GET /api/v1/auth/status HTTP/1.1\r\n\r\n");
        let len = d
            .dispatch(&req, 90, &mut buf, &rt_peer(90, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{}", resp);
        assert!(resp.contains("\"bootstrap_required\":true"), "{}", resp);
    }

    /// Kredensial salah selalu menjawab sama, tanpa membuka sesi.
    #[test]
    fn test_29_kredensial_salah_tidak_membuka_sesi() {
        let mut d = admin_sess();
        let mut buf = [0u8; 512];
        let pw = core::str::from_utf8(FIX_PW).unwrap();
        let salah: [(&str, &str); 3] = [
            ("orang-lain", pw),
            ("auditor", "kata-sandi-palsu-12345"),
            ("auditor", "pendek"),
        ];
        for (i, (u, p)) in salah.iter().enumerate() {
            let tick = (i as u64) * 30;
            let req = login_post(u, p);
            let len = d
                .dispatch_post(&req, &mut buf, &rt_peer(tick, [198, 51, 100, 9]))
                .unwrap();
            let resp = core::str::from_utf8(&buf[..len]).unwrap();
            assert!(
                resp.contains("HTTP/1.1 401 Unauthorized"),
                "{} => {}",
                u,
                resp
            );
            assert!(
                resp.contains("\"error\":\"invalid_credentials\""),
                "{} => {}",
                u,
                resp
            );
            assert!(!resp.contains(p), "kata sandi kembali di respons: {}", resp);
            assert_eq!(d.auth.sessions_active(), 1, "hanya sesi fixture");
        }

        // Kredensial benar pada dispatcher yang sama => token yang benar-benar
        // bisa dipakai membaca.
        let req = login_post("auditor", pw);
        let len = d
            .dispatch_post(&req, &mut buf, &rt_peer(120, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{}", resp);
        assert!(resp.contains("\"role\":\"admin\""), "{}", resp);
        assert!(!resp.contains(pw), "sandi tidak boleh ikut kembali");
        let tk = token_of(resp);
        assert_eq!(d.auth.sessions_active(), 2);

        let req = req_bearer(b"GET /api/v1/system HTTP/1.1\r\n\r\n", &tk);
        let len = d
            .dispatch(&req, 121, &mut buf, &rt_peer(121, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{}", resp);
        assert!(resp.contains("\"distro\":\"Rinjani 1.0.0\""));
    }

    /// Role viewer boleh membaca, tidak boleh menulis konfigurasi atau daya.
    #[test]
    fn test_30_viewer_ditolak_untuk_tulis_dan_daya() {
        let mut d = viewer_sess();
        let mut buf = [0u8; 512];
        let req = req_sess(b"GET /api/v1/system HTTP/1.1\r\n\r\n");
        let len = d
            .dispatch(&req, 0, &mut buf, &rt_peer(0, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{}", resp);

        for (i, raw) in [
            post("mode=dhcp"),
            post_power("action=arm"),
            post_power("action=restart"),
        ]
        .iter()
        .enumerate()
        {
            let tick = (i as u64) * 30 + 1;
            let len = d
                .dispatch_post(raw, &mut buf, &rt_peer(tick, [198, 51, 100, 9]))
                .unwrap();
            let resp = core::str::from_utf8(&buf[..len]).unwrap();
            assert!(
                resp.contains("HTTP/1.1 403 Forbidden"),
                "{} => {}",
                raw.path,
                resp
            );
            assert!(
                resp.contains("\"error\":\"insufficient_role\""),
                "{} => {}",
                raw.path,
                resp
            );
        }
        assert!(d.take_write().is_none());
        assert!(d.take_power().is_none());
        assert!(!d.power_armed(200), "viewer tidak boleh men-arm aksi daya");

        // Logout adalah hak pemilik sesi, bukan aksi admin.
        let req = logout_post(FIX_TOKEN_HEX);
        let len = d
            .dispatch_post(&req, &mut buf, &rt_peer(300, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{}", resp);
        assert_eq!(d.auth.sessions_active(), 0);
    }

    /// Sesi kedaluwarsa karena idle, dan token mati setelah logout.
    #[test]
    fn test_31_sesi_kedaluwarsa_dan_dicabut() {
        let mut d = admin_sess();
        let mut buf = [0u8; 512];
        let req = req_sess(b"GET /api/v1/system HTTP/1.1\r\n\r\n");
        let len = d
            .dispatch(&req, 5, &mut buf, &rt_peer(5, [198, 51, 100, 9]))
            .unwrap();
        assert!(core::str::from_utf8(&buf[..len])
            .unwrap()
            .contains("HTTP/1.1 200 OK"));

        let lewat = 5 + AUTH_IDLE_TICKS + 1;
        let len = d
            .dispatch(&req, lewat, &mut buf, &rt_peer(lewat, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 401 Unauthorized"), "{}", resp);
        assert!(resp.contains("\"error\":\"session_expired\""), "{}", resp);
        assert_eq!(d.auth.sessions_active(), 0, "sesi mati dibersihkan");

        // Login ulang lalu logout: token yang sama tidak hidup lagi.
        let pw = core::str::from_utf8(FIX_PW).unwrap();
        let req = login_post("auditor", pw);
        let len = d
            .dispatch_post(&req, &mut buf, &rt_peer(lewat, [198, 51, 100, 9]))
            .unwrap();
        let tk = token_of(core::str::from_utf8(&buf[..len]).unwrap());
        let req = req_bearer(b"GET /api/v1/system HTTP/1.1\r\n\r\n", &tk);
        let len = d
            .dispatch(
                &req,
                lewat + 1,
                &mut buf,
                &rt_peer(lewat + 1, [198, 51, 100, 9]),
            )
            .unwrap();
        assert!(core::str::from_utf8(&buf[..len])
            .unwrap()
            .contains("HTTP/1.1 200 OK"));

        let req = logout_post(&tk);
        let len = d
            .dispatch_post(&req, &mut buf, &rt_peer(lewat + 2, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"status\":\"logged_out\""), "{}", resp);

        let req = req_bearer(b"GET /api/v1/system HTTP/1.1\r\n\r\n", &tk);
        let len = d
            .dispatch(
                &req,
                lewat + 3,
                &mut buf,
                &rt_peer(lewat + 3, [198, 51, 100, 9]),
            )
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 401 Unauthorized"), "{}", resp);
        assert!(resp.contains("\"error\":\"session_expired\""), "{}", resp);

        // Logout juga habiskan jendela arming daya.
        let req = login_post("auditor", pw);
        let len = d
            .dispatch_post(&req, &mut buf, &rt_peer(lewat + 30, [198, 51, 100, 9]))
            .unwrap();
        let tk2 = token_of(core::str::from_utf8(&buf[..len]).unwrap());
        // POST lengkap dengan token baru.
        let armed = mk(format!(
            "POST /api/v1/power HTTP/1.1\r\nHost: 198.51.100.77\r\nAuthorization: Bearer {}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 10\r\n\r\naction=arm",
            tk2
        ));
        let len = d
            .dispatch_post(&armed, &mut buf, &rt_peer(lewat + 31, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"status\":\"armed\""), "{}", resp);
        assert!(d.power_armed(lewat + 31));
        let req = logout_post(&tk2);
        let _ = d
            .dispatch_post(&req, &mut buf, &rt_peer(lewat + 32, [198, 51, 100, 9]))
            .unwrap();
        assert!(!d.power_armed(lewat + 32), "logout harus memutuskan arming");
    }

    /// Probe status anonim: cukup untuk membuka kartu masuk, tanpa membocorkan
    /// nama akun, kata sandi, maupun isi tabel kredensial.
    #[test]
    fn test_32_probe_status_anonim_tidak_membocorkan() {
        let mut d = admin_sess();
        let mut buf = [0u8; 512];
        let req = req_anon(b"GET /api/v1/auth/status HTTP/1.1\r\n\r\n");
        let len = d
            .dispatch(&req, 0, &mut buf, &rt_peer(0, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{}", resp);
        assert!(resp.contains("\"authenticated\":false"), "{}", resp);
        assert!(resp.contains("\"bootstrap_required\":false"), "{}", resp);
        assert!(!resp.contains("auditor"), "nama akun bocor: {}", resp);
        assert!(!resp.contains(FIX_TOKEN_HEX), "token bocor: {}", resp);
        // Probe tidak mengubah pencacah penolakan keamanan.
        assert_eq!(d.auth.rejects_anon, 0);
        assert_eq!(d.auth.sessions_active(), 1);

        // Dengan header milik sesi, probe melaporkan sesi itu sendiri.
        let req = req_sess(b"GET /api/v1/auth/status HTTP/1.1\r\n\r\n");
        let len = d
            .dispatch(&req, 1, &mut buf, &rt_peer(1, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"authenticated\":true"), "{}", resp);
        assert!(resp.contains("\"role\":\"admin\""), "{}", resp);
    }

    /// Kuota login habis hanya untuk klien yang meledak-ledak; klien lain tetap
    /// bisa masuk. Tidak ada blokir global.
    #[test]
    fn test_33_tebak_sandi_hanya_menguras_kuota_penyerang() {
        let mut d = admin_sess();
        let mut buf = [0u8; 512];
        let penyerang = [203, 0, 113, 66];
        let operator = [198, 51, 100, 99];
        let denied_anon = d.rate_limiter.denied;
        let mut kod = 0u32;
        for i in 0..5u64 {
            let req = login_post("auditor", "kata-sandi-palsu-12345");
            let len = d
                .dispatch_post(&req, &mut buf, &rt_peer(100 + i, penyerang))
                .unwrap();
            let resp = core::str::from_utf8(&buf[..len]).unwrap();
            if resp.contains("HTTP/1.1 429") {
                kod += 1;
            } else {
                assert!(
                    resp.contains("HTTP/1.1 401 Unauthorized"),
                    "percobaan {} => {}",
                    i,
                    resp
                );
            }
        }
        assert!(kod >= 1, "penyerang harus mulai kehabisan kuota");
        assert_eq!(d.rate_limiter.denied, denied_anon + kod);
        assert_eq!(
            d.auth.logins_bad, 2,
            "hanya usaha yang dilayani yang dicatat"
        );

        // Operator pada alamat lain tetap bisa masuk penuh.
        let req = login_post("auditor", core::str::from_utf8(FIX_PW).unwrap());
        let len = d
            .dispatch_post(&req, &mut buf, &rt_peer(100, operator))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{}", resp);
        assert_eq!(d.auth.sessions_active(), 2);
    }

    /// Konsistensi kontrak: `POST /api/v1/auth/login` satu-satunya jalur tulis
    /// anonim, dan ia tetap tunduk pada proteksi same-origin.
    #[test]
    fn test_34_login_anonim_tetap_terikat_host_dan_kode_kredensial() {
        let mut d = admin_sess();
        let mut buf = [0u8; 512];
        // Host asing => 403 host_mismatch, sebelum sempat mencoba kredensial.
        let body = "user=auditor&password=rinjani-aud02-kata-sandi-tes";
        let raw = mk(format!(
            "POST /api/v1/auth/login HTTP/1.1\r\nHost: 203.0.113.9\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        ));
        let len = d
            .dispatch_post(&raw, &mut buf, &rt_peer(0, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 403 Forbidden"), "{}", resp);
        assert!(resp.contains("\"error\":\"host_mismatch\""), "{}", resp);

        // Body tanpa field => 400, tanpa membuka sesi.
        let req = mk(alloc::string::String::from(
            "POST /api/v1/auth/login HTTP/1.1\r\nHost: 198.51.100.77\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 4\r\n\r\nx=1y"
        ));
        let len = d
            .dispatch_post(&req, &mut buf, &rt_peer(30, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 400 Bad Request"), "{}", resp);
        assert_eq!(d.auth.sessions_active(), 1, "hanya sesi fixture");

        // Login tidak membaca header Authorization: sesi lama tidak tergantikan
        // oleh login baru yang sah dari klien berbeda.
        let req = login_post("auditor", core::str::from_utf8(FIX_PW).unwrap());
        let len = d
            .dispatch_post(&req, &mut buf, &rt_peer(60, [198, 51, 100, 9]))
            .unwrap();
        assert!(core::str::from_utf8(&buf[..len])
            .unwrap()
            .contains("HTTP/1.1 200 OK"));
        assert_eq!(d.auth.sessions_active(), 2);
        assert!(
            d.auth.authorize(Some(FIX_SESSION), false, 60).is_ok(),
            "sesi fixture harus tetap hidup"
        );
    }

    // ------------------------------------------------------ User Manager (P0.1-D)
    const NEW_PW: &str = "sandi-baru-untuk-uji-01";

    /// POST `/api/v1/auth/users` dengan bearer sesi fixture admin.
    fn users_post(d: &mut ApiDispatcher, body: &str, tick: u64) -> alloc::string::String {
        let req = mk(format!(
            "POST /api/v1/auth/users HTTP/1.1\r\nHost: 198.51.100.77\r\nAuthorization: {}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{}",
            FIX_SESSION,
            body.len(),
            body
        ));
        let mut r = rt_peer(tick, [198, 51, 100, 77]);
        let mut buf = [0u8; 512];
        let len = d
            .dispatch_post(&req, &mut buf, &r)
            .expect("POST /api/v1/auth/users harus terlayani");
        alloc::string::String::from(core::str::from_utf8(&buf[..len]).unwrap())
    }

    /// GET `/api/v1/auth/users`; `None` berarti klien anonim tanpa header.
    fn users_get(d: &mut ApiDispatcher, bearer: Option<&str>, tick: u64) -> alloc::string::String {
        let req = match bearer {
            Some(b) => req_bearer(b"GET /api/v1/auth/users HTTP/1.1\r\n\r\n", b),
            None => req_anon(b"GET /api/v1/auth/users HTTP/1.1\r\n\r\n"),
        };
        let r = rt_peer(tick, [198, 51, 100, 77]);
        let mut buf = [0u8; 512];
        let len = d
            .dispatch(&req, tick, &mut buf, &r)
            .expect("GET /api/v1/auth/users harus terlayani");
        alloc::string::String::from(core::str::from_utf8(&buf[..len]).unwrap())
    }

    #[test]
    fn test_35_daftar_akun_hanya_admin_dan_bebas_secret() {
        let mut d = admin_sess();
        let resp = users_get(&mut d, Some(FIX_TOKEN_HEX), 0);
        assert!(resp.contains("HTTP/1.1 200 OK"), "{}", resp);
        assert!(resp.contains("\"user\":\"auditor\""), "{}", resp);
        assert!(resp.contains("\"role\":\"admin\""), "{}", resp);
        assert!(resp.contains("\"rounds\":1024"), "{}", resp);
        // Nama, role, dan iterasi saja yang keluar. Garam, hash, kata sandi,
        // dan token sesi tidak boleh muncul di badan respons apa pun.
        for leak in [
            "salt",
            "hash",
            "password",
            FIX_TOKEN_HEX,
            core::str::from_utf8(FIX_PW).unwrap(),
        ] {
            assert!(!resp.contains(leak), "daftar akun membocorkan {leak}");
        }

        // Role viewer: membaca daftar akun adalah aksi admin.
        let mut v = viewer_sess();
        let resp = users_get(&mut v, Some(FIX_TOKEN_HEX), 0);
        assert!(resp.contains("HTTP/1.1 403 Forbidden"), "{}", resp);
        assert!(resp.contains("\"error\":\"insufficient_role\""), "{}", resp);

        // Anonim: 401, bukan 404 — jalur ini tidak bisa dienumerasi.
        let resp = users_get(&mut d, None, 1);
        assert!(resp.contains("HTTP/1.1 401 Unauthorized"), "{}", resp);
        assert!(resp.contains("\"error\":\"unauthenticated\""), "{}", resp);
    }

    /// Penyeimbang kurung: menelusuri badan respons, mengabaikan isi string
    /// (termasuk kutip yang terlari). Mengembalikan false hanya bila badan
    /// bukan objek JSON yang dapat diparse peramban.
    fn json_balanced(body: &str) -> bool {
        let mut depth: i32 = 0;
        let mut in_str = false;
        let mut esc = false;
        for ch in body.chars() {
            if in_str {
                if esc {
                    esc = false;
                } else if ch == '\\' {
                    esc = true;
                } else if ch == '"' {
                    in_str = false;
                }
                continue;
            }
            match ch {
                '"' => in_str = true,
                '{' | '[' => depth += 1,
                '}' | ']' => {
                    depth -= 1;
                    if depth < 0 {
                        return false;
                    }
                }
                _ => {}
            }
        }
        !in_str && depth == 0
    }

    /// Setiap jalur baca wajib menghasilkan sampul JSON yang berimbang. Cacat
    /// nyata yang ditutup di sini: `/api/v1/auth/users` dulu menutup sampul
    /// dengan `]}}` — tiga penutup untuk dua pembuka. Kernel menjawab 200 dan
    /// teksnya memuat nama akun, jadi tes berbasis `contains` tetap hijau,
    /// tetapi `JSON.parse` peramban menolak badan itu sehingga User Manager
    /// tampil kosong. Status HTTP yang benar tidak membuktikan badan terbaca.
    #[test]
    fn test_35b_seluruh_jalur_baca_menghasilkan_json_berimbang() {
        const PATHS: [&str; 18] = [
            "/api/v1/system",
            "/api/v1/interfaces",
            "/api/v1/firewall/rules",
            "/api/v1/nat",
            "/api/v1/nat/rules",
            "/api/v1/nat/conntrack",
            "/api/v1/wireless",
            "/api/v1/wireless/devices",
            "/api/v1/olt",
            "/api/v1/olt/ons",
            "/api/v1/services",
            "/api/v1/writes/last",
            "/api/v1/arp",
            "/api/v1/arp/stats",
            "/api/v1/routes",
            "/api/v1/routes/stats",
            "/api/v1/dns",
            "/api/v1/auth/users",
        ];
        let mut d = admin_sess();
        for p in PATHS.iter() {
            let req = mk(alloc::format!(
                "GET {} HTTP/1.1\r\nAuthorization: Bearer {}\r\n\r\n",
                p,
                FIX_TOKEN_HEX
            ));
            let r = rt_peer(0, [198, 51, 100, 77]);
            let mut buf = [0u8; 2048];
            let len = match d.dispatch(&req, 0, &mut buf, &r) {
                Ok(n) => n,
                Err(why) => panic!("{p} harus terlayani, dapat {why}"),
            };
            let txt = alloc::string::String::from(core::str::from_utf8(&buf[..len]).unwrap());
            let body = txt.splitn(2, "\r\n\r\n").nth(1).unwrap_or("");
            assert!(
                body.starts_with("{\"ok\":"),
                "{p} badan bukan sampul ok: {body}"
            );
            assert!(json_balanced(body), "{p} JSON tidak berimbang: {body}");
        }
    }

    #[test]
    fn test_36_users_set_del_dan_penjaga_akun_terakhir() {
        let mut v = viewer_sess();
        let resp = users_post(
            &mut v,
            &format!("action=set&user=operator&role=viewer&password={NEW_PW}"),
            0,
        );
        assert!(resp.contains("HTTP/1.1 403 Forbidden"), "{}", resp);
        assert_eq!(v.auth.user_count(), 1, "viewer tidak boleh menambah akun");
        assert!(
            !v.auth_dirty,
            "percobaan viewer tidak boleh menjadwalkan disk"
        );

        let mut d = admin_sess();
        // Pasang akun viewer baru: kata sandi hanya dipakai di dalam handler.
        let resp = users_post(
            &mut d,
            &format!("action=set&user=operator&role=viewer&password={NEW_PW}"),
            10,
        );
        assert!(resp.contains("HTTP/1.1 200 OK"), "{}", resp);
        assert!(resp.contains("\"action\":\"set\""), "{}", resp);
        assert!(resp.contains("\"role\":\"viewer\""), "{}", resp);
        assert!(resp.contains("\"persist\":\"queued\""), "{}", resp);
        assert!(!resp.contains(NEW_PW), "ACK mengembalikan kata sandi");
        assert_eq!(d.auth.user_count(), 2);
        assert!(
            d.auth_dirty,
            "perubahan kredensial wajib dijadwalkan ke RJCFG01"
        );
        assert_eq!(
            d.auth.verify_password("operator", NEW_PW.as_bytes()),
            Some(Role::Viewer),
            "akun baru harus benar-benar tersimpan"
        );

        // Hapus akun kedua: tersisa satu akun, penjaga belum aktif.
        let resp = users_post(&mut d, "action=del&user=operator", 20);
        assert!(resp.contains("HTTP/1.1 200 OK"), "{}", resp);
        assert_eq!(d.auth.user_count(), 1);
        assert!(
            d.auth.authorize(Some(FIX_SESSION), false, 20).is_ok(),
            "sesi admin tidak boleh ikut tercabut oleh penghapusan akun lain"
        );

        // Menghapus satu-satunya akun ditolak: WebUI tidak boleh mengunci
        // pemiliknya sendiri (pemulihan tetap lewat konsol serial).
        let resp = users_post(&mut d, "action=del&user=auditor", 30);
        assert!(resp.contains("\"error\":\"last_account\""), "{}", resp);
        assert_eq!(d.auth.user_count(), 1);

        // Admin terakhir juga dilindungi ketika masih ada akun non-admin.
        users_post(
            &mut d,
            &format!("action=set&user=penonton&role=viewer&password={NEW_PW}"),
            40,
        );
        let resp = users_post(&mut d, "action=del&user=auditor", 50);
        assert!(resp.contains("\"error\":\"last_admin\""), "{}", resp);
        assert_eq!(d.auth.user_count(), 2);

        // Nama tak dikenal dan aksi tak dikenal ditolak tanpa mengubah tabel.
        let resp = users_post(&mut d, "action=del&user=tidak-ada", 60);
        assert!(resp.contains("\"error\":\"unknown_user\""), "{}", resp);
        let resp = users_post(&mut d, "action=reset&user=auditor", 70);
        assert!(resp.contains("\"error\":\"bad_user_action\""), "{}", resp);

        // revoke sesi adalah aksi nontulis-kredensial: cabut semua sesi tanpa
        // berjanji persistensi ke disk.
        d.auth_dirty = false;
        let resp = users_post(&mut d, "action=revoke&user=auditor", 80);
        assert!(resp.contains("HTTP/1.1 200 OK"), "{}", resp);
        assert!(resp.contains("\"sessions_revoked\":1"), "{}", resp);
        assert_eq!(d.auth.sessions_active(), 0);
        assert!(
            !d.auth_dirty,
            "revoke sesi bukan perubahan kredensial, tidak ditulis ke disk"
        );
        assert_eq!(d.auth.user_count(), 2, "revoke tidak menghapus akun");
    }

    #[test]
    fn test_37_mengganti_sandi_sendiri_mencabut_sesi_sendiri() {
        let mut d = admin_sess();
        assert!(d.auth.authorize(Some(FIX_SESSION), true, 0).is_ok());
        let resp = users_post(
            &mut d,
            &format!("action=set&user=auditor&role=admin&password={NEW_PW}"),
            10,
        );
        assert!(resp.contains("HTTP/1.1 200 OK"), "{}", resp);
        // Sesi lama milik akun yang kredensialnya berubah tidak boleh hidup.
        assert!(
            d.auth.authorize(Some(FIX_SESSION), false, 10).is_err(),
            "sesi lama masih diterima setelah sandi diganti"
        );
        // Write berikutnya dari klien yang sama otomatis ditolak: tidak ada
        // jalur penulisan yang tetap terbuka padahal identitasnya sudah dicabut.
        // Token yang tidak lagi ditemui di tabel sesi dilaporkan sebagai
        // `session_expired` (lihat `AuthStore::authorize`), bukan sebagai header
        // cacat, supaya klien tahu yang perlu dilakukan adalah masuk kembali.
        let resp = users_post(&mut d, "action=revoke&user=auditor", 20);
        assert!(resp.contains("HTTP/1.1 401 Unauthorized"), "{}", resp);
        assert!(resp.contains("\"error\":\"session_expired\""), "{}", resp);
        // Kata sandi baru yang sah dipakai, yang lama mati.
        assert_eq!(
            d.auth.verify_password("auditor", NEW_PW.as_bytes()),
            Some(Role::Admin)
        );
        assert_eq!(
            d.auth
                .verify_password("auditor", core::str::from_utf8(FIX_PW).unwrap().as_bytes()),
            None
        );
    }

    /// Tabel ARP hanya boleh menampilkan tetangga yang benar-benar dilaporkan
    /// kernel: baris memuat antarmuka, MAC hasil belajar, asal frame, dan umur
    /// yang dihitung dari tick permintaan.
    #[test]
    fn test_39_arp_melaporkan_tetangga_hasil_belajar_kernel() {
        let mut d = admin_sess();
        let mut buf = [0u8; 1024];
        let req = req_sess(b"GET /api/v1/arp HTTP/1.1\r\n\r\n");
        let len = d
            .dispatch(&req, 1000, &mut buf, &rt_peer(1000, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{resp}");
        // Baris WAN: disegarkan ARP reply, umur = (1000-990) tick = 1 detik.
        assert!(resp.contains("\"iface\":\"eth0\""), "{resp}");
        assert!(resp.contains("\"ip\":\"198.51.100.50\""), "{resp}");
        assert!(resp.contains("\"mac\":\"08:00:27:aa:bb:cc\""), "{resp}");
        assert!(resp.contains("\"source\":\"reply\""), "{resp}");
        assert!(resp.contains("\"age_s\":1}"), "{resp}");
        // Baris LAN: nama antarmuka diambil dari rt.lan_name, asal request,
        // umur 2 detik.
        assert!(resp.contains("\"iface\":\"eth1\""), "{resp}");
        assert!(resp.contains("\"ip\":\"192.0.2.9\""), "{resp}");
        assert!(resp.contains("\"source\":\"request\""), "{resp}");
        assert!(resp.contains("\"age_s\":2}"), "{resp}");
        // Dinamis: kernel tidak punya entri statis, jadi tidak boleh ada 'S'.
        assert!(resp.contains("\"type\":\"D\""), "{resp}");
        assert!(!resp.contains("\"type\":\"S\""), "{resp}");
    }

    /// Statistik tabel memuat kapasitas dan angka penggusuran/penolakan, supaya
    /// tabel yang penuh tidak tampak seperti tabel normal.
    #[test]
    fn test_40_arp_stats_melaporkan_kapasitas_dan_penolakan() {
        let mut d = admin_sess();
        let mut buf = [0u8; 512];
        let req = req_sess(b"GET /api/v1/arp/stats HTTP/1.1\r\n\r\n");
        let len = d
            .dispatch(&req, 1000, &mut buf, &rt_peer(1000, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{resp}");
        for want in [
            "\"entries\":2",
            "\"capacity\":16",
            "\"timeout_s\":20",
            "\"evictions\":0",
            "\"rejected\":4",
            "\"snapshot\":2",
        ] {
            assert!(resp.contains(want), "{want} hilang: {resp}");
        }

        // Kernel yang belum melihat siapa pun harus menjawab tabel kosong,
        // bukan baris karangan.
        let mut r = rt_peer(1000, [198, 51, 100, 9]);
        r.arp_list = &[];
        r.arp_count = 0;
        let mut buf2 = [0u8; 512];
        let len = d.dispatch(&req, 1000, &mut buf2, &r).unwrap();
        let resp2 = core::str::from_utf8(&buf2[..len]).unwrap();
        assert!(resp2.contains("\"entries\":0"), "{resp2}");

        let req2 = req_sess(b"GET /api/v1/arp HTTP/1.1\r\n\r\n");
        let mut buf3 = [0u8; 512];
        let len = d.dispatch(&req2, 1000, &mut buf3, &r).unwrap();
        let resp3 = core::str::from_utf8(&buf3[..len]).unwrap();
        assert!(resp3.contains("\"data\":[]"), "{resp3}");
        assert!(!resp3.contains("\"mac\""), "{resp3}");
    }

    /// Role viewer boleh membaca tabel tetangga (data operasional), bukan daftar
    /// akun.
    #[test]
    fn test_41_viewer_membaca_arp_tetapi_bukan_daftar_akun() {
        let mut d = viewer_sess();
        let mut buf = [0u8; 1024];
        let req = req_sess(b"GET /api/v1/arp HTTP/1.1\r\n\r\n");
        let len = d
            .dispatch(&req, 1000, &mut buf, &rt_peer(1000, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{resp}");

        // Tabel rute juga data operasional: viewer boleh melihat FIB.
        let req_r = req_sess(b"GET /api/v1/routes HTTP/1.1\r\n\r\n");
        let mut buf_r = [0u8; 1024];
        let len = d
            .dispatch(&req_r, 1001, &mut buf_r, &rt_peer(1001, [198, 51, 100, 9]))
            .unwrap();
        let resp_r = core::str::from_utf8(&buf_r[..len]).unwrap();
        assert!(resp_r.contains("HTTP/1.1 200 OK"), "{resp_r}");

        let mut buf2 = [0u8; 512];
        let req2 = req_sess(b"GET /api/v1/auth/users HTTP/1.1\r\n\r\n");
        let len = d
            .dispatch(&req2, 1002, &mut buf2, &rt_peer(1002, [198, 51, 100, 9]))
            .unwrap();
        let resp2 = core::str::from_utf8(&buf2[..len]).unwrap();
        assert!(resp2.contains("HTTP/1.1 403 Forbidden"), "{resp2}");
    }

    /// Validasi form penolakan harus datang sebelum `auth_dirty` diangkat, dan
    /// alasan penolakannya hanya kode — tidak pernah nilai yang dikirim klien.
    #[test]
    fn test_42_validasi_akun_hanya_memakai_dasar_form() {
        let mut d = admin_sess();
        for (body, code) in [
            (
                "user=admin&role=viewer&password=sandi-baru-untuk-uji-01",
                "missing_action",
            ),
            (
                "action=set&role=viewer&password=sandi-baru-untuk-uji-01",
                "missing_user",
            ),
            (
                "action=set&user=admin&password=sandi-baru-untuk-uji-01",
                "bad_role",
            ),
            ("action=set&user=admin&role=viewer", "missing_password"),
            (
                "action=set&user=admin&role=viewer&password=pendek1",
                "password_too_short",
            ),
            ("action=hapus&user=admin", "bad_user_action"),
        ] {
            d.auth_dirty = false;
            let resp = users_post(&mut d, body, 0);
            assert!(resp.contains("HTTP/1.1 400"), "{body} => {resp}");
            assert!(resp.contains(code), "{body} => harapkan {code}: {resp}");
            assert!(
                !d.auth_dirty,
                "{body}: penolakan tidak boleh mengangkat bendera persistensi"
            );
            // Password klien tidak pernah diulangi di alasan error.
            assert!(!resp.contains("sandi-baru-untuk-uji-01"), "{resp}");
        }
    }

    #[test]
    fn test_38_validasi_pengguna_ditolak_tanpa_jejak_sandi() {
        let mut d = admin_sess();
        let cases: [(&str, &str); 5] = [
            (
                "action=set&user=operator1&role=viewer&password=pendek",
                "password_too_short",
            ),
            (
                "action=set&user=operator&role=supervisor&password=",
                "bad_role",
            ),
            ("action=set&user=operator&role=viewer", "missing_password"),
            ("action=set&password=", "missing_user"),
            ("user=operator", "missing_action"),
        ];
        for (i, (body, code)) in cases.iter().enumerate() {
            let tick = 10 + i as u64 * 10;
            let body = if body.ends_with("password=") {
                alloc::format!("{body}{NEW_PW}")
            } else {
                alloc::string::String::from(*body)
            };
            let resp = users_post(&mut d, &body, tick);
            assert!(
                resp.contains("HTTP/1.1 400 Bad Request"),
                "{body} => {resp}"
            );
            assert!(
                resp.contains(&alloc::format!("\"error\":\"{code}\"")),
                "{body} => {resp}"
            );
            assert!(
                !resp.contains(NEW_PW) && !resp.contains("pendek"),
                "respons penolakan mengulang kata sandi: {resp}"
            );
        }
        // Tabel tidak berubah sama sekali oleh permintaan yang cacat.
        assert_eq!(d.auth.user_count(), 1);
        assert!(!d.auth_dirty);

        // Content-type asing ditolak, bukan diabaikan.
        let req = mk(alloc::string::String::from(
            "POST /api/v1/auth/users HTTP/1.1\r\nHost: 198.51.100.77\r\nAuthorization: Bearer 6b28ce8e57533bbca7a3f5ffea484eb65a528ab6796d6853614133a2cb60c23f\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
        ));
        let mut buf = [0u8; 512];
        let len = d
            .dispatch_post(&req, &mut buf, &rt_peer(200, [198, 51, 100, 77]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(
            resp.contains("\"error\":\"unsupported_content_type\""),
            "{}",
            resp
        );
    }

    /// FIB hanyalah salinan isi tabel rute kernel. Tes ini menuntut tiga hal:
    /// baris muncul persis seperti snapshot (prefix, gateway, metrik, tipe),
    /// nomor antarmuka diterjemahkan memakai nama yang dilaporkan kernel, dan
    /// rute tanpa antarmuka keluar tidak boleh diberi nama antarmuka yang tidak
    /// ada.
    #[test]
    fn test_43_routes_melaporkan_isi_fib_kernel() {
        let mut d = admin_sess();
        let mut buf = [0u8; 2048];
        let req = req_sess(b"GET /api/v1/routes HTTP/1.1\r\n\r\n");
        let len = d
            .dispatch(&req, 1000, &mut buf, &rt_peer(1000, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{resp}");
        // Rute default via gateway WAN.
        assert!(
            resp.contains(
                "{\"dst\":\"0.0.0.0\",\"prefix\":0,\"gw\":\"198.51.100.254\",\"iface\":\"eth0\",\"metric\":100,\"type\":\"static\"}"
            ),
            "{resp}"
        );
        // Rute connected subnet WAN: tanpa gateway, metrik 0.
        assert!(
            resp.contains(
                "{\"dst\":\"198.51.100.0\",\"prefix\":24,\"gw\":\"0.0.0.0\",\"iface\":\"eth0\",\"metric\":0,\"type\":\"connected\"}"
            ),
            "{resp}"
        );
        // Rute tanpa antarmuka keluar -> strip, bukan nama yang dikarang.
        assert!(
            resp.contains(
                "{\"dst\":\"10.99.0.0\",\"prefix\":16,\"gw\":\"0.0.0.0\",\"iface\":\"-\",\"metric\":0,\"type\":\"blackhole\"}"
            ),
            "{resp}"
        );
        assert_eq!(resp.matches("\"dst\":").count(), 3, "{resp}");

        // Kernel tanpa rute harus menjawab daftar kosong.
        let mut r = rt_peer(1000, [198, 51, 100, 9]);
        r.route_list = &[];
        r.route_count = 0;
        let mut buf2 = [0u8; 512];
        let len = d.dispatch(&req, 1000, &mut buf2, &r).unwrap();
        let resp2 = core::str::from_utf8(&buf2[..len]).unwrap();
        assert!(resp2.contains("\"data\":[]"), "{resp2}");
        assert!(!resp2.contains("\"dst\""), "{resp2}");
    }

    /// Statistik rute memisahkan "tabel terisi" dari " datapath benar-benar
    /// bertanya". Tanpa LOOKUP nyata angka harus tetap nol — inilah yang mencegah
    /// halaman Gateway terlihat aktif padahal `route_packet` tidak dipanggil.
    #[test]
    fn test_44_routes_stats_memisahkan_tabel_dari_pemakaian() {
        let mut d = admin_sess();
        let mut buf = [0u8; 512];
        let req = req_sess(b"GET /api/v1/routes/stats HTTP/1.1\r\n\r\n");
        let len = d
            .dispatch(&req, 1000, &mut buf, &rt_peer(1000, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{resp}");
        for want in [
            "\"entries\":3",
            "\"capacity\":64",
            "\"lookups\":7",
            "\"forwards\":0",
            "\"drops\":7",
            "\"default_active\":true",
            "\"default_gw\":\"198.51.100.254\"",
            "\"snapshot\":3",
        ] {
            assert!(resp.contains(want), "{want} hilang: {resp}");
        }

        // Tanpa rute default: `default_active` harus false dan gw nol, bukan
        // mewarisi alamat dari baris lain.
        let mut r = rt_peer(1000, [198, 51, 100, 9]);
        r.route_list = &ROUTES[1..2];
        r.route_count = 1;
        r.route_lookups = 0;
        r.route_drops = 0;
        let mut buf2 = [0u8; 512];
        let len = d.dispatch(&req, 1000, &mut buf2, &r).unwrap();
        let resp2 = core::str::from_utf8(&buf2[..len]).unwrap();
        assert!(resp2.contains("\"default_active\":false"), "{resp2}");
        assert!(resp2.contains("\"default_gw\":\"0.0.0.0\""), "{resp2}");
        assert!(resp2.contains("\"lookups\":0"), "{resp2}");
        assert!(resp2.contains("\"entries\":1"), "{resp2}");
    }

    /// `GET /api/v1/dns` memisahkan dua fakta: apa yang dibawa lease dan apa
    /// yang kini dipegang mesin proksi. Keduanya harus bisa berbeda — kalau
    /// endpoint hanya mengulang angka yang sama, klaim "DNS tersalur" tidak
    /// terbukti.
    #[test]
    fn test_45_dns_memisahkan_lease_dari_mesin_proksi() {
        let mut d = admin_sess();
        let mut buf = [0u8; 1024];
        let req = req_sess(b"GET /api/v1/dns HTTP/1.1\r\n\r\n");
        let len = d
            .dispatch(&req, 1000, &mut buf, &rt_peer(1000, [198, 51, 100, 9]))
            .unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{resp}");
        for want in [
            "\"config_mode\":\"dhcp\"",
            "\"resolver_1\":\"198.51.100.53\"",
            "\"resolver_2\":\"0.0.0.0\"",
            "\"resolver_src\":\"option-6-offer\"",
            "\"from_dhcp\":true",
            "\"proxy_upstream\":\"198.51.100.53\"",
            "\"proxy_upstream_ready\":true",
            "\"serving\":false",
            "\"cache_hits\":11",
            "\"cache_misses\":5",
            "\"cache_entries\":3",
            "\"cache_capacity\":16",
            "\"static_hosts\":1",
            "\"hosts_capacity\":8",
        ] {
            assert!(resp.contains(want), "{want} hilang: {resp}");
        }
        assert!(
            !resp.contains("8.8.8.8") && !resp.contains("1.1.1.1"),
            "resolver publik cadangan tidak boleh muncul: {resp}"
        );
        assert!(json_pairs_balanced(resp).is_ok(), "{resp}");
    }

    /// Fail-closed: server yang tidak mengirim opsi 6 membuat endpoint DNS
    /// melaporkan ketiadaan, bukan menebak resolver atau mewarisi gateway.
    #[test]
    fn test_46_dns_tanpa_opsi_6_lapor_ketiadaan() {
        let mut d = admin_sess();
        let mut r = rt_peer(1000, [198, 51, 100, 9]);
        r.wan_dns1 = [0, 0, 0, 0];
        r.wan_dns2 = [0, 0, 0, 0];
        r.wan_dns_src = "none";
        r.dns_upstream = [0, 0, 0, 0];
        r.dns_cache_entries = 0;
        r.dns_cache_hits = 0;
        r.dns_cache_misses = 0;
        let mut buf = [0u8; 1024];
        let req = req_sess(b"GET /api/v1/dns HTTP/1.1\r\n\r\n");
        let len = d.dispatch(&req, 1000, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"resolver_1\":\"0.0.0.0\""), "{resp}");
        assert!(resp.contains("\"resolver_src\":\"none\""), "{resp}");
        assert!(resp.contains("\"from_dhcp\":false"), "{resp}");
        assert!(resp.contains("\"proxy_upstream_ready\":false"), "{resp}");
        assert!(resp.contains("\"cache_entries\":0"), "{resp}");

        // Mode statik operator: label sumber bukan rantai lease, dan tidak ada
        // klaim resolver yang datang dari DHCP.
        let mut s = rt_peer(1000, [198, 51, 100, 9]);
        s.wan_config_static = true;
        s.wan_dns_src = "static-serial";
        s.wan_dns1 = [198, 51, 100, 60];
        s.dns_upstream = [198, 51, 100, 60];
        let mut buf2 = [0u8; 1024];
        let len = d.dispatch(&req, 1000, &mut buf2, &s).unwrap();
        let resp2 = core::str::from_utf8(&buf2[..len]).unwrap();
        assert!(resp2.contains("\"config_mode\":\"static\""), "{resp2}");
        assert!(resp2.contains("\"from_dhcp\":false"), "{resp2}");
        assert!(
            resp2.contains("\"resolver_src\":\"static-serial\""),
            "{resp2}"
        );
    }

    #[test]
    fn test_47_himpunan_alamat_lokal_hanya_mengenali_entri_hidup() {
        // Gerbang alamat lokal dibaca dari `local`, bukan dari `wan_ip`: fixture
        // runtime punya satu entri hidup (alamat WAN) dan satu slot kosong.
        let mut r = rt();
        assert_eq!(r.local_match([198, 51, 100, 77]), Some(0));
        assert_eq!(r.local_match([198, 51, 100, 78]), None);
        assert_eq!(
            r.local_match([0, 0, 0, 0]),
            None,
            "slot kosong bukan alamat"
        );

        r.local[1] = WebLocalAddr {
            ip: [198, 51, 100, 10],
            mac: [0x08, 0x00, 0x27, 0x11, 0x22, 0x34],
            ifindex: 2,
        };
        r.local_count = 2;
        assert_eq!(r.local_match([198, 51, 100, 10]), Some(1));

        // `local_count` membatasi: entri yang belum dinyatakan hidup tidak boleh
        // melayani, meski isinya sudah terpasang di array.
        r.local_count = 1;
        assert_eq!(r.local_match([198, 51, 100, 10]), None);

        // Entri ber-ifindex 0 (antarmuka tidak ada) tetap mati walau IP cocok.
        r.local[0] = WebLocalAddr {
            ip: [198, 51, 100, 77],
            mac: [0; 6],
            ifindex: 0,
        };
        r.local_count = 1;
        assert_eq!(r.local_match([198, 51, 100, 77]), None);
    }

    /// IFACE-01: proteksi same-origin dibaca dari himpunan alamat lokal, bukan
    /// dari alamat WAN saja. Kalau hanya WAN yang diakui, login dari antarmuka
    /// kedua mati di 403 `host_mismatch` sebelum kata sandi diperiksa.
    #[test]
    fn test_48_login_ke_alamat_lokal_kedua_lolos_same_origin() {
        const LAN_IP: [u8; 4] = [198, 51, 100, 10];
        let mut d = admin_sess();
        let mut buf = [0u8; 512];
        let mut r = rt_peer(5, [198, 51, 100, 9]);
        r.local[1] = WebLocalAddr {
            ip: LAN_IP,
            mac: [0x08, 0x00, 0x27, 0x0A, 0x00, 0x04],
            ifindex: 2,
        };
        r.local_count = 2;
        let body = "user=auditor&password=rinjani-aud02-kata-sandi-tes";

        // Host = alamat lokal kedua => same-origin lolos dan sesi terbit.
        let req = mk(format!(
            "POST /api/v1/auth/login HTTP/1.1\r\nHost: 198.51.100.10\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        ));
        let len = d.dispatch_post(&req, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 200 OK"), "{}", resp);
        assert!(resp.contains("\"token\""), "{}", resp);

        // Host asing (bukan anggota himpunan lokal) tetap 403 sebelum kredensial.
        let req = mk(format!(
            "POST /api/v1/auth/login HTTP/1.1\r\nHost: 203.0.113.9\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        ));
        let len = d.dispatch_post(&req, &mut buf, &r).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 403 Forbidden"), "{}", resp);
        assert!(resp.contains("\"error\":\"host_mismatch\""), "{}", resp);

        // Slot mati (ifindex 0) tidak membuka jalur tulis walaupun IP-nya persis
        // dengan Host yang dikirim klien.
        let mut mati = rt_peer(6, [198, 51, 100, 9]);
        mati.local[0].ifindex = 0;
        let req = mk(format!(
            "POST /api/v1/auth/login HTTP/1.1\r\nHost: 198.51.100.77\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        ));
        let len = d.dispatch_post(&req, &mut buf, &mati).unwrap();
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("HTTP/1.1 403 Forbidden"), "{}", resp);
        assert!(resp.contains("\"error\":\"host_mismatch\""), "{}", resp);
    }
}
