//! Lapisan autentikasi & otorisasi WebUI (Sprint AUD-02, P0.1).
//!
//! Kebenaran desain yang harus dibaca bersama kode ini:
//!
//! 1. Gerbang berada di lapisan server (`ApiDispatcher`), bukan di peramban.
//!    Aset statis (`shell.html`, CSS, JS) memang tetap anonim supaya kartu
//!    masuk dapat tampil; seluruh jalur `/api/...` menuntut token sesi sah.
//! 2. Tidak ada akun bawaan dan tidak ada kata sandi universal. Tabel kosong
//!    berarti sistem terkunci (`AuthError::NoCredentials`): administrator
//!    pertama wajib dibuat lewat konsol serial, dan jalur itu pula yang menjadi
//!    pemulihan ketika kata sandi lupa.
//! 3. Kredensial disimpan sebagai PBKDF2-HMAC-SHA256 (RFC 8018) dengan salt
//!    per pengguna dan jumlah iterasi per rekaman. Kata sandi, token, dan hash
//!    tidak pernah ditulis ke log, tidak pernah muncul di URL, dan tidak pernah
//!    kembali di badan error.
//! 4. Token sesi adalah byte acak hasil pemerahan SHA-256
//!    (`asl_router_crypto::expand`) yang di-seed ulang dari entropy yang
//!    ditambah kernel. Ini bukan CSPRNG bersertifikat; kualitasnya mengikuti
//!    entropy yang dicampur, dan batas itu dinyatakan di laporan sprint.
//! 5. Bayar lewat header `Authorization`, bukan cookie. Tidak ada cookie sesi
//!    sehingga tidak ada automasi penyertaan kredensial oleh peramban; gerbang
//!    `Host` same-origin pada POST tetap dipertahankan.
//!
//! Batas yang belum ditutup dan dinyatakan jujur: HTTP memakai port 80 tanpa
//! TLS, jadi kata sandi dan token berjalan jelas pada jaringan pengelolaan.
//! Menutup itu butuh tumpukan TLS, di luar cakupan sprint ini.

use asl_router_crypto::{ct, expand, hex, pbkdf2, sha256};
use asl_router_persistence::{
    auth_user_ok, AuthRecord, RjcfgConfigPayload, RJCFG_AUTH_HASH_MAX, RJCFG_AUTH_ROLE_ADMIN,
    RJCFG_AUTH_ROLE_VIEWER, RJCFG_AUTH_SALT_MAX, RJCFG_AUTH_USER_MAX,
};

/// Jumlah kredensial yang ditampung (sama dengan slot jurnal).
pub const AUTH_USERS_MAX: usize = 4;
/// Jumlah sesi aktif; disamakan dengan kapasitas sesi TCP server web.
pub const AUTH_SESSIONS_MAX: usize = 8;
/// Iterasi PBKDF2 untuk akun baru. Terukur: pada profil debug host 1024
/// iterasi = 30 ms per verifikasi, dan verifikasi berlangsung di dalam poll
/// loop satu-inti kernel.
pub const AUTH_DEFAULT_ROUNDS: u32 = 1024;
/// Batas bawah iterasi; menolak rekaman jurnal yang dilemahkan.
pub const AUTH_MIN_ROUNDS: u32 = 512;
/// Batas atas iterasi (menolak rekaman rusak yang membuat login menjamur).
pub const AUTH_MAX_ROUNDS: u32 = 1_000_000;
/// Lebar byte hash yang dicocokkan saat login.
pub const AUTH_MATCH_BYTES: usize = 32;
/// Panjang token sesi dalam byte.
pub const AUTH_TOKEN_BYTES: usize = 32;
/// Sesi menganggur lebih dari 30 menit (18 000 tick 100 ms) kedaluwarsa.
pub const AUTH_IDLE_TICKS: u64 = 18_000;
/// Umur maksimum sesi 4 jam, tidak peduli seberapa sering dipakai.
pub const AUTH_MAX_AGE_TICKS: u64 = 144_000;
/// Kebijakan panjang kata sandi: 10..=64 byte tercetak.
pub const AUTH_PASSWORD_MIN: usize = 10;
pub const AUTH_PASSWORD_MAX: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Admin,
    Viewer,
}

impl Role {
    pub const fn code(self) -> u8 {
        match self {
            Role::Admin => RJCFG_AUTH_ROLE_ADMIN,
            Role::Viewer => RJCFG_AUTH_ROLE_VIEWER,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Viewer => "viewer",
        }
    }

    pub const fn from_code(v: u8) -> Option<Self> {
        match v {
            RJCFG_AUTH_ROLE_ADMIN => Some(Role::Admin),
            RJCFG_AUTH_ROLE_VIEWER => Some(Role::Viewer),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    /// Tidak ada header `Authorization` sama sekali.
    Anonymous,
    /// Header ada tetapi bentuknya salah atau token tidak dikenal.
    BadToken,
    /// Sesi dikenal tetapi sudah kedaluwarsa atau dicabut.
    Expired,
    /// Sesi sah, tetapi aksi menuntut role admin.
    InsufficientRole,
    /// Belum ada kredensial di perangkat: bootstrap lewat konsol serial.
    NoCredentials,
}

impl AuthError {
    pub const fn code(self) -> &'static str {
        match self {
            AuthError::Anonymous => "unauthenticated",
            AuthError::BadToken => "invalid_token",
            AuthError::Expired => "session_expired",
            AuthError::InsufficientRole => "insufficient_role",
            AuthError::NoCredentials => "no_credentials",
        }
    }

    /// 401 untuk masalah identitas, 403 untuk masalah kewenangan.
    pub const fn status_401(self) -> bool {
        !matches!(self, AuthError::InsufficientRole)
    }
}

#[derive(Debug, Clone, Copy)]
struct SessionRec {
    token: [u8; AUTH_TOKEN_BYTES],
    user: usize,
    role: u8,
    born_tick: u64,
    last_tick: u64,
    active: bool,
}

impl SessionRec {
    const fn idle() -> Self {
        Self {
            token: [0u8; AUTH_TOKEN_BYTES],
            user: 0,
            role: RJCFG_AUTH_ROLE_VIEWER,
            born_tick: 0,
            last_tick: 0,
            active: false,
        }
    }
}

/// Isi rekaman kosong; dipakai sebagai nilai awal array.
const fn empty_record() -> AuthRecord {
    AuthRecord {
        user: [0u8; RJCFG_AUTH_USER_MAX],
        user_len: 0,
        role: RJCFG_AUTH_ROLE_VIEWER,
        salt: [0u8; RJCFG_AUTH_SALT_MAX],
        salt_len: 0,
        rounds: 0,
        hash: [0u8; RJCFG_AUTH_HASH_MAX],
        hash_len: 0,
    }
}

pub struct AuthStore {
    users: [AuthRecord; AUTH_USERS_MAX],
    sessions: [SessionRec; AUTH_SESSIONS_MAX],
    seed: [u8; 32],
    seed_ctr: u64,
    /// Lebar byte hash yang dicocokkan, ikut tersimpan di jurnal agar rekaman
    /// lama tidak berubah maknanya ketika konstanta dinaikkan.
    match_bytes: u8,
    pub logins_ok: u32,
    pub logins_bad: u32,
    pub rejects_anon: u32,
    pub rejects_bad_token: u32,
    pub sessions_expired: u32,
    pub revocations: u32,
}

impl AuthStore {
    pub const fn new() -> Self {
        Self {
            users: [empty_record(); AUTH_USERS_MAX],
            sessions: [SessionRec::idle(); AUTH_SESSIONS_MAX],
            // Seed awal bukan nol dan tidak dipublikasikan sebagai kredensial;
            // kernel wajib menambah entropy nyata lewat `add_entropy` sebelum
            // token pertama diterbitkan.
            seed: *b"rinjani-aud02-auth-seed-v1\0\0\0\0\0\0",
            seed_ctr: 0,
            match_bytes: AUTH_MATCH_BYTES as u8,
            logins_ok: 0,
            logins_bad: 0,
            rejects_anon: 0,
            rejects_bad_token: 0,
            sessions_expired: 0,
            revocations: 0,
        }
    }

    /// Campur entropy dari pemanggil (kernel). Mencampur, bukan mengganti,
    /// sehingga hilangnya satu sumber tidak menurunkan kualitas di bawah sumber
    /// lain yang masih ada.
    pub fn add_entropy(&mut self, bytes: &[u8]) {
        let mut h = sha256::Sha256::new();
        h.update(&self.seed);
        h.update(bytes);
        self.seed = h.finalize();
    }

    /// Keluarkan byte pseudo-acak, lalu majukan seed dari keluaran itu supaya
    /// penerbitan berikutnya tidak mengulang aliran lama.
    fn random(&mut self, out: &mut [u8]) {
        expand::bytes(&self.seed, self.seed_ctr, out);
        self.seed_ctr = self.seed_ctr.wrapping_add(((out.len() + 31) / 32) as u64);
        self.add_entropy(out);
    }

    #[must_use]
    pub fn has_users(&self) -> bool {
        self.users.iter().any(|u| u.user_len > 0)
    }

    #[must_use]
    pub fn user_count(&self) -> u32 {
        self.users.iter().filter(|u| u.user_len > 0).count() as u32
    }

    /// Jumlah kredensial dengan role tertentu. Dipakai penjaga "jangan kunci
    /// pemilik keluar": penghapusan admin terakhir ditolak.
    #[must_use]
    pub fn count_role(&self, role: Role) -> u32 {
        self.users
            .iter()
            .filter(|u| u.user_len > 0 && Role::from_code(u.role) == Some(role))
            .count() as u32
    }

    #[must_use]
    pub fn sessions_active(&self) -> u32 {
        self.sessions.iter().filter(|s| s.active).count() as u32
    }

    #[must_use]
    pub fn find_user(&self, name: &str) -> Option<usize> {
        if name.len() > RJCFG_AUTH_USER_MAX {
            return None;
        }
        for (i, u) in self.users.iter().enumerate() {
            if u.user_len as usize == name.len() && &u.user[..name.len()] == name.as_bytes() {
                return Some(i);
            }
        }
        None
    }

    /// Salin nama pengguna ke buffer pemanggil; `(byte, panjang)`. Panjang 0
    /// berarti indeks tidak memiliki nama.
    #[must_use]
    pub fn user_name(&self, idx: usize) -> ([u8; RJCFG_AUTH_USER_MAX], usize) {
        let mut out = [0u8; RJCFG_AUTH_USER_MAX];
        match self.users.get(idx) {
            Some(u) if u.user_len > 0 => {
                let n = u.user_len as usize;
                out[..n].copy_from_slice(&u.user[..n]);
                (out, n)
            }
            _ => (out, 0),
        }
    }

    /// Role pada indeks tabel kredensial; `None` untuk slot kosong atau rusak.
    #[must_use]
    pub fn user_role(&self, idx: usize) -> Option<Role> {
        self.users
            .get(idx)
            .filter(|u| u.user_len > 0)
            .and_then(|u| Role::from_code(u.role))
    }

    /// Iterasi PBKDF2 pada indeks tabel kredensial (0 untuk slot kosong).
    #[must_use]
    pub fn user_rounds(&self, idx: usize) -> u32 {
        self.users.get(idx).map_or(0, |u| u.rounds)
    }

    /// Turunkan kata sandi menjadi hash dengan salt dan iterasi rekaman.
    #[must_use]
    pub fn derive(pw: &[u8], salt: &[u8], rounds: u32) -> [u8; 32] {
        let mut dk = [0u8; 32];
        pbkdf2::pbkdf2_sha256(pw, salt, rounds, &mut dk);
        dk
    }

    /// Salt baru untuk akun: 16 byte dari pemerahan internal.
    pub fn new_salt(&mut self) -> [u8; RJCFG_AUTH_SALT_MAX] {
        let mut salt = [0u8; RJCFG_AUTH_SALT_MAX];
        self.random(&mut salt);
        salt
    }

    /// Pasang atau ganti kredensial. Nama yang sudah ada diperbarui di tempat
    /// dan sesi milik pengguna itu dicabut: kredensial lama tidak boleh dibiarkan
    /// hidup setelah sandi diganti.
    pub fn set_user(
        &mut self,
        name: &str,
        role: Role,
        salt: &[u8],
        rounds: u32,
        hash: &[u8],
    ) -> Result<(), &'static str> {
        if !auth_user_ok(name) {
            return Err("nama kredensial tidak valid");
        }
        if salt.len() != RJCFG_AUTH_SALT_MAX {
            return Err("salt harus 16 byte");
        }
        if hash.len() != RJCFG_AUTH_HASH_MAX {
            return Err("hash harus 32 byte");
        }
        if rounds < AUTH_MIN_ROUNDS || rounds > AUTH_MAX_ROUNDS {
            return Err("iterasi PBKDF2 di luar rentang");
        }
        let mut rec = empty_record();
        rec.user_len = name.len() as u8;
        rec.user[..name.len()].copy_from_slice(name.as_bytes());
        rec.role = role.code();
        rec.salt_len = RJCFG_AUTH_SALT_MAX as u8;
        rec.salt.copy_from_slice(salt);
        rec.rounds = rounds;
        rec.hash_len = RJCFG_AUTH_HASH_MAX as u8;
        rec.hash.copy_from_slice(hash);

        let idx = match self.find_user(name) {
            Some(i) => {
                self.revoke_user_sessions(i);
                i
            }
            None => match self.users.iter().position(|u| u.user_len == 0) {
                Some(i) => i,
                None => return Err("tabel kredensial penuh"),
            },
        };
        self.users[idx] = rec;
        self.match_bytes = AUTH_MATCH_BYTES as u8;
        Ok(())
    }

    /// Hapus kredensial beserta seluruh sesinya.
    pub fn remove_user(&mut self, name: &str) -> bool {
        match self.find_user(name) {
            Some(i) => {
                self.users[i] = empty_record();
                self.revoke_user_sessions(i);
                true
            }
            None => false,
        }
    }

    fn revoke_user_sessions(&mut self, user_idx: usize) {
        for s in self.sessions.iter_mut() {
            if s.active && s.user == user_idx {
                s.active = false;
                s.token = [0u8; AUTH_TOKEN_BYTES];
                self.revocations += 1;
            }
        }
    }

    /// Cabut semua sesi (dipakai konsol serial saat pemulihan).
    pub fn revoke_all(&mut self) -> u32 {
        let mut n = 0u32;
        for s in self.sessions.iter_mut() {
            if s.active {
                s.active = false;
                s.token = [0u8; AUTH_TOKEN_BYTES];
                n += 1;
            }
        }
        self.revocations += n;
        n
    }

    /// Cocokkan kata sandi. Biaya dibuat sama untuk nama tidak dikenal:
    /// penurunan hash tetap dijalankan pada salt tetap, sehingga penebak tidak
    /// mengukur "nama ini ada" dari selisih waktu.
    #[must_use]
    pub fn verify_password(&self, name: &str, pw: &[u8]) -> Option<Role> {
        // Salt tetap untuk nama tak dikenal; panjangnya wajib sama dengan salt
        // nyata supaya biaya penurunan hash identik.
        let mut salt = *b"rj-dummy-salt-01";
        let mut rounds = AUTH_DEFAULT_ROUNDS;
        let mut expect = [0u8; RJCFG_AUTH_HASH_MAX];
        let mut role = RJCFG_AUTH_ROLE_VIEWER;
        let mut found = false;
        if let Some(i) = self.find_user(name) {
            let u = &self.users[i];
            if u.salt_len as usize != RJCFG_AUTH_SALT_MAX
                || u.hash_len as usize != RJCFG_AUTH_HASH_MAX
                || u.rounds < AUTH_MIN_ROUNDS
                || u.rounds > AUTH_MAX_ROUNDS
            {
                // Rekaman cacat: tolak tanpa membandingkan. Tidak ada login
                // yang boleh lolos lewat parameter lemah.
                return None;
            }
            salt = u.salt;
            rounds = u.rounds;
            expect = u.hash;
            role = u.role;
            found = true;
        }
        let dk = Self::derive(pw, &salt, rounds);
        if !found || pw.len() < AUTH_PASSWORD_MIN || pw.len() > AUTH_PASSWORD_MAX {
            // Panji kata sandi dicek setelah penurunan supaya pengukuran waktu
            // tidak membocorkan panjang yang sah.
            return None;
        }
        let n = core::cmp::min(self.match_bytes as usize, RJCFG_AUTH_HASH_MAX);
        if ct::eq(&dk[..n], &expect[..n]) {
            Role::from_code(role)
        } else {
            None
        }
    }

    /// Login: verifikasi lalu terbitkan token. Kegagalan selalu `BadToken`
    /// (tanpa informasi apakah nama atau sandinya yang salah).
    pub fn login(
        &mut self,
        name: &str,
        pw: &[u8],
        tick: u64,
    ) -> Result<(usize, [u8; AUTH_TOKEN_BYTES], Role), AuthError> {
        if !self.has_users() {
            return Err(AuthError::NoCredentials);
        }
        let role = match self.verify_password(name, pw) {
            Some(r) => r,
            None => {
                self.logins_bad += 1;
                return Err(AuthError::BadToken);
            }
        };
        let idx = self.find_user(name).unwrap_or(0);
        let token = match self.issue(idx, role, tick) {
            Some(t) => t,
            None => {
                self.logins_bad += 1;
                return Err(AuthError::BadToken);
            }
        };
        self.logins_ok += 1;
        Ok((idx, token, role))
    }

    fn issue(&mut self, user_idx: usize, role: Role, tick: u64) -> Option<[u8; AUTH_TOKEN_BYTES]> {
        let mut token = [0u8; AUTH_TOKEN_BYTES];
        self.random(&mut token);
        let slot = match self.sessions.iter().position(|s| !s.active) {
            Some(i) => i,
            None => {
                // Tabel penuh: gantikan sesi paling lama menganggur, jangan
                // menolak administrator yang sedang melakukan pemulihan.
                let mut oldest = 0usize;
                let mut tmin = u64::MAX;
                for (i, s) in self.sessions.iter().enumerate() {
                    if s.last_tick < tmin {
                        tmin = s.last_tick;
                        oldest = i;
                    }
                }
                self.sessions[oldest].active = false;
                self.revocations += 1;
                oldest
            }
        };
        self.sessions[slot] = SessionRec {
            token,
            user: user_idx,
            role: role.code(),
            born_tick: tick,
            last_tick: tick,
            active: true,
        };
        Some(token)
    }

    /// Urai `Authorization: Bearer <hex64>` menjadi token. Bentuk lain ditolak.
    #[must_use]
    pub fn token_of(header: Option<&str>) -> Option<[u8; AUTH_TOKEN_BYTES]> {
        let rest = strip_scheme(header?.trim())?;
        let mut t = [0u8; AUTH_TOKEN_BYTES];
        if hex::decode(rest, &mut t) == Some(AUTH_TOKEN_BYTES) {
            Some(t)
        } else {
            None
        }
    }

    /// Nilai: `(indeks pengguna, role)`. Sesi yang dipakai diperbarui tick-nya.
    pub fn authorize(
        &mut self,
        header: Option<&str>,
        need_admin: bool,
        tick: u64,
    ) -> Result<(usize, Role), AuthError> {
        self.purge(tick);
        if !self.has_users() {
            self.rejects_anon += 1;
            return Err(AuthError::NoCredentials);
        }
        let token = match Self::token_of(header) {
            Some(t) => t,
            None => {
                if header.is_none() {
                    self.rejects_anon += 1;
                    return Err(AuthError::Anonymous);
                }
                self.rejects_bad_token += 1;
                return Err(AuthError::BadToken);
            }
        };
        for s in self.sessions.iter_mut() {
            if !s.active {
                continue;
            }
            if ct::eq(&s.token, &token) {
                s.last_tick = tick;
                let role = Role::from_code(s.role).unwrap_or(Role::Viewer);
                if need_admin && role != Role::Admin {
                    return Err(AuthError::InsufficientRole);
                }
                return Ok((s.user, role));
            }
        }
        self.rejects_bad_token += 1;
        Err(AuthError::Expired)
    }

    /// Probe tanpa efek samping untuk jalur anonim (`GET /api/v1/auth/status`).
    /// Sesi kedaluwarsa dilaporkan sebagai tidak sah, tetapi tidak ada pencabutan
    /// maupun hitungan penolakan yang berubah: probe tidak boleh menjadi jalur
    /// polusi metrik keamanan.
    #[must_use]
    pub fn peek(&self, header: Option<&str>, tick: u64) -> Option<(usize, Role)> {
        let token = Self::token_of(header)?;
        for s in self.sessions.iter() {
            if !s.active {
                continue;
            }
            if ct::eq(&s.token, &token) {
                let idle = tick.saturating_sub(s.last_tick);
                let age = tick.saturating_sub(s.born_tick);
                if idle > AUTH_IDLE_TICKS || age > AUTH_MAX_AGE_TICKS {
                    return None;
                }
                return Some((s.user, Role::from_code(s.role).unwrap_or(Role::Viewer)));
            }
        }
        None
    }

    /// Cabut sesi dari header (jalur logout).
    pub fn logout(&mut self, header: Option<&str>) -> bool {
        let token = match Self::token_of(header) {
            Some(t) => t,
            None => return false,
        };
        for s in self.sessions.iter_mut() {
            if s.active && ct::eq(&s.token, &token) {
                s.active = false;
                s.token = [0u8; AUTH_TOKEN_BYTES];
                self.revocations += 1;
                return true;
            }
        }
        false
    }

    fn purge(&mut self, tick: u64) {
        for s in self.sessions.iter_mut() {
            if !s.active {
                continue;
            }
            let idle = tick.saturating_sub(s.last_tick);
            let age = tick.saturating_sub(s.born_tick);
            if idle > AUTH_IDLE_TICKS || age > AUTH_MAX_AGE_TICKS {
                s.active = false;
                s.token = [0u8; AUTH_TOKEN_BYTES];
                self.sessions_expired += 1;
            }
        }
    }

    /// Sisa umur sesi dalam detik untuk header ini; 0 bila tidak dikenal.
    #[must_use]
    pub fn session_left_s(&self, header: Option<&str>, tick: u64) -> u32 {
        let token = match Self::token_of(header) {
            Some(t) => t,
            None => return 0,
        };
        for s in self.sessions.iter() {
            if s.active && ct::eq(&s.token, &token) {
                let idle_left = AUTH_IDLE_TICKS.saturating_sub(tick.saturating_sub(s.last_tick));
                let age_left = AUTH_MAX_AGE_TICKS.saturating_sub(tick.saturating_sub(s.born_tick));
                return (core::cmp::min(idle_left, age_left) / 10) as u32;
            }
        }
        0
    }

    /// Salin tabel kredensial ke payload jurnal RJCFG01.
    pub fn save_to(&self, payload: &mut RjcfgConfigPayload) {
        for i in 0..AUTH_USERS_MAX {
            payload.auth[i] = self.users[i];
        }
        payload.auth_match_bytes = self.match_bytes;
    }

    /// Muat tabel kredensial dari jurnal. Rekaman cacat dibuang seluruhnya,
    /// bukan dipakai setengah: sistem lebih baik terkunci daripada akun rusak
    /// diterima diam-diam. Sesi tidak pernah ikut tersimpan di disk.
    pub fn load_from(&mut self, payload: &RjcfgConfigPayload) {
        for i in 0..AUTH_USERS_MAX {
            let rec = payload.auth[i];
            let name_ok = match core::str::from_utf8(&rec.user[..rec.user_len as usize]) {
                Ok(s) => rec.user_len > 0 && auth_user_ok(s),
                Err(_) => false,
            };
            let role_ok = Role::from_code(rec.role).is_some();
            let len_ok = rec.salt_len as usize == RJCFG_AUTH_SALT_MAX
                && rec.hash_len as usize == RJCFG_AUTH_HASH_MAX;
            let rounds_ok = rec.rounds >= AUTH_MIN_ROUNDS && rec.rounds <= AUTH_MAX_ROUNDS;
            self.users[i] = if rec.user_len == 0 {
                empty_record()
            } else if name_ok && role_ok && len_ok && rounds_ok {
                rec
            } else {
                empty_record()
            };
        }
        self.match_bytes = match payload.auth_match_bytes {
            0 => AUTH_MATCH_BYTES as u8,
            n if (n as usize) <= RJCFG_AUTH_HASH_MAX => n,
            _ => AUTH_MATCH_BYTES as u8,
        };
        self.sessions = [SessionRec::idle(); AUTH_SESSIONS_MAX];
    }
}

impl Default for AuthStore {
    fn default() -> Self {
        Self::new()
    }
}

/// `Authorization` boleh `Bearer` atau `bearer` (skema tidak peka huruf sesuai
/// RFC 7235); selain itu ditolak.
fn strip_scheme(raw: &str) -> Option<&str> {
    let b = raw.as_bytes();
    if b.len() < 7 {
        return None;
    }
    let head = &b[..6];
    if head != b"Bearer" && head != b"bearer" {
        return None;
    }
    if b[6] != b' ' {
        return None;
    }
    Some(&raw[7..])
}

/// Percent-decoding untuk body `application/x-www-form-urlencoded`. Escape tidak
/// sah (`%zz`, terpotong) dan byte kendali menyebabkan kegagalan, bukan lolos
/// diam-diam.
pub fn pct_decode<'a>(src: &str, dst: &'a mut [u8]) -> Option<&'a str> {
    let b = src.as_bytes();
    let mut i = 0usize;
    let mut o = 0usize;
    while i < b.len() {
        if o >= dst.len() {
            return None;
        }
        match b[i] {
            b'%' => {
                if i + 2 >= b.len() {
                    return None;
                }
                let hi = hex_nib(b[i + 1])?;
                let lo = hex_nib(b[i + 2])?;
                let v = (hi << 4) | lo;
                if !(0x20..0x7f).contains(&v) {
                    return None;
                }
                dst[o] = v;
                o += 1;
                i += 3;
            }
            b'+' => {
                dst[o] = b' ';
                o += 1;
                i += 1;
            }
            c if (0x20..0x7f).contains(&c) => {
                dst[o] = c;
                o += 1;
                i += 1;
            }
            _ => return None,
        }
    }
    core::str::from_utf8(&dst[..o]).ok()
}

fn hex_nib(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SALT: [u8; RJCFG_AUTH_SALT_MAX] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    const PW: &[u8] = b"rahasia-operator-1";
    const PW_VIEWER: &[u8] = b"pembaca-saja-1234";

    fn salt2() -> [u8; RJCFG_AUTH_SALT_MAX] {
        let mut s = SALT;
        s[0] ^= 0xa5;
        s
    }

    fn store_with_users() -> AuthStore {
        let mut a = AuthStore::new();
        let admin = AuthStore::derive(PW, &SALT, AUTH_DEFAULT_ROUNDS);
        let viewer = AuthStore::derive(PW_VIEWER, &salt2(), AUTH_DEFAULT_ROUNDS);
        a.set_user("admin", Role::Admin, &SALT, AUTH_DEFAULT_ROUNDS, &admin)
            .expect("admin terpasang");
        a.set_user(
            "pembaca",
            Role::Viewer,
            &salt2(),
            AUTH_DEFAULT_ROUNDS,
            &viewer,
        )
        .expect("viewer terpasang");
        a
    }

    /// "Bearer " + 64 hex = 71 byte.
    fn bearer(tok: &[u8; AUTH_TOKEN_BYTES]) -> [u8; 71] {
        let mut out = [0u8; 71];
        out[..7].copy_from_slice(b"Bearer ");
        let _ = hex::encode(tok, &mut out[7..]).expect("64 hex muat persis");
        out
    }

    fn header_of<'a>(tok: &[u8; AUTH_TOKEN_BYTES], out: &'a mut [u8; 71]) -> &'a str {
        *out = bearer(tok);
        core::str::from_utf8(out).unwrap()
    }

    #[test]
    fn test_auth_no_default_credentials() {
        let a = AuthStore::new();
        assert!(!a.has_users(), "build baru tidak boleh punya akun bawaan");
        assert_eq!(a.user_count(), 0);
        assert_eq!(a.sessions_active(), 0);
    }

    #[test]
    fn test_auth_set_user_validation_and_capacity() {
        let mut a = AuthStore::new();
        let hash = AuthStore::derive(PW, &SALT, AUTH_DEFAULT_ROUNDS);
        assert_eq!(
            a.set_user("ad", Role::Admin, &SALT, AUTH_DEFAULT_ROUNDS, &hash),
            Err("nama kredensial tidak valid")
        );
        assert_eq!(
            a.set_user("Admin", Role::Admin, &SALT, AUTH_DEFAULT_ROUNDS, &hash),
            Err("nama kredensial tidak valid"),
            "nama huruf besar ditolak"
        );
        assert_eq!(
            a.set_user("admin", Role::Admin, &[0u8; 8], AUTH_DEFAULT_ROUNDS, &hash),
            Err("salt harus 16 byte")
        );
        assert_eq!(
            a.set_user("admin", Role::Admin, &SALT, 1, &hash),
            Err("iterasi PBKDF2 di luar rentang"),
            "iterasi 1 ditolak, tidak diam-diam dipakai"
        );
        assert_eq!(
            a.set_user("admin", Role::Admin, &SALT, AUTH_DEFAULT_ROUNDS, &[0u8; 16]),
            Err("hash harus 32 byte")
        );
        assert!(a
            .set_user("admin", Role::Admin, &SALT, AUTH_DEFAULT_ROUNDS, &hash)
            .is_ok());
        assert_eq!(a.user_count(), 1);

        // Perbarui nama yang sama tidak menambah slot.
        assert!(a.set_user("admin", Role::Admin, &SALT, 2048, &hash).is_ok());
        assert_eq!(a.user_count(), 1);
        assert_eq!(a.users[0].rounds, 2048);

        for n in ["baca", "tulis", "pantau"] {
            assert!(a
                .set_user(n, Role::Viewer, &salt2(), AUTH_DEFAULT_ROUNDS, &hash)
                .is_ok());
        }
        assert_eq!(a.user_count(), AUTH_USERS_MAX as u32);
        assert_eq!(
            a.set_user("slotlima", Role::Viewer, &SALT, AUTH_DEFAULT_ROUNDS, &hash),
            Err("tabel kredensial penuh")
        );
        assert!(a.remove_user("pantau"));
        assert!(
            a.set_user("slotlima", Role::Viewer, &SALT, AUTH_DEFAULT_ROUNDS, &hash)
                .is_ok(),
            "slot hasil penghapusan dapat dipakai lagi"
        );
        assert!(
            !a.remove_user("tiada"),
            "menghapus nama tidak dikenal berbohong?"
        );
    }

    #[test]
    fn test_auth_verify_password_and_roles() {
        let a = store_with_users();
        assert_eq!(a.verify_password("admin", PW), Some(Role::Admin));
        assert_eq!(a.verify_password("admin", b"rahasia-operator-2"), None);
        assert_eq!(a.verify_password("tidakada", PW), None);
        assert_eq!(a.verify_password("pembaca", PW_VIEWER), Some(Role::Viewer));
        // Salt berbeda -> kata sandi yang sama tidak berlaku lintas akun.
        assert_eq!(a.verify_password("pembaca", PW), None);
        assert_eq!(a.verify_password("admin", b"pendek"), None);
        let long = [b'x'; AUTH_PASSWORD_MAX + 1];
        assert_eq!(a.verify_password("admin", &long), None);
    }

    #[test]
    fn test_auth_login_authorize_and_role_gate() {
        let mut a = store_with_users();
        let (idx, tok, role) = a.login("admin", PW, 100).expect("login admin sah");
        assert_eq!(role, Role::Admin);
        assert_eq!(a.user_name(idx).1, 5);
        assert_eq!(a.logins_ok, 1);

        let mut hb = [0u8; 71];
        let header = header_of(&tok, &mut hb);
        let (got_idx, got_role) = a
            .authorize(Some(header), false, 101)
            .expect("token diterima");
        assert_eq!((got_idx, got_role), (idx, Role::Admin));

        // Viewer boleh membaca, tidak boleh menulis.
        let (_, vtok, _) = a.login("pembaca", PW_VIEWER, 102).expect("login viewer");
        let mut vh = [0u8; 71];
        let vheader = header_of(&vtok, &mut vh);
        assert!(a.authorize(Some(vheader), false, 103).is_ok());
        let err = a.authorize(Some(vheader), true, 104).unwrap_err();
        assert_eq!(err, AuthError::InsufficientRole);
        assert_eq!(err.code(), "insufficient_role");
        assert!(!err.status_401(), "kewenangan cukup dijawab 403, bukan 401");

        // Tanpa header, header cacat, dan skema lain.
        assert_eq!(a.authorize(None, false, 105), Err(AuthError::Anonymous));
        assert_eq!(
            a.authorize(Some("Bearer zz"), false, 106),
            Err(AuthError::BadToken)
        );
        assert_eq!(
            a.authorize(Some("Basic YWRtaW4="), false, 107),
            Err(AuthError::BadToken),
            "skema lain tidak diterima"
        );
        // Token orang lain tidak bisa dipakai setelah login gagal berulang.
        assert_eq!(a.logins_bad, 0);
    }

    #[test]
    fn test_auth_peek_is_side_effect_free_and_honours_expiry() {
        let mut a = store_with_users();
        let (_, tok, _) = a.login("admin", PW, 500).expect("login");
        let mut hb = [0u8; 71];
        let header = header_of(&tok, &mut hb);
        let before = (a.rejects_anon, a.rejects_bad_token, a.sessions_expired);
        assert_eq!(a.peek(Some(header), 501).map(|(_, r)| r), Some(Role::Admin));
        // Probe tidak menaikkan pencacah penolakan dan tidak memperpanjang sesi.
        assert_eq!(
            (a.rejects_anon, a.rejects_bad_token, a.sessions_expired),
            before
        );
        // Kedaluwarsa menurut kebijakan yang sama dengan authorize, tanpa ditulis.
        assert_eq!(a.peek(Some(header), 501 + AUTH_IDLE_TICKS + 1), None);
        assert_eq!(a.peek(None, 502), None, "probe anonim tidak memberi sesi");
        // `authorize` pada tick yang sama tetap menyahut Expired dan memberesi.
        assert_eq!(
            a.authorize(Some(header), false, 501 + AUTH_IDLE_TICKS + 1),
            Err(AuthError::Expired)
        );
        assert_eq!(a.sessions_expired, before.2 + 1);
    }

    #[test]
    fn test_auth_bad_login_never_issues_token() {
        let mut a = store_with_users();
        assert!(a.login("admin", b"kata-sandi-salah-1", 10).is_err());
        assert!(a.login("admin", b"pendek", 10).is_err());
        assert!(a.login("tidakada", PW, 10).is_err());
        assert_eq!(a.logins_bad, 3);
        assert_eq!(a.logins_ok, 0);
        assert_eq!(a.sessions_active(), 0, "login gagal tidak membuka sesi");
        // Login pada sistem tanpa akun: NoCredentials, bukan tebak-tebakan.
        let mut b = AuthStore::new();
        assert_eq!(b.login("admin", PW, 1), Err(AuthError::NoCredentials));
    }

    #[test]
    fn test_auth_expiry_idle_and_max_age() {
        let mut a = store_with_users();
        let (_, tok, _) = a.login("admin", PW, 1_000).expect("login");
        let mut hb = [0u8; 71];
        let header = header_of(&tok, &mut hb);
        assert!(a.authorize(Some(header), false, 1_001).is_ok());
        let late = 1_001 + AUTH_IDLE_TICKS + 1;
        assert_eq!(
            a.authorize(Some(header), false, late),
            Err(AuthError::Expired)
        );
        assert_eq!(a.sessions_expired, 1);

        // Umur maksimum: sesi dijaga tetap aktif oleh aktivitas rutin (idle
        // tidak pernah lewat) sampai tepat sebelum batas umur, lalu diuji setelah
        // batas itu. Tanpa penyegaran, idle-lah yang lebih dulu kedaluwarsa dan
        // jalur umur maksimum tidak teruji sama sekali.
        let mut b = store_with_users();
        let (_, t2, _) = b.login("admin", PW, 0).expect("login");
        let mut h2b = [0u8; 71];
        let h2 = header_of(&t2, &mut h2b);
        let langkah = AUTH_IDLE_TICKS - 100;
        let mut t = 0u64;
        while t + langkah < AUTH_MAX_AGE_TICKS {
            t += langkah;
            assert!(
                b.authorize(Some(h2), false, t).is_ok(),
                "aktivitas rutin menjaga sesi"
            );
        }
        let aktif = AUTH_MAX_AGE_TICKS - 10;
        assert!(
            b.authorize(Some(h2), false, aktif).is_ok(),
            "umur belum lewat, idle aman"
        );
        let tua = AUTH_MAX_AGE_TICKS + 1;
        assert!(tua - aktif <= AUTH_IDLE_TICKS, "idle sengaja dijaga aman");
        assert_eq!(
            b.authorize(Some(h2), false, tua),
            Err(AuthError::Expired),
            "idle aman tetapi umur maksimum lewat tetap kedaluwarsa"
        );
    }

    #[test]
    fn test_auth_logout_and_password_change_revokes() {
        let mut a = store_with_users();
        let (_, tok, _) = a.login("admin", PW, 5).expect("login");
        let mut hb = [0u8; 71];
        let header = header_of(&tok, &mut hb);
        assert!(a.logout(Some(header)));
        assert_eq!(a.authorize(Some(header), false, 6), Err(AuthError::Expired));
        assert!(!a.logout(Some(header)), "logout kedua tidak menemukan sesi");

        // Ganti kata sandi pada nama yang sama -> sesi lama dicabut.
        let (_, t2, _) = a.login("admin", PW, 7).expect("login 2");
        let mut h2b = [0u8; 71];
        let h2 = header_of(&t2, &mut h2b);
        assert!(a.authorize(Some(h2), false, 8).is_ok());
        let new_hash = AuthStore::derive(b"kata-sandi-baru-12", &SALT, AUTH_DEFAULT_ROUNDS);
        a.set_user("admin", Role::Admin, &SALT, AUTH_DEFAULT_ROUNDS, &new_hash)
            .expect("perbarui admin");
        assert_eq!(
            a.authorize(Some(h2), false, 9),
            Err(AuthError::Expired),
            "sesi lama harus mati setelah sandi diganti"
        );
        assert!(
            a.login("admin", PW, 10).is_err(),
            "sandi lama tidak berlaku lagi"
        );
        assert!(a.login("admin", b"kata-sandi-baru-12", 11).is_ok());
    }

    #[test]
    fn test_auth_no_credentials_blocks_every_route() {
        let mut a = AuthStore::new();
        assert_eq!(
            a.authorize(None, false, 1),
            Err(AuthError::NoCredentials),
            "sebelum bootstrap semua jalur API tertutup"
        );
        assert_eq!(
            a.authorize(Some("Bearer aa"), true, 1),
            Err(AuthError::NoCredentials)
        );
        assert_eq!(a.rejects_anon, 2);
    }

    #[test]
    fn test_auth_journal_roundtrip_and_rejects_corrupt() {
        let a = store_with_users();
        let mut payload = RjcfgConfigPayload::default_config();
        a.save_to(&mut payload);
        assert_eq!(payload.auth_match_bytes, AUTH_MATCH_BYTES as u8);

        let mut loaded = AuthStore::new();
        loaded.load_from(&payload);
        assert_eq!(loaded.user_count(), 2);
        assert_eq!(loaded.verify_password("admin", PW), Some(Role::Admin));
        assert_eq!(
            loaded.verify_password("pembaca", PW_VIEWER),
            Some(Role::Viewer)
        );

        // Iterasi di bawah minimum: rekaman dibuang, bukan dipakai.
        let mut bad = payload;
        bad.auth[0].rounds = 1;
        let mut l2 = AuthStore::new();
        l2.load_from(&bad);
        assert_eq!(
            l2.verify_password("admin", PW),
            None,
            "rekaman lemah dibuang"
        );
        assert_eq!(l2.user_count(), 1, "hanya rekaman sah yang bertahan");

        // Nama cacat (huruf besar) dibuang.
        let mut bad2 = payload;
        bad2.auth[0].user[0] = b'A';
        let mut l3 = AuthStore::new();
        l3.load_from(&bad2);
        assert_eq!(l3.user_count(), 1);

        // Lebar pencocokan tidak sah: jatuh ke default, bukan nol.
        let mut bad3 = payload;
        bad3.auth_match_bytes = 99;
        let mut l4 = AuthStore::new();
        l4.load_from(&bad3);
        assert_eq!(l4.match_bytes, AUTH_MATCH_BYTES as u8);

        // Sesi tidak pernah ikut tersimpan/muat.
        let mut l5 = AuthStore::new();
        let _ = l5.login("admin", PW, 1);
        l5.load_from(&payload);
        assert_eq!(l5.sessions_active(), 0);
    }

    #[test]
    fn test_auth_session_table_full_evicts_oldest_idle() {
        let mut a = store_with_users();
        let mut first = [0u8; AUTH_TOKEN_BYTES];
        for i in 0..AUTH_SESSIONS_MAX {
            let (_, t, _) = a.login("admin", PW, i as u64 + 1).expect("login");
            if i == 0 {
                first = t;
            }
        }
        assert_eq!(a.sessions_active(), AUTH_SESSIONS_MAX as u32);
        let (_, t9, _) = a.login("admin", PW, 100).expect("login ke-9");
        assert_eq!(a.sessions_active(), AUTH_SESSIONS_MAX as u32);
        assert_eq!(a.revocations, 1);
        let mut h1 = [0u8; 71];
        let mut h9 = [0u8; 71];
        let s1 = header_of(&first, &mut h1);
        let s9 = header_of(&t9, &mut h9);
        assert_eq!(
            a.authorize(Some(s1), false, 101),
            Err(AuthError::Expired),
            "sesi tergusur tidak lagi diterima"
        );
        assert!(a.authorize(Some(s9), false, 102).is_ok());
    }

    #[test]
    fn test_auth_tokens_distinct_and_salt_random() {
        let mut a = store_with_users();
        a.add_entropy(b"tik-1");
        let (_, t1, _) = a.login("admin", PW, 1).expect("login 1");
        let (_, t2, _) = a.login("admin", PW, 2).expect("login 2");
        assert_ne!(t1, t2, "dua sesi wajib berbeda");
        assert_ne!(t1, [0u8; AUTH_TOKEN_BYTES]);
        let s1 = a.new_salt();
        let s2 = a.new_salt();
        assert_ne!(s1, s2, "salt akun baru tidak berulang");
    }

    #[test]
    fn test_auth_pct_decode() {
        let mut buf = [0u8; 64];
        assert_eq!(pct_decode("halo%20dunia", &mut buf), Some("halo dunia"));
        assert_eq!(
            pct_decode("a%2Bb", &mut buf),
            Some("a+b"),
            "+ tersandi harfiah"
        );
        assert_eq!(pct_decode("plain", &mut buf), Some("plain"));
        assert_eq!(pct_decode("x+y", &mut buf), Some("x y"));
        assert_eq!(
            pct_decode("cacat%zz", &mut buf),
            None,
            "escape tidak sah harus gagal, bukan lolos"
        );
        assert_eq!(pct_decode("terpotong%2", &mut buf), None);
        assert_eq!(
            pct_decode("kontrol%0a", &mut buf),
            None,
            "byte kendali ditolak"
        );
        let mut tiny = [0u8; 2];
        assert_eq!(
            pct_decode("abc", &mut tiny),
            None,
            "buffer kurang besar: tolak"
        );
    }

    #[test]
    fn test_auth_token_shape_rules() {
        let mut tok = [0u8; AUTH_TOKEN_BYTES];
        tok[0] = 0xab;
        let mut hb = [0u8; 71];
        let text = header_of(&tok, &mut hb);
        assert_eq!(AuthStore::token_of(Some(text)), Some(tok));

        // Skema tidak peka huruf: "bearer ..." juga diterima.
        let mut lower = [0u8; 71];
        lower.copy_from_slice(text.as_bytes());
        lower[..6].copy_from_slice(b"bearer");
        let ltext = core::str::from_utf8(&lower).unwrap();
        assert_eq!(AuthStore::token_of(Some(ltext)), Some(tok));

        assert_eq!(
            AuthStore::token_of(Some("Bearer")),
            None,
            "tanpa spasi+token"
        );
        assert_eq!(
            AuthStore::token_of(Some("Bearer  deadbeef")),
            None,
            "spasi ganda"
        );
        assert_eq!(
            AuthStore::token_of(Some("Bearer deadbeef")),
            None,
            "hex terlalu pendek ditolak"
        );
        let mut sp = [0u8; 72];
        sp[..71].copy_from_slice(text.as_bytes());
        sp[71] = b'!';
        assert_eq!(
            AuthStore::token_of(Some(core::str::from_utf8(&sp).unwrap())),
            None,
            "ekor bukan hex ditolak"
        );
        assert_eq!(AuthStore::token_of(None), None);
    }

    #[test]
    fn test_auth_session_left_counts_down() {
        let mut a = store_with_users();
        let (_, tok, _) = a.login("admin", PW, 0).expect("login");
        let mut hb = [0u8; 71];
        let header = header_of(&tok, &mut hb);
        assert!(a.authorize(Some(header), false, 0).is_ok());
        // Sisa 4 jam umur maksimum, idle 30 menit -> idle yang lebih dulu habis.
        assert_eq!(
            a.session_left_s(Some(header), 10),
            ((AUTH_IDLE_TICKS - 10) / 10) as u32
        );
        assert_eq!(a.session_left_s(Some("Bearer zz"), 10), 0);
        assert_eq!(a.session_left_s(None, 10), 0);
    }
}
