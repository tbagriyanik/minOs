//! secp256r1 (NIST P-256) ECDHE — no_std saf Rust.
//!
//! Alan elemanları 8 × 32-bit uzuv (little-endian) olarak saklanır.
//! 512-bit çarpmanın mod p indirgeme P-256'nın özel biçiminden yararlanır:
//!
//! ```text
//! 2^256 ≡ 2^224 - 2^192 - 2^96 + 1  (mod p)
//! ```
//!
//! Yani yüksek 256 bit `M256` ile çarpılıp sonuca eklenir; her katlamada
//! değer ≈ 2^32 kat küçüldüğünden birkaç turda 256 bite iner. n gibi genel
//! modüller için ikili uzun bölme kullanılır. Nokta işlemleri Jacobian
//! koordinatlarında yapılır; sonsuz nokta `z == 0` ile temsil edilir.
//!
//! Sıcak yol (ECDHE el sıkışması) başına iki ölçek çarpımı ve iki ters
//! çevirme yapılır; toplam milyonlarca basit işlemle sınırlıdır.

use core::cmp::Ordering;

type U256 = [u32; 8];

const ZERO: U256 = [0; 8];
const ONE: U256 = [1, 0, 0, 0, 0, 0, 0, 0];

/// Büyükten küçüğe 32 baytlık ASCII hex dizisini uzuvlara çevirir.
const fn from_be(b: &[u8; 32]) -> U256 {
    let mut r = [0u32; 8];
    let mut i = 0;
    while i < 8 {
        let j = 28 - i * 4; // en az anlamlı uzuv son 4 bayttır
        r[i] = u32::from_be_bytes([b[j], b[j + 1], b[j + 2], b[j + 3]]);
        i += 1;
    }
    r
}

fn to_be(u: &U256) -> [u8; 32] {
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[28 - i * 4..32 - i * 4].copy_from_slice(&u[i].to_be_bytes());
    }
    out
}

// p  = 2^256 - 2^224 + 2^192 + 2^96 - 1
const P: U256 = from_be(&[
    0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
]);
// a = -3 (mod p)
const A_COEF: U256 = from_be(&[
    0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFC,
]);
const B_CURVE: U256 = from_be(&[
    0x5A, 0xC6, 0x35, 0xD8, 0xAA, 0x3A, 0x93, 0xE7, 0xB3, 0xEB, 0xBD, 0x55, 0x76, 0x98, 0x86, 0xBC,
    0x65, 0x1D, 0x06, 0xB0, 0xCC, 0x53, 0xB0, 0xF6, 0x3B, 0xCE, 0x3C, 0x3E, 0x27, 0xD2, 0x60, 0x4B,
]);
const GX: U256 = from_be(&[
    0x6B, 0x17, 0xD1, 0xF2, 0xE1, 0x2C, 0x42, 0x47, 0xF8, 0xBC, 0xE6, 0xE5, 0x63, 0xA4, 0x40, 0xF2,
    0x77, 0x03, 0x7D, 0x81, 0x2D, 0xEB, 0x33, 0xA0, 0xF4, 0xA1, 0x39, 0x45, 0xD8, 0x98, 0xC2, 0x96,
]);
const GY: U256 = from_be(&[
    0x4F, 0xE3, 0x42, 0xE2, 0xFE, 0x1A, 0x7F, 0x9B, 0x8E, 0xE7, 0xEB, 0x4A, 0x7C, 0x0F, 0x9E, 0x16,
    0x2B, 0xCE, 0x33, 0x57, 0x6B, 0x31, 0x5E, 0xCE, 0xCB, 0xB6, 0x40, 0x68, 0x37, 0xBF, 0x51, 0xF5,
]);
// n: taban noktanın sırası (grup mertebesi)
const N: U256 = from_be(&[
    0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xBC, 0xE6, 0xFA, 0xAD, 0xA7, 0x17, 0x9E, 0x84, 0xF3, 0xB9, 0xCA, 0xC2, 0xFC, 0x63, 0x25, 0x51,
]);
// 2^256 mod p = 2^224 - 2^192 - 2^96 + 1 (katlama çarpanı)
const M256: U256 = from_be(&[
    0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
]);

fn cmp_limbs(a: &[u32], b: &[u32]) -> Ordering {
    for i in (0..a.len()).rev() {
        if a[i] != b[i] {
            return if a[i] > b[i] { Ordering::Greater } else { Ordering::Less };
        }
    }
    Ordering::Equal
}

/// 16 uzuvluk çarpım (okul kitabı yöntemi).
fn mul_wide(a: &U256, b: &U256) -> [u32; 16] {
    let mut acc = [0u32; 16];
    for i in 0..8 {
        let ai = a[i] as u64;
        if ai == 0 {
            continue;
        }
        let mut carry = 0u64;
        for j in 0..8 {
            let t = acc[i + j] as u64 + ai * (b[j] as u64) + carry;
            acc[i + j] = t as u32;
            carry = t >> 32;
        }
        let mut k = i + 8;
        while carry != 0 && k < 16 {
            let t = acc[k] as u64 + carry;
            acc[k] = t as u32;
            carry = t >> 32;
            k += 1;
        }
        // 512 bit sınırı: ürün 16 uzuvdan taşamaz, son carry her zaman 0.
    }
    acc
}

fn sub8(a: &mut U256, b: &U256) {
    let mut borrow = 0i64;
    for i in 0..8 {
        let v = a[i] as i64 - b[i] as i64 - borrow;
        if v < 0 {
            a[i] = (v + (1i64 << 32)) as u32;
            borrow = 1;
        } else {
            a[i] = v as u32;
            borrow = 0;
        }
    }
}

/// 9 uzuvluk değerden 9 uzuvluk modülü çıkarır (borç dahil).
fn sub9(a: &mut [u32; 9], b: &[u32; 9]) {
    let mut borrow = 0i64;
    for i in 0..9 {
        let v = a[i] as i64 - b[i] as i64 - borrow;
        if v < 0 {
            a[i] = (v + (1i64 << 32)) as u32;
            borrow = 1;
        } else {
            a[i] = v as u32;
            borrow = 0;
        }
    }
}

/// P-256'ya özel hızlı indirgeme (katlama yöntemli).
fn reduce_p(w: &[u32; 16]) -> U256 {
    let mut t = *w;
    // V = hi·2^256 + lo → V' = lo + hi·M = V - hi·p, her turda değer en az
    // p kadar azalır ve ~2^32 kat küçüldüğünden birkaç turda biter.
    loop {
        if t[8] == 0 && t[9] == 0 && t[10] == 0 && t[11] == 0 && t[12] == 0 && t[13] == 0
            && t[14] == 0 && t[15] == 0
        {
            break;
        }
        let mut hi = [0u32; 8];
        hi.copy_from_slice(&t[8..16]);
        for v in t[8..16].iter_mut() {
            *v = 0;
        }
        let prod = mul_wide(&hi, &M256);
        let mut carry = 0u64;
        for i in 0..16 {
            let s = t[i] as u64 + prod[i] as u64 + carry;
            t[i] = s as u32;
            carry = s >> 32;
        }
    }
    let mut r = [0u32; 8];
    r.copy_from_slice(&t[..8]);
    // r < 2^256 < 2p olduğundan en fazla bir çıkarma gerekir.
    while cmp_limbs(&r, &P) != Ordering::Less {
        sub8(&mut r, &P);
    }
    r
}

/// Genel modüler indirgeme: ikili uzun bölme (p dışı modüller için).
fn reduce_bin(w: &[u32; 16], m: &U256) -> U256 {
    let mut m9 = [0u32; 9];
    m9[..8].copy_from_slice(m);
    let mut r = [0u32; 9];
    for i in (0..512).rev() {
        let bit = (w[i / 32] >> (i % 32)) & 1;
        let mut carry = bit;
        for limb in r.iter_mut() {
            let hi = *limb >> 31;
            *limb = (*limb << 1) | carry;
            carry = hi;
        }
        // r < 2m olduğundan tek koşullu çıkarma yeterli.
        if r[8] != 0 || cmp_limbs(&r[..8], m) != Ordering::Less {
            sub9(&mut r, &m9);
        }
    }
    let mut out = [0u32; 8];
    out.copy_from_slice(&r[..8]);
    out
}

fn madd(a: &U256, b: &U256, m: &U256) -> U256 {
    let mut s = [0u32; 9];
    let mut carry = 0u64;
    for i in 0..8 {
        let t = a[i] as u64 + b[i] as u64 + carry;
        s[i] = t as u32;
        carry = t >> 32;
    }
    s[8] = carry as u32;
    if s[8] != 0 || cmp_limbs(&s[..8], m) != Ordering::Less {
        let mut m9 = [0u32; 9];
        m9[..8].copy_from_slice(m);
        sub9(&mut s, &m9);
    }
    let mut r = [0u32; 8];
    r.copy_from_slice(&s[..8]);
    r
}

fn msub(a: &U256, b: &U256, m: &U256) -> U256 {
    let mut d = *a;
    let mut borrow = 0i64;
    for i in 0..8 {
        let v = a[i] as i64 - b[i] as i64 - borrow;
        if v < 0 {
            d[i] = (v + (1i64 << 32)) as u32;
            borrow = 1;
        } else {
            d[i] = v as u32;
            borrow = 0;
        }
    }
    if borrow == 0 {
        return d; // a >= b: sonuç zaten [0, m) aralığında
    }
    // a < b: (a + m) - b hesaplanır. a + m < 2m ve sonuç < m olduğundan ek
    // indirgeme gerekmez. (d = a - b + 2^256 doğrudan kullanılamaz; çünkü
    // 2^256 - m > 0 kalabilir ve sonuç m'den büyük kalıp madd'ın ön koşulunu
    // bozar.)
    let mut s = [0u32; 9];
    let mut carry = 0u64;
    for i in 0..8 {
        let t = a[i] as u64 + m[i] as u64 + carry;
        s[i] = t as u32;
        carry = t >> 32;
    }
    s[8] = carry as u32;
    let mut borrow2 = 0i64;
    for i in 0..8 {
        let v = s[i] as i64 - b[i] as i64 - borrow2;
        if v < 0 {
            s[i] = (v + (1i64 << 32)) as u32;
            borrow2 = 1;
        } else {
            s[i] = v as u32;
            borrow2 = 0;
        }
    }
    // Sonuç [0, m) olduğundan s[8] ve borrow2 her zaman sıfırdır.
    let _ = (s[8], borrow2);
    let mut r = [0u32; 8];
    r.copy_from_slice(&s[..8]);
    r
}

fn mmul(a: &U256, b: &U256, m: &U256) -> U256 {
    let w = mul_wide(a, b);
    if *m == P {
        reduce_p(&w)
    } else {
        reduce_bin(&w, m)
    }
}

/// a^e mod m (256 bitlik üs, kare-ve-çarp).
fn mpow(a: &U256, e: &U256, m: &U256) -> U256 {
    let mut r = ONE;
    for i in (0..256).rev() {
        r = mmul(&r, &r, m);
        if (e[i / 32] >> (i % 32)) & 1 == 1 {
            r = mmul(&r, a, m);
        }
    }
    r
}

/// Asal modülde ters: a^(m-2) mod m.
fn minv(a: &U256, m: &U256) -> U256 {
    let mut e = *m;
    sub8(&mut e, &[2, 0, 0, 0, 0, 0, 0, 0]); // m - 2 (borrow yok)
    mpow(a, &e, m)
}

// ---------------------------------------------------------------------------
// Nokta işlemleri (Jacobian)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Pt {
    x: U256,
    y: U256,
    z: U256,
}

const fn infinity() -> Pt {
    Pt { x: ZERO, y: ZERO, z: ZERO }
}

fn is_inf(p: &Pt) -> bool {
    p.z == ZERO
}

fn base_point() -> Pt {
    Pt { x: GX, y: GY, z: ONE }
}

/// Çiftleme (a = -3): M = 3·(X² - Z⁴), S = 4·X·Y².
fn pt_double(p: &Pt) -> Pt {
    if is_inf(p) {
        return *p;
    }
    let z2 = mmul(&p.z, &p.z, &P);
    let z4 = mmul(&z2, &z2, &P);
    let x2 = mmul(&p.x, &p.x, &P);
    let t = msub(&x2, &z4, &P); // X² - Z⁴
    let t2 = madd(&t, &t, &P);
    let m = madd(&t, &t2, &P); // 3·(X² - Z⁴)

    let y2 = mmul(&p.y, &p.y, &P);
    let xy2 = mmul(&p.x, &y2, &P);
    let two_xy2 = madd(&xy2, &xy2, &P);
    let s = madd(&two_xy2, &two_xy2, &P); // 4·X·Y²

    let m2 = mmul(&m, &m, &P);
    let two_s = madd(&s, &s, &P);
    let x3 = msub(&m2, &two_s, &P);

    let y4 = mmul(&y2, &y2, &P);
    let y4b = madd(&y4, &y4, &P); // 2·Y⁴
    let y4c = madd(&y4b, &y4b, &P); // 4·Y⁴
    let eight_y4 = madd(&y4c, &y4c, &P); // 8·Y⁴

    let s_minus_x3 = msub(&s, &x3, &P);
    let y3 = msub(&mmul(&m, &s_minus_x3, &P), &eight_y4, &P);

    let yz = mmul(&p.y, &p.z, &P);
    let z3 = madd(&yz, &yz, &P);

    Pt { x: x3, y: y3, z: z3 }
}

/// Toplama (EFD add-2007-bl, Jacobian + Jacobian).
fn pt_add(a: &Pt, b: &Pt) -> Pt {
    if is_inf(a) {
        return *b;
    }
    if is_inf(b) {
        return *a;
    }
    let z1z1 = mmul(&a.z, &a.z, &P);
    let z2z2 = mmul(&b.z, &b.z, &P);
    let u1 = mmul(&a.x, &z2z2, &P);
    let u2 = mmul(&b.x, &z1z1, &P);
    let s1 = mmul(&mmul(&a.y, &b.z, &P), &z2z2, &P);
    let s2 = mmul(&mmul(&b.y, &a.z, &P), &z1z1, &P);

    let h = msub(&u2, &u1, &P);
    let r = msub(&s2, &s1, &P);
    if h == ZERO {
        return if r == ZERO { pt_double(a) } else { infinity() };
    }
    let r = madd(&r, &r, &P); // 2·(S2 - S1)

    let h2 = madd(&h, &h, &P);
    let i = mmul(&h2, &h2, &P); // (2H)²
    let j = mmul(&h, &i, &P);
    let v = mmul(&u1, &i, &P);

    let r2 = mmul(&r, &r, &P);
    let two_v = madd(&v, &v, &P);
    let x3 = msub(&msub(&r2, &j, &P), &two_v, &P);

    let s1j = mmul(&s1, &j, &P);
    let two_s1j = madd(&s1j, &s1j, &P);
    let y3 = msub(&mmul(&r, &msub(&v, &x3, &P), &P), &two_s1j, &P);

    let z1z2 = madd(&a.z, &b.z, &P);
    let zsum2 = mmul(&z1z2, &z1z2, &P);
    let inner = msub(&msub(&zsum2, &z1z1, &P), &z2z2, &P);
    let z3 = mmul(&inner, &h, &P);

    Pt { x: x3, y: y3, z: z3 }
}

/// k · P (çiftle-ve-ekle).
fn pt_mul(k: &U256, p: &Pt) -> Pt {
    let mut acc = infinity();
    for i in (0..256).rev() {
        acc = pt_double(&acc);
        if (k[i / 32] >> (i % 32)) & 1 == 1 {
            acc = pt_add(&acc, p);
        }
    }
    acc
}

/// Jacobian → afin; sonsuz nokta `None`.
fn pt_affine(p: &Pt) -> Option<(U256, U256)> {
    if is_inf(p) {
        return None;
    }
    let zi = minv(&p.z, &P);
    let zi2 = mmul(&zi, &zi, &P);
    let zi3 = mmul(&zi2, &zi, &P);
    Some((mmul(&p.x, &zi2, &P), mmul(&p.y, &zi3, &P)))
}

/// Noktanın eğri üzerinde olduğunu denetler: Y² = X³ + A·X + B (A = -3).
pub fn is_on_curve(x: &U256, y: &U256) -> bool {
    let lhs = mmul(y, y, &P);
    let x2 = mmul(x, x, &P);
    let x3 = mmul(&x2, x, &P);
    let ax = mmul(&A_COEF, x, &P);
    let rhs = madd(&madd(&x3, &ax, &P), &B_CURVE, &P);
    lhs == rhs
}

// ---------------------------------------------------------------------------
// Dış arayüz
// ---------------------------------------------------------------------------

/// ECDHE anahtar çifti: gizli skalar ve `04 || X || Y` biçiminde açık nokta.
pub struct Keypair {
    pub secret: [u8; 32],
    pub public: [u8; 65],
}

fn limbs_from_be_bytes(b: &[u8; 32]) -> U256 {
    from_be(b)
}

/// 32 baytlık tohumdan geçici ECDHE anahtarı üretir (mod n azaltılır).
pub fn generate_keypair(seed: &[u8; 32]) -> Keypair {
    let mut w = [0u32; 16];
    let s = from_be(seed);
    w[..8].copy_from_slice(&s);
    let mut d = reduce_bin(&w, &N);
    if d == ZERO {
        d = ONE; // tohum n'nin katıysa 1'e düşür (pratikte olası değil)
    }
    let q = pt_mul(&d, &base_point());
    let (x, y) = pt_affine(&q).unwrap_or((GX, GY));
    let xb = to_be(&x);
    let yb = to_be(&y);
    let mut public = [0u8; 65];
    public[0] = 0x04;
    public[1..33].copy_from_slice(&xb);
    public[33..65].copy_from_slice(&yb);
    Keypair { secret: to_be(&d), public }
}

/// Paylaşılan gizliyi üretir: X(d · Q_peer). Nokta doğrulanmazsa `None`.
pub fn shared_secret(secret: &[u8; 32], peer_public: &[u8]) -> Option<[u8; 32]> {
    if peer_public.len() != 65 || peer_public[0] != 0x04 {
        return None;
    }
    let mut xb = [0u8; 32];
    let mut yb = [0u8; 32];
    xb.copy_from_slice(&peer_public[1..33]);
    yb.copy_from_slice(&peer_public[33..65]);
    let x = limbs_from_be_bytes(&xb);
    let y = limbs_from_be_bytes(&yb);
    if !is_on_curve(&x, &y) {
        return None;
    }
    let d = limbs_from_be_bytes(secret);
    if d == ZERO {
        return None;
    }
    let peer = Pt { x, y, z: ONE };
    let shared = pt_mul(&d, &peer);
    let (sx, _) = pt_affine(&shared)?;
    Some(to_be(&sx))
}

/// P-256 (secp256r1) ECDSA imza üretimi.
///
/// `secret`: özel anahtar skaları (d).
/// `hash`: imzalanan mesajın SHA-256 özeti.
/// `k`: tek kullanımlık nonce (skalar).
///
/// Dönen değer: `(r, s)`.
pub fn ecdsa_sign_p256(
    secret: &[u8; 32],
    hash: &[u8; 32],
    k: &[u8; 32],
) -> Option<([u8; 32], [u8; 32])> {
    let d = limbs_from_be_bytes(secret);
    if d == ZERO || cmp_limbs(&d, &N) != Ordering::Less {
        return None;
    }
    let kv = limbs_from_be_bytes(k);
    if kv == ZERO || cmp_limbs(&kv, &N) != Ordering::Less {
        return None;
    }

    // r = (k·G).x mod n
    let point = pt_mul(&kv, &base_point());
    let (px, _) = pt_affine(&point)?;
    let mut wide = [0u32; 16];
    wide[..8].copy_from_slice(&px);
    let r = reduce_bin(&wide, &N);
    if r == ZERO {
        return None;
    }

    // s = k^-1 · (e + r·d) mod n
    let e = limbs_from_be_bytes(hash);
    let mut ew = [0u32; 16];
    ew[..8].copy_from_slice(&e);
    let e = reduce_bin(&ew, &N);

    let kinv = minv(&kv, &N);
    let rd = mmul(&r, &d, &N);
    let sum = madd(&e, &rd, &N);
    let s = mmul(&kinv, &sum, &N);
    if s == ZERO {
        return None;
    }
    Some((to_be(&r), to_be(&s)))
}

/// P-256 (secp256r1) ECDSA imza doğrulaması (FIPS 186-4 §6.4).
///
/// `pubkey`: 04 || X || Y (65 bayt, sıkıştırılmamış SEC1 noktası).
/// `hash`: imzalanan mesajın SHA-256 özeti.
/// `r`, `s`: ECDSA imza bileşenleri (big-endian, 32 bayt).
///
/// Doğrulama: `X = u1·G + u2·Q` noktasının x bileşeni `r` ile eşleşmeli.
pub fn ecdsa_verify_p256(
    pubkey: &[u8; 65],
    hash: &[u8; 32],
    r: &[u8; 32],
    s: &[u8; 32],
) -> bool {
    if pubkey[0] != 0x04 {
        return false;
    }
    let mut xb = [0u8; 32];
    let mut yb = [0u8; 32];
    xb.copy_from_slice(&pubkey[1..33]);
    yb.copy_from_slice(&pubkey[33..65]);
    let x = limbs_from_be_bytes(&xb);
    let y = limbs_from_be_bytes(&yb);
    if !is_on_curve(&x, &y) {
        return false;
    }

    let rv = limbs_from_be_bytes(r);
    let sv = limbs_from_be_bytes(s);
    // 1 <= r, s <= n-1
    if rv == ZERO || sv == ZERO {
        return false;
    }
    if cmp_limbs(&rv, &N) != Ordering::Less || cmp_limbs(&sv, &N) != Ordering::Less {
        return false;
    }

    // P-256'da hash (256 bit) ve n (256 bit) aynı uzunlukta: e = hash.
    let e = limbs_from_be_bytes(hash);

    // w = s^-1 mod n
    let w = minv(&sv, &N);
    let u1 = mmul(&e, &w, &N);
    let u2 = mmul(&rv, &w, &N);

    // X = u1·G + u2·Q
    let q = Pt { x, y, z: ONE };
    let p1 = pt_mul(&u1, &base_point());
    let p2 = pt_mul(&u2, &q);
    let sum = pt_add(&p1, &p2);
    let (ax, _) = match pt_affine(&sum) {
        Some(v) => v,
        None => return false,
    };

    // x mod n == r ?
    let mut wide = [0u32; 16];
    wide[..8].copy_from_slice(&ax);
    let xn = reduce_bin(&wide, &N);
    cmp_limbs(&xn, &rv) == Ordering::Equal
}

/// ECDSA imzasının DER çözümü: `SEQUENCE { r INTEGER, s INTEGER }`.
///
/// Başarı durumunda `(r, s)` değerleri 32 bayta tamamlanır.
pub fn parse_ecdsa_der_signature(sig: &[u8]) -> Option<([u8; 32], [u8; 32])> {
    if sig.len() < 8 || sig[0] != 0x30 {
        return None;
    }
    let mut pos = 2usize;
    if sig[1] & 0x80 != 0 {
        let n = (sig[1] & 0x7F) as usize;
        if n == 0 || n > 2 || pos + n > sig.len() {
            return None;
        }
        let mut total = 0usize;
        for i in 0..n {
            total = (total << 8) | sig[pos + i] as usize;
        }
        pos += n;
        if pos + total != sig.len() {
            return None;
        }
    } else if pos + sig[1] as usize != sig.len() {
        return None;
    }

    // r INTEGER
    if pos + 2 > sig.len() || sig[pos] != 0x02 {
        return None;
    }
    let r_len = sig[pos + 1] as usize;
    if r_len == 0 || r_len > 33 || pos + 2 + r_len > sig.len() {
        return None;
    }
    let r_bytes = &sig[pos + 2..pos + 2 + r_len];
    pos += 2 + r_len;

    // s INTEGER
    if pos + 2 > sig.len() || sig[pos] != 0x02 {
        return None;
    }
    let s_len = sig[pos + 1] as usize;
    if s_len == 0 || s_len > 33 || pos + 2 + s_len > sig.len() {
        return None;
    }
    let s_bytes = &sig[pos + 2..pos + 2 + s_len];

    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    if r_len == 33 {
        if r_bytes[0] != 0x00 {
            return None;
        }
        r.copy_from_slice(&r_bytes[1..33]);
    } else {
        r[32 - r_len..].copy_from_slice(r_bytes);
    }
    if s_len == 33 {
        if s_bytes[0] != 0x00 {
            return None;
        }
        s.copy_from_slice(&s_bytes[1..33]);
    } else {
        s[32 - s_len..].copy_from_slice(s_bytes);
    }
    Some((r, s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn parse_hex(s: &str) -> Vec<u8> {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(s.len() / 2);
        let mut i = 0;
        while i + 1 < b.len() {
            let hi = (b[i] as char).to_digit(16).unwrap() as u8;
            let lo = (b[i + 1] as char).to_digit(16).unwrap() as u8;
            out.push((hi << 4) | lo);
            i += 2;
        }
        out
    }

    #[allow(dead_code)]
    // NIST CAVP / bilinen vektörler (Python ile üretildi, ayrıca
    // d=C9AFA9...:x=60FED4BA... örneği yaygın bilinen NIST numunesidir).
    #[test]
    fn g_on_curve() {
        assert!(is_on_curve(&GX, &GY));
    }

    #[test]
    fn base_point_order() {
        // n·G = sonsuz nokta olmalı (grup mertebesi doğruluğu).
        let inf = pt_mul(&N, &base_point());
        assert!(is_inf(&inf));
        // (n-1)·G = -G: x = Gx, y = p - Gy
        let mut nm1 = N;
        sub8(&mut nm1, &ONE);
        let p = pt_mul(&nm1, &base_point());
        let (x, y) = pt_affine(&p).expect("(n-1)G sonsuz olmamalı");
        assert_eq!(x, GX);
        let neg_y = msub(&ZERO, &GY, &P);
        assert_eq!(y, neg_y);
    }

    #[test]
    fn double_g_known_answer() {
        let d = pt_double(&Pt { x: GX, y: GY, z: ONE });
        let (x, y) = pt_affine(&d).unwrap();
        assert_eq!(to_be(&x), {
            let v = parse_hex("7CF27B188D034F7E8A52380304B51AC3C08969E277F21B35A60B48FC47669978");
            let mut a = [0u8; 32];
            a.copy_from_slice(&v);
            a
        });
        assert_eq!(to_be(&y), {
            let v = parse_hex("07775510DB8ED040293D9AC69F7430DBBA7DADE63CE982299E04B79D227873D1");
            let mut a = [0u8; 32];
            a.copy_from_slice(&v);
            a
        });
    }

    #[test]
    fn scalar_mult_known_answers() {
        let cases = [(
            "C9AFA9D845BA75166B5C215767B1D6934E50C3DB36E89B127B8A622B120F6721",
            "60FED4BA255A9D31C961EB74C6356D68C049B8923B61FA6CE669622E60F29FB6",
            "7903FE1008B8BC99A41AE9E95628BC64F2F1B20C2D7E9F5177A3C294D4462299",
        ), (
            "1E6F2A8B9C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4E5F60718293A4B5C6D7",
            "76BC9AE66B389722BD200A266ABC2259BDA43A82448179BD6EA841E625B3657B",
            "C72A7975EF89038C37B59BBB0AE5F341037497533733D2A739C64D901D5BACE9",
        ), (
            "0000000000000000000000000000000000000000000000000000000000000007",
            "8E533B6FA0BF7B4625BB30667C01FB607EF9F8B8A80FEF5B300628703187B2A3",
            "73EB1DBDE03318366D069F83A6F5900053C73633CB041B21C55E1A86C1F400B4",
        )];
        for (k, qx, qy) in cases {
            let kp = generate_keypair(&{
                let mut s = [0u8; 32];
                let v = parse_hex(k);
                s.copy_from_slice(&v);
                s
            });
            // tohum skaları mod n'ye indirgenir; bu vektörler n'den küçük.
            assert_eq!(&kp.public[1..33], &parse_hex(qx)[..], "k = {}", k);
            assert_eq!(&kp.public[33..65], &parse_hex(qy)[..], "k = {}", k);
        }
    }

    #[test]
    fn ecdh_shared_secret_known_answer() {
        let d0 = {
            let mut s = [0u8; 32];
            let v = parse_hex("C9AFA9D845BA75166B5C215767B1D6934E50C3DB36E89B127B8A622B120F6721");
            s.copy_from_slice(&v);
            s
        };
        // Q1 (d1·G) vektörden alınır; paylaşılan X = Z vektörü.
        let mut peer = [0u8; 65];
        peer[0] = 0x04;
        let qx = parse_hex("76BC9AE66B389722BD200A266ABC2259BDA43A82448179BD6EA841E625B3657B");
        let qy = parse_hex("C72A7975EF89038C37B59BBB0AE5F341037497533733D2A739C64D901D5BACE9");
        peer[1..33].copy_from_slice(&qx);
        peer[33..65].copy_from_slice(&qy);

        let shared = shared_secret(&d0, &peer).expect("ECDHE paylaşılan secret");
        let expected = parse_hex("EC87D440BE564C949D90D601AA01FF9776F01BF041D049E35DEA80C1BD5729FF");
        assert_eq!(&shared[..], &expected[..]);
    }

    #[test]
    fn ecdh_symmetry() {
        let d0 = {
            let mut s = [0u8; 32];
            let v = parse_hex("C9AFA9D845BA75166B5C215767B1D6934E50C3DB36E89B127B8A622B120F6721");
            s.copy_from_slice(&v);
            s
        };
        let d1 = {
            let mut s = [0u8; 32];
            let v = parse_hex("1E6F2A8B9C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4E5F60718293A4B5C6D7");
            s.copy_from_slice(&v);
            s
        };
        let kp0 = generate_keypair(&d0);
        let kp1 = generate_keypair(&d1);
        let s0 = shared_secret(&d0, &kp1.public).unwrap();
        let s1 = shared_secret(&d1, &kp0.public).unwrap();
        assert_eq!(s0, s1);
    }

    #[test]
    fn reject_invalid_points() {
        let d = [7u8; 32];
        // Yanlış uzunluk / yanlış ön ek / eğri dışı nokta reddedilir.
        assert!(shared_secret(&d, &[0u8; 64]).is_none());
        let mut p = [0u8; 65];
        p[0] = 0x05;
        p[1] = 1;
        assert!(shared_secret(&d, &p).is_none());
        let mut off = [0u8; 65];
        off[0] = 0x04;
        off[1] = 1; // x = 1, y = 0 eğri üzerinde değil
        assert!(shared_secret(&d, &off).is_none());
        // Gizli skalar sıfırsa işlem tanımsız.
        let kp = generate_keypair(&d);
        assert!(shared_secret(&[0u8; 32], &kp.public).is_none());
    }

    #[test]
    fn identity_secret_returns_peer_x() {
        let kp = generate_keypair(&[9u8; 32]);
        let mut one = [0u8; 32];
        one[31] = 1;
        let shared = shared_secret(&one, &kp.public).expect("1·Q tanımlı");
        assert_eq!(&shared[..], &kp.public[1..33]);
    }

    #[test]
    fn point_arithmetic_consistency() {
        // 0) minv: a · a⁻¹ ≡ 1 (mod p)
        let zi = minv(&GY, &P);
        assert_eq!(mmul(&GY, &zi, &P), ONE, "minv bozuk: a*a^-1 != 1");
        let g = Pt { x: GX, y: GY, z: ONE };
        // 1) 1·G = G (pt_mul + minv + pt_affine)
        let one_g = pt_mul(&ONE, &g);
        assert!(!is_inf(&one_g), "1G sonsuz nokta");
        let (x, y) = pt_affine(&one_g).expect("1G affine donusum");
        assert_eq!(x, GX, "1G x != Gx");
        assert_eq!(y, GY, "1G y != Gy");
        // 2) 2·G (pt_double) eğri üzerinde mi ve bilinen vektörle eşleşiyor mu
        let d = pt_double(&g);
        let (dx, dy) = pt_affine(&d).expect("2G affine donusum");
        assert!(is_on_curve(&dx, &dy), "2G egride degil");
        // 3) 3·G = 2G + G (pt_add, farklı noktalar)
        let t = pt_add(&d, &g);
        let (tx, ty) = pt_affine(&t).expect("3G affine donusum");
        assert!(is_on_curve(&tx, &ty), "3G egride degil");
        // 4) 4·G = 2·(2G) tekrar çiftleme
        let f = pt_double(&d);
        let (fx, fy) = pt_affine(&f).expect("4G affine donusum");
        assert!(is_on_curve(&fx, &fy), "4G egride degil");
        // 4G, hem 2·(2G) hem de 3G + G ile elde edilmelidir.
        let f2 = pt_add(&t, &g);
        let (f2x, f2y) = pt_affine(&f2).expect("3G + G affine");
        assert_eq!(fx, f2x, "4G x tutarsiz");
        assert_eq!(fy, f2y, "4G y tutarsiz");
    }

    #[test]
    fn ecdsa_verify_known_answer() {
        // P-256 ECDSA test vektörü (Python ile üretildi).
        let mut pubkey = [0u8; 65];
        pubkey[0] = 0x04;
        pubkey[1..33].copy_from_slice(
            &parse_hex("60FED4BA255A9D31C961EB74C6356D68C049B8923B61FA6CE669622E60F29FB6"),
        );
        pubkey[33..65].copy_from_slice(
            &parse_hex("7903FE1008B8BC99A41AE9E95628BC64F2F1B20C2D7E9F5177A3C294D4462299"),
        );

        let hash = parse_hex("23A684D8E64D69E3B9F2568CF4C088087C6C63D11858DC02BEA2805828694645");
        let mut h = [0u8; 32];
        h.copy_from_slice(&hash);

        let r = parse_hex("EFD48B2AACB6A8FD1140DD9CD45E81D69D2C877B56AAF991C34D0EA84EAF3716");
        let s = parse_hex("EB4FB2D3DB54C0251C9AFF85224F763B072B320518A123599C62FDBE2B561E1A");
        let mut ra = [0u8; 32];
        let mut sa = [0u8; 32];
        ra.copy_from_slice(&r);
        sa.copy_from_slice(&s);

        assert!(ecdsa_verify_p256(&pubkey, &h, &ra, &sa), "gecerli imza reddedilmemeli");

        // Bozuk özet reddedilmeli.
        let mut bad_hash = h;
        bad_hash[0] ^= 0xff;
        assert!(!ecdsa_verify_p256(&pubkey, &bad_hash, &ra, &sa));

        // Bozuk imza bileşeni reddedilmeli.
        let mut bad_r = ra;
        bad_r[0] ^= 0xff;
        assert!(!ecdsa_verify_p256(&pubkey, &h, &bad_r, &sa));

        // Eğri dışı nokta reddedilmeli.
        let mut bad_key = pubkey;
        bad_key[1] ^= 0xff;
        assert!(!ecdsa_verify_p256(&bad_key, &h, &ra, &sa));
    }

    #[test]
    fn ecdsa_der_signature_parsing() {
        let der = parse_hex(
            "3046022100efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716\
             022100eb4fb2d3db54c0251c9aff85224f763b072b320518a123599c62fdbe2b561e1a",
        );
        let (r, s) = parse_ecdsa_der_signature(&der).expect("DER imza cozulemeli");
        assert_eq!(
            r.to_vec(),
            parse_hex("EFD48B2AACB6A8FD1140DD9CD45E81D69D2C877B56AAF991C34D0EA84EAF3716")
        );
        assert_eq!(
            s.to_vec(),
            parse_hex("EB4FB2D3DB54C0251C9AFF85224F763B072B320518A123599C62FDBE2B561E1A")
        );

        // Bozuk girdiler reddedilmeli.
        assert!(parse_ecdsa_der_signature(&[]).is_none());
        assert!(parse_ecdsa_der_signature(&[0x30, 0x00]).is_none());
        assert!(parse_ecdsa_der_signature(&[0x31, 0x02, 0x00, 0x00]).is_none());
    }

    #[test]
    fn field_reduction_consistency() {
        // P üzerinde temel özdekler: (p-1) + 1 = 0, p ≡ 0.
        let mut pm1 = P;
        sub8(&mut pm1, &ONE);
        assert_eq!(madd(&pm1, &ONE, &P), ZERO);
        assert_eq!(mmul(&ZERO, &N, &P), ZERO);
        // 2^256 katlama çarpanı doğru olmalı: M256 ≡ 2^256 (mod p)
        let mut w = [0u32; 16];
        w[8] = 1; // 2^256
        assert_eq!(reduce_p(&w), M256);
        // Genel indirgeme de aynı sonucu vermeli.
        assert_eq!(reduce_bin(&w, &P), M256);
        // N modülünde indirgeme: n ≡ 0 (mod n)
        let mut wn = [0u32; 16];
        wn[..8].copy_from_slice(&N);
        assert_eq!(reduce_bin(&wn, &N), ZERO);
    }
}
