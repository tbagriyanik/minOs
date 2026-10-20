# Security Policy

## Desteklenen Sürümler

| Sürüm    | Destekleniyor          |
| :------- | :--------------------- |
| 1.7.x    | :white_check_mark:     |
| 1.6.x    | :white_check_mark:     |
| 1.5.x    | :white_check_mark:     |
| < 1.5    | :x:                    |

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

minOs'ta tüm kriptografik ilkeller **elle yazılmıştır** (`src/drivers/crypto/`) ve harici kütüphane
bağımlılığı yoktur. Bu nedenle güvenlik duruşu açıkça şu şekilde tanımlanır:

| Özellik | Durum |
| :--- | :--- |
| TLS sürümü | **Yalnızca TLS 1.2** (TLS 1.3 desteklenmez) |
| Cipher suite | **Yalnızca `TLS_RSA_WITH_AES_128_GCM_SHA256` (0x009C)** |
| Anahtar değişimi | RSA (PKCS#1 v1.5) — ECDHE/DHE yok |
| Simetrik şifreleme | AES-128-GCM (128-bit anahtar) |
| Hash / PRF | SHA-256, P_SHA256 |
| Sertifika zinciri doğrulama | **Yok** — sunucu sertifikası okunur, ancak güvenilir kök/CA veya hostname eşleşmesi doğrulanmaz |
| İstemci kimlik doğrulama | Yok (anonymous TLS) |
| Rastgelelik | `main_monotonic_ms()` tabanlı istemci random'ı — CSPRNG değildir |

### Bilinen kısıtlar ve riskler

- **Sertifika doğrulaması yapılmaz.** MITM'e karşı koruma sağlanmaz; yalnızca trafik gizliliği
  (server key exchange sonrası) sağlanır. Güvenilir olmayan ağlarda kullanılmamalıdır.
- **TLS 1.3 ve modern cipher suite'ler yoktur.** Yalnızca RSA anahtar değişimi kabul eden sunucular
  ile çalışır; ECDHE zorunlu sunucular bağlantıyı reddedebilir.
- **RSA ile anahtar değişimi kriptografik olarak zayıftır** (forward secrecy sağlamaz) ve padding
  oracle saldırılarına karşı ek koruma (RFC 3211 / blinding) uygulanmaz.
- **Anahtar türetme ve PRF** standartlara uygun çalışır; `master_secret` ve `pre_master_secret`
  bellekte düz (plaintext) tutulur.
- **Sertifika özel anahtarları** bu depoda bulunmaz ve yalnızca açık (public) anahtar işlemleri
  (`rsa.rs`) uygulanmıştır.
- **Kriptografik kod `src/drivers/crypto/` altındadır.** Bu modüldeki değişiklikler, sertifika
  doğrulaması eklenene kadar **güvenlik açığı** olarak değerlendirilmelidir.

## Güvenlik ile İlgili Olmayan Konular

Genel hata raporları (çökme, ekran bozulması, performans sorunları) normal GitHub issue olarak
bildirilebilir.
