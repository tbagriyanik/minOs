# Security Policy

## Desteklenen Sürümler

| Sürüm    | Destekleniyor          |
| :------- | :--------------------- |
| 2.1.x    | :white_check_mark:     |
| 2.0.x    | :white_check_mark:     |
| 1.7.x    | :white_check_mark:     |
| < 1.7    | :x:                    |

## Güvenlik Bildirimi (Reporting a Vulnerability)

Bir güvenlik açığı bulduğunuzu düşünüyorsanız, sorunu **açık bir GitHub issue olarak açmayın**.
Bunun yerine proje sahibine özel olarak bildirin:

1. **Konu (Subject):** `Security: <kısa açıklama>` (örn. `Security: TLS oturumunda tampon taşması`)
2. **Açıklama:** Etkilenen bileşen (`src/drivers/tls.rs`, `src/drivers/crypto/…` vb.), yeniden üretim adımları, etki (veri sızıntısı, bellek bozulması, kimlik taklidi vb.).
3. **Önem derecesi:** Kritik / Yüksek / Orta / Düşük.
4. **İletişim:** GitHub kullanıcı adı veya e-posta adresi.

Yanıt süresi: açıklar **7 gün** içinde onaylanır, düzeltme planı **14 gün** içinde paylaşılır.
Düzeltme yayınlandıktan sonra **30 gün** boyunca kamuya açıklanmaz; ardından yama notu ile birlikte duyurulur.

## TLS / Kriptografi Politikası

minOs'ta kriptografik ilkeller ağırlıklı olarak **elle yazılmıştır** (`src/drivers/crypto/`). Güvenlik duruşu şu şekilde tanımlanır:

| Özellik | Durum |
| :--- | :--- |
| TLS sürümü | **TLS 1.3** (tam el sıkışma) ve istemci tarafında **TLS 1.2** desteği |
| Cipher suite (TLS 1.3) | `TLS_AES_128_GCM_SHA256` (0x1301) |
| Cipher suite (TLS 1.2) | `TLS_RSA_WITH_AES_128_GCM_SHA256` (0x009C) |
| Anahtar değişimi | TLS 1.3: **ECDHE (secp256r1)** + HKDF anahtar planı; TLS 1.2: RSA (PKCS#1 v1.5) |
| Simetrik şifreleme | AES-128-GCM (128-bit anahtar) |
| Hash / PRF | TLS 1.3: SHA-256 / HKDF-Expand-Label; TLS 1.2: SHA-256 / P_SHA256 |
| Sertifika zinciri doğrulama | **Var** — gömülü kök CA deposu (`src/drivers/crypto/roots/`: ISRG Root X1 & X2, GTS Root R1 & R2, DigiCert Global Root CA & G2, SSL.com TLS ECC Root CA 2022) üzerinden RFC 5280 zincir doğrulaması; zincir derinliği `MAX_CHAIN_DEPTH` (6) ile sınırlıdır |
| Kimlik doğrulama | Sertifika geçerlilik aralığı (`not_before`/`not_after`) CMOS RTC ile, hedef ana makine adı CN/SAN üzerinden RFC 6125 kurallarıyla (tek düzeyli joker dahil) doğrulanır |
| İptal kontrolü | CRL / OCSP (`revocation_core.rs`) DER ayrıştırma; `revocation.rs` fail-closed politika uygular (durum bilinmiyorsa bağlantı reddedilir) |
| `close_notify` | RFC 8446 §6.1 uyarınca **zarif kapanış** olarak işlenir (fatal hata değil); aynı kayıttaki uygulama verisi korunur |
| İstemci kimlik doğrulama | Yok (anonymous TLS) |
| Rastgelelik | `main_monotonic_ms()` tabanlı istemci random'ı — **CSPRNG değildir** |

### Bilinen kısıtlar ve riskler

- **Sertifika zinciri doğrulaması gömülü kök CA deposuyla sınırlıdır.** Depoda bulunmayan bir kök CA'ya
  dayanan siteler (`UntrustedRoot`) reddedilir; ayrıca CRL/OCSP yanıtlarının **otomatik indirilmesi
  (HTTP/OCSP isteği üretimi) henüz yoktur**, iptal durumu `Manual` kaynakla elle işaretlenebilir.
- **CSPRNG yoktur.** İstemci `random` ve `pre_master_secret`/`master_secret` üretimi
  `main_monotonic_ms()` tabanlıdır; kriptografik olarak güvenli rastgelelik sağlanmaz.
- **Anahtar materyali bellekte düz (plaintext) tutulur.** `master_secret`, oturum anahtarları ve
  trafik sırları için bellek sıfırlama (zeroization) uygulanmaz.
- **TLS 1.2 tarafında ECDHE yoktur.** RSA anahtar değişimi (PKCS#1 v1.5) forward secrecy sağlamaz ve
  padding oracle saldırılarına karşı ek koruma (blinding) uygulanmaz. TLS 1.3 tarafı ECDHE (secp256r1)
  kullanır ve forward secrecy sağlar.
- **Gerçek zaman saati zorunludur.** Sertifika geçerlilik kontrolü CMOS RTC'ye bağlıdır; saat
  ayarlanmamışsa geçerli sertifikalar reddedilebilir.
- **Kriptografik kod `src/drivers/crypto/` altındadır.** Bu modüldeki değişiklikler güvenlik açısından
  kritik olarak değerlendirilmelidir.

## Güvenlik ile İlgili Olmayan Konular

Genel hata raporları (çökme, ekran bozulması, performans sorunları) normal GitHub issue olarak
bildirilebilir.
