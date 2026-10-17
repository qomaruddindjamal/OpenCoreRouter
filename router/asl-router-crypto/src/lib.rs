//! Open Core Router (OpenCR) — asl-router-crypto
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Primitif kripto native untuk lapisan autentikasi router.
//!
//! Isi: SHA-256 (FIPS 180-4), HMAC-SHA256 (RFC 2104), PBKDF2-HMAC-SHA256
//! (RFC 8018), pembanding waktu-konstan, pemerahan byte acak berbasis SHA-256,
//! dan enkripsi hex. Semua ditulis native untuk `#![no_std]` karena repositori
//! ini tidak punya dependensi crates.io sama sekali (32 paket di `Cargo.lock`
//! seluruhnya crate workspace), jadi tidak ada implementasi tepercaya pihak
//! ketiga yang bisa dipakai.
//!
//! Batas yang dinyatakan jujur:
//! - `expand` bukan CSPRNG bersertifikat (NDRNG/SP 800-90). Kualitas keacakan
//!   keluarannya sama dengan entropy yang dicampur pemanggil ke dalam `seed`.
//! - PBKDF2 di sini adalah penambah biaya, bukan pengganti Argon2/bcrypt yang
//!   tahan GPU. Jumlah iterasi dipilih oleh pemanggil dan wajib dicatat.

#![no_std]

// -----------------------------------------------------------------------------
// SHA-256 (FIPS 180-4) — implementasi native no_std
// -----------------------------------------------------------------------------

pub mod sha256 {
    /// K = 32 bit pertama dari bagian pecahan akar pangkat tiga 64 bilangan
    /// prima pertama (2..311), dihitung presisi-eksak dan dicocokkan terhadap
    /// vektor uji FIPS 180-4 (lihat `test_sha256_vectors`).
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    #[derive(Clone, Copy)]
    pub struct Sha256 {
        state: [u32; 8],
        buf: [u8; 64],
        buflen: usize,
        total: u64,
    }

    impl Sha256 {
        pub const fn new() -> Self {
            Self {
                state: [
                    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
                    0x1f83d9ab, 0x5be0cd19,
                ],
                buf: [0u8; 64],
                buflen: 0,
                total: 0,
            }
        }

        pub fn update(&mut self, mut data: &[u8]) {
            self.total = self
                .total
                .checked_add(data.len() as u64)
                .expect("SHA-256: panjang overflow");
            if self.buflen > 0 {
                let take = core::cmp::min(64 - self.buflen, data.len());
                self.buf[self.buflen..self.buflen + take].copy_from_slice(&data[..take]);
                self.buflen += take;
                data = &data[take..];
                if self.buflen == 64 {
                    let block = self.buf;
                    self.compress(&block);
                    self.buflen = 0;
                }
            }
            while data.len() >= 64 {
                let mut block = [0u8; 64];
                block.copy_from_slice(&data[..64]);
                self.compress(&block);
                data = &data[64..];
            }
            if !data.is_empty() {
                self.buf[..data.len()].copy_from_slice(data);
                self.buflen = data.len();
            }
        }

        fn compress(&mut self, block: &[u8; 64]) {
            let mut w = [0u32; 64];
            for i in 0..16 {
                w[i] = u32::from_be_bytes([
                    block[i * 4],
                    block[i * 4 + 1],
                    block[i * 4 + 2],
                    block[i * 4 + 3],
                ]);
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[i - 7])
                    .wrapping_add(s1);
            }
            let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
            for i in 0..64 {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let ch = (e & f) ^ ((!e) & g);
                let t1 = h
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[i])
                    .wrapping_add(w[i]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let t2 = s0.wrapping_add(maj);
                h = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }
            let adds = [a, b, c, d, e, f, g, h];
            for (s, x) in self.state.iter_mut().zip(adds.iter()) {
                *s = s.wrapping_add(*x);
            }
        }

        /// Digest dari seluruh data yang sudah dikonsumsi, TANPA mengubah
        /// akumulator (dipakai untuk membandingkan digest jalur tulis vs
        /// verifikasi saat akumulasi masih berjalan).
        pub fn peek(&self) -> [u8; 32] {
            let mut s = *self;
            s.pad_and_finish()
        }

        pub fn finalize(mut self) -> [u8; 32] {
            self.pad_and_finish()
        }

        fn pad_and_finish(&mut self) -> [u8; 32] {
            let bits = self.total.wrapping_mul(8);
            let padlen = if self.buflen < 56 {
                56 - self.buflen
            } else {
                120 - self.buflen
            };
            self.push_pad(padlen, bits);
            let mut out = [0u8; 32];
            for (i, st) in self.state.iter().enumerate() {
                out[i * 4..i * 4 + 4].copy_from_slice(&st.to_be_bytes());
            }
            out
        }

        /// Tambah byte padding 0x80..00 lalu panjang 64-bit BE, tanpa
        /// menghitungnya ke dalam `total`.
        fn push_pad(&mut self, padlen: usize, bits: u64) {
            let mut pad = [0u8; 128];
            pad[0] = 0x80;
            let saved = self.total;
            self.feed(&pad[..padlen]);
            let mut len_be = [0u8; 8];
            len_be.copy_from_slice(&bits.to_be_bytes());
            self.feed(&len_be);
            self.total = saved;
        }

        fn feed(&mut self, mut data: &[u8]) {
            if self.buflen > 0 {
                let take = core::cmp::min(64 - self.buflen, data.len());
                self.buf[self.buflen..self.buflen + take].copy_from_slice(&data[..take]);
                self.buflen += take;
                data = &data[take..];
                if self.buflen == 64 {
                    let block = self.buf;
                    self.compress(&block);
                    self.buflen = 0;
                }
            }
            while data.len() >= 64 {
                let mut block = [0u8; 64];
                block.copy_from_slice(&data[..64]);
                self.compress(&block);
                data = &data[64..];
            }
            if !data.is_empty() {
                self.buf[..data.len()].copy_from_slice(data);
                self.buflen = data.len();
            }
        }
    }

    pub fn digest_oneshot(data: &[u8]) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(data);
        h.finalize()
    }
}

// -----------------------------------------------------------------------------
// HMAC-SHA256 (RFC 2104)
// -----------------------------------------------------------------------------

pub mod hmac {
    use super::sha256::Sha256;

    const BLOCK: usize = 64;

    /// Kunci sudah dipanggang ke dalam blok ipad/opad agar PBKDF2 tidak
    /// memproses ulang kunci pada setiap iterasi.
    #[derive(Clone, Copy)]
    pub struct HmacSha256 {
        ipad: [u8; BLOCK],
        opad: [u8; BLOCK],
    }

    impl HmacSha256 {
        pub fn new(key: &[u8]) -> Self {
            let mut kb = [0u8; BLOCK];
            if key.len() > BLOCK {
                // Kunci lebih panjang dari blok ukuran: diganti digest-nya (RFC 2104).
                let d = super::sha256::digest_oneshot(key);
                kb[..d.len()].copy_from_slice(&d);
            } else {
                kb[..key.len()].copy_from_slice(key);
            }
            let mut ipad = [0u8; BLOCK];
            let mut opad = [0u8; BLOCK];
            for i in 0..BLOCK {
                ipad[i] = kb[i] ^ 0x36;
                opad[i] = kb[i] ^ 0x5c;
            }
            Self { ipad, opad }
        }

        pub fn compute(&self, msg: &[u8]) -> [u8; 32] {
            let mut inner = Sha256::new();
            inner.update(&self.ipad);
            inner.update(msg);
            let inner_digest = inner.finalize();
            let mut outer = Sha256::new();
            outer.update(&self.opad);
            outer.update(&inner_digest);
            outer.finalize()
        }

        /// Salin `n` byte pertama hasil ke `out`; dipakai vektor uji RFC 4231
        /// dengan keluaran dipotong.
        pub fn compute_into(&self, msg: &[u8], out: &mut [u8]) {
            let d = self.compute(msg);
            let n = core::cmp::min(out.len(), d.len());
            out[..n].copy_from_slice(&d[..n]);
        }
    }

    pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
        HmacSha256::new(key).compute(msg)
    }
}

// -----------------------------------------------------------------------------
// PBKDF2-HMAC-SHA256 (RFC 8018, 5.2) — penambah biaya untuk kata sandi
// -----------------------------------------------------------------------------

pub mod pbkdf2 {
    use super::hmac::HmacSha256;

    /// `dk = PBKDF2(P, S, c, dkLen)`. `out` menentukan `dkLen`.
    ///
    /// Tidak ada alokasi: satu blok `u` di tumpukan per blok keluaran. Kegagalan
    /// paling berbahaya di sini adalah iterasi Nol, jadi `rounds` minimum 1.
    pub fn pbkdf2_sha256(pw: &[u8], salt: &[u8], rounds: u32, out: &mut [u8]) {
        let prf = HmacSha256::new(pw);
        let rounds = if rounds == 0 { 1 } else { rounds };
        let blocks = (out.len() + 31) / 32;
        for blk in 0..blocks {
            let mut input = [0u8; 128];
            let take = core::cmp::min(salt.len(), input.len() - 4);
            input[..take].copy_from_slice(&salt[..take]);
            input[take..take + 4].copy_from_slice(&((blk as u32 + 1).to_be_bytes()));
            let mut u = prf.compute(&input[..take + 4]);
            let mut t = u;
            for _ in 1..rounds {
                u = prf.compute(&u);
                for (dst, src) in t.iter_mut().zip(u.iter()) {
                    *dst ^= *src;
                }
            }
            let off = blk * 32;
            let n = core::cmp::min(32, out.len() - off);
            out[off..off + n].copy_from_slice(&t[..n]);
        }
    }
}

// -----------------------------------------------------------------------------
// Pembanding waktu-konstan
// -----------------------------------------------------------------------------

pub mod ct {
    /// Bandingkan dua lintah tanpa cabang pada isi byte. Panjang yang berbeda
    /// langsung `false`; kebocoran panjang bukan masalah pada token hex.
    #[must_use]
    pub fn eq(a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let mut diff = 0u8;
        for i in 0..a.len() {
            diff |= a[i] ^ b[i];
        }
        diff == 0
    }
}

// -----------------------------------------------------------------------------
// Pemerahan byte (SHA-256 counter mode) + hex
// -----------------------------------------------------------------------------

pub mod expand {
    use super::sha256::Sha256;

    /// Pemerahan deterministik dari `seed`: `block_i = SHA-256(seed || i)`.
    ///
    /// Jujur: fungsi ini hanya merentangkan entropy yang sudah ada di `seed`.
    /// Pemanggil (kernel) wajib mencampur sumber acak nyata ke dalam seed dan
    /// mencatat sumbernya; tanpa itu keluarannya dapat ditebak.
    pub fn bytes(seed: &[u8; 32], mut counter: u64, out: &mut [u8]) {
        let mut off = 0usize;
        while off < out.len() {
            let mut h = Sha256::new();
            h.update(seed);
            h.update(&counter.to_be_bytes());
            let d = h.finalize();
            let n = core::cmp::min(32, out.len() - off);
            out[off..off + n].copy_from_slice(&d[..n]);
            off += n;
            counter = counter.wrapping_add(1);
        }
    }
}

pub mod hex {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";

    /// Tulis `src` sebagai hex huruf kecil ke `dst`. `None` bila `dst` kurang
    /// panjang (2 byte per oktet) — tidak ada pemotongan diam-diam.
    #[must_use]
    pub fn encode<'a>(src: &[u8], dst: &'a mut [u8]) -> Option<&'a str> {
        if dst.len() < src.len() * 2 {
            return None;
        }
        for (i, b) in src.iter().enumerate() {
            dst[i * 2] = DIGITS[(b >> 4) as usize];
            dst[i * 2 + 1] = DIGITS[(b & 0x0f) as usize];
        }
        core::str::from_utf8(&dst[..src.len() * 2]).ok()
    }

    /// Baca hex (panjang genap, hanya 0-9a-fA-F). `None` untuk input cacat.
    #[must_use]
    pub fn decode(src: &str, dst: &mut [u8]) -> Option<usize> {
        let b = src.as_bytes();
        if b.len() % 2 != 0 || dst.len() < b.len() / 2 {
            return None;
        }
        let mut i = 0usize;
        while i < b.len() {
            let hi = nib(b[i])?;
            let lo = nib(b[i + 1])?;
            dst[i / 2] = (hi << 4) | lo;
            i += 2;
        }
        Some(i / 2)
    }

    fn nib(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        // Vektor uji selalu 64 hex; pemotongan dianggap kegagalan tes.
        let n = hex::decode(s, &mut out).expect("vektor uji hex cacat");
        assert_eq!(n, 32);
        out
    }

    #[test]
    fn test_sha256_vectors() {
        // FIPS 180-4 / NIST short & long messages. Nilai pembanding dihitung
        // terpisah dengan hashlib CPython pada sesi kerja ini (bukan ingatan).
        assert_eq!(
            sha256::digest_oneshot(b""),
            h("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        );
        assert_eq!(
            sha256::digest_oneshot(b"abc"),
            h("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(
            sha256::digest_oneshot(
                b"abcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcd"
            ),
            h("625b41490b883891943c5fa54ad45d7c900b9b6e91e159334e320b1f5215a209")
        );
        // 55/56/64 byte: tiga kasus batas padding yang paling sering salah.
        assert_eq!(
            sha256::digest_oneshot(&[b'a'; 55]),
            h("9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318")
        );
        assert_eq!(
            sha256::digest_oneshot(&[b'a'; 56]),
            h("b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a")
        );
        assert_eq!(
            sha256::digest_oneshot(&[b'a'; 64]),
            h("ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb")
        );
    }

    #[test]
    fn test_sha256_streaming_matches_oneshot() {
        // Pengiriman mencicil harus identik dengan satu lahap, termasuk saat
        // pemotongan jatuh di tengah blok yang sedang terisi.
        let data = [b'z'; 300];
        let mut acc = sha256::Sha256::new();
        let mut rest = &data[..];
        while !rest.is_empty() {
            let n = core::cmp::min(37, rest.len());
            acc.update(&rest[..n]);
            rest = &rest[n..];
        }
        assert_eq!(acc.peek(), sha256::digest_oneshot(&data));
        assert_eq!(acc.finalize(), sha256::digest_oneshot(&data));

        // Blok penuh yang disuplai sekaligus juga harus sama.
        let mut whole = sha256::Sha256::new();
        whole.update(&data);
        assert_eq!(whole.finalize(), sha256::digest_oneshot(&data));
    }

    #[test]
    fn test_hmac_rfc4231_vectors() {
        // RFC 4231 Appendix 2, case 1..7 untuk HMAC-SHA-256.
        assert_eq!(
            hmac::hmac_sha256(&[0x0b; 20], b"Hi There"),
            h("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7")
        );
        assert_eq!(
            hmac::hmac_sha256(b"Jefe", b"what do ya want for nothing?"),
            h("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843")
        );
        assert_eq!(
            hmac::hmac_sha256(&[0xaa; 20], &[0xdd; 50]),
            h("773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe")
        );
        let key4: [u8; 25] = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25,
        ];
        assert_eq!(
            hmac::hmac_sha256(&key4, &[0xcd; 50]),
            h("82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b")
        );
        // Case 5: keluaran dipotong 16 byte.
        let mut trunc = [0u8; 16];
        hmac::HmacSha256::new(&[0x0c; 20]).compute_into(b"Test With Truncation", &mut trunc);
        assert_eq!(
            trunc,
            hex16("a3b6167473100ee06e0c796c2955552b"),
            "keluaran HMAC yang dipotong harus sama dengan 16 byte pertama"
        );
        // Case 6 & 7: kunci 131 byte (lebih panjang dari blok -> di-hash dulu).
        assert_eq!(
            hmac::hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            ),
            h("60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54")
        );
        assert_eq!(
            hmac::hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key and Larger Than One Block-Size Data"
            ),
            h("c9731f25665706dab8200d9ce68fad2cbac48efc4a5f72292e4eeb81e7d29298")
        );
    }

    fn hex16(s: &str) -> [u8; 16] {
        let mut out = [0u8; 16];
        assert_eq!(hex::decode(s, &mut out), Some(16));
        out
    }

    #[test]
    fn test_pbkdf2_vectors() {
        let mut dk = [0u8; 32];
        pbkdf2::pbkdf2_sha256(b"password", b"salt", 1, &mut dk);
        assert_eq!(
            dk,
            h("120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b")
        );
        pbkdf2::pbkdf2_sha256(b"password", b"salt", 2, &mut dk);
        assert_eq!(
            dk,
            h("ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43")
        );
        pbkdf2::pbkdf2_sha256(b"password", b"salt", 3, &mut dk);
        assert_eq!(
            dk,
            h("ad35240ac683febfaf3cd49d845473fbbbaa2437f5f82d5a415ae00ac76c6bfc")
        );
        // Multi-blok keluaran (40 byte -> 2 blok, blok kedua dipotong).
        let mut dk40 = [0u8; 40];
        pbkdf2::pbkdf2_sha256(
            b"passwordPASSWORDpassword",
            b"saltSALTsaltSALTsaltSALTsaltSALTsalt",
            4096,
            &mut dk40,
        );
        let mut full = [0u8; 80];
        let got = hex::encode(&dk40, &mut full).expect("40 byte -> 80 hex");
        assert_eq!(
            got,
            "348c89dbcbd32b2f32d814b8116e84cf2b17347ebc1800181c4e2a1fb8dd53e1c635518c7dac47e9"
        );
    }

    #[test]
    fn test_pbkdf2_zero_rounds_is_clamped() {
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        pbkdf2::pbkdf2_sha256(b"x", b"y", 0, &mut a);
        pbkdf2::pbkdf2_sha256(b"x", b"y", 1, &mut b);
        assert_eq!(a, b, "iterasi 0 wajib diklamp ke 1, bukan menghasilkan nol");
    }

    #[test]
    fn test_ct_eq_and_length() {
        assert!(ct::eq(b"abcdef", b"abcdef"));
        assert!(!ct::eq(b"abcdef", b"abcdeg"));
        assert!(!ct::eq(b"abc", b"abcd"), "panjang berbeda harus ditolak");
        assert!(ct::eq(b"", b""));
    }

    #[test]
    fn test_expand_is_deterministic_and_counter_dependent() {
        let seed = [7u8; 32];
        let mut a = [0u8; 64];
        let mut b = [0u8; 64];
        expand::bytes(&seed, 0, &mut a);
        expand::bytes(&seed, 0, &mut b);
        assert_eq!(a, b, "pemerahan harus dapat direproduksi");
        let mut c = [0u8; 64];
        expand::bytes(&seed, 1, &mut c);
        assert_ne!(a, c, "counter berbeda wajib memberi byte berbeda");
        let mut d = [0u8; 32];
        expand::bytes(&[8u8; 32], 0, &mut d);
        assert_ne!(a[..32], d, "seed berbeda wajib memberi byte berbeda");
        // 64 byte = dua blok: paruh kedua wajib sama dengan counter berikutnya.
        let mut second = [0u8; 32];
        expand::bytes(&seed, 1, &mut second);
        assert_eq!(&a[32..], &second[..]);
    }

    #[test]
    fn test_hex_roundtrip_and_guards() {
        let src = [0x00u8, 0x0f, 0xa5, 0xff];
        let mut buf = [0u8; 8];
        assert_eq!(hex::encode(&src, &mut buf), Some("000fa5ff"));
        let mut back = [0u8; 4];
        assert_eq!(hex::decode("000FA5FF", &mut back), Some(4));
        assert_eq!(back, src);
        let mut tiny = [0u8; 6];
        assert_eq!(
            hex::encode(&src, &mut tiny),
            None,
            "buffer kurang besar: tolak"
        );
        assert_eq!(hex::decode("abc", &mut back), None, "hex ganjil: tolak");
        assert_eq!(hex::decode("zz", &mut back), None, "nondigit: tolak");
    }
}
