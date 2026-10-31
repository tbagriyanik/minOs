# Ağ ve İnternet

minOs ağ yığını, TCP/IP protokolü, DNS çözümleme (A/AAAA), HTTP/1.1, TLS 1.3 (HTTPS) ve hazırlanan HTTP/2 istemci altyapısı içerir.

---

## Ağ Yığını (netstack.rs)

| Özellik | Açıklama |
| :--- | :--- |
| **IPv4/IPv6** | `IpAddress` (V4/V6), RFC 4291 adres ayrıştırıcı (`::` sıkıştırma ve `[...]` formatı) |
| **ICMPv6** | Echo Request/Reply (`ping6`), Neighbor Discovery (Solicitation/Advertisement) |
| **ETHERTYPE_IPV6** | 0x86DD desteği |
| **Multicast MAC** | RFC 2464 Multicast MAC haritalama |
| **Pseudo-header Checksum** | RFC 8200 sözde başlık checksum hesaplama |
| **Soket API** | `AF_INET6` (10), `resolve_ip()` / `resolve_ipv6()`, Web Tarayıcısında IPv6 adres doğrulaması |
| **DNS A / AAAA** | `dns_query()` + `dns_parse_a()` (QTYPE=A) ve `dns_query_aaaa()` + `dns_parse_aaaa()` (QTYPE=AAAA, RFC 3596); `resolve_host()` / `resolve_host_aaaa()` (5 dakikalık önbellek) ve her ikisini deneyen `resolve_host_ip()` |

> **IPv6 Sınırı:** Alan adı çözümlemesi AAAA desteklidir, ancak IPv6 bağlantısı kurulduktan sonra oluşan hatalarda IPv4 yolu otomatik denenmez ve `dns`/`nslookup` çıktısı henüz AAAA kaydını raporlamaz.

---

## HTTP/1.1

| Özellik | Açıklama |
| :--- | :--- |
| **Protokol** | HTTP/1.1, `Connection: close`, `Host` ve `User-Agent: minOs/2.1` başlıkları |
| **Yöntemler** | GET, POST (`application/x-www-form-urlencoded`) |
| **Yönlendirme** | HTTP 3xx (301, 302, 303, 307, 308) ve `Location:` başlığı çözümleyicisi (azami 3 atlama) |
| **Çerezler** | `Set-Cookie:` başlıklarının otomatik kaydedilmesi ve domain/path eşleşmesine göre `Cookie:` başlığının gönderilmesi |
| **Parçalı Gönderim** | `Transfer-Encoding: chunked` yanıtlarının yerinde çözülmesi |
| **Zaman Aşımı** | 15 saniyelik ağ zaman aşımı |
| **Tamponlar** | 64 KB TCP alma tamponu, 64 KB dinamik yanıt tamponu |
| **IPv6 Öncelikli Bağlantı** | İstek öncesi AAAA çözülür ve `connect_v6` ile IPv6 bağlanılır (HTTPS'te TLS el sıkışması IPv6 üzerinden); AAAA yoksa/başarısızsa IPv4 `fetch_once` yoluna düşülür |

---

## HTTP/2 (Altyapı)

`src/drivers/http.rs` içinde RFC 7540 istemci çekirdeği bulunur.

| Özellik | Açıklama |
| :--- | :--- |
| **Çerçeve Katmanı** | `h2` modülü: `build_frame`, `parse_frame_header`, `build_window_update`; DATA, HEADERS, SETTINGS, PING, GOAWAY, RST_STREAM, WINDOW_UPDATE sabitleri |
| **HPACK** | `encode_get_headers` ile `:method GET`, `:scheme`, `:authority`, `:path` başlık bloğu kodlaması (statik tablo indeksleri) |
| **Bağlantı** | `Http2Connection::connect` (istemci önyüzü + sunucu SETTINGS bekleme), `get()` (tek soket üzerinden çoklu akış, tek sayılı akış ID'leri), `close()` (GOAWAY) |
| **Kolaylık API** | `http2_get(url, &mut buf)` |
| **Sınırlar** | HTTPS'te TLS kaydı şifrelemesi, `Upgrade: h2c` / ALPN `h2` pazarlığı ve HPACK dinamik tablo + tam başlık çözümlemesi henüz yok (yalnızca `:status 200` sezgisel olarak bulunur) |

> Tarayıcı hâlâ **HTTP/1.1** kullanır; HTTP/2 yolu tamamlandığında bağlanacaktır.

---

## TLS 1.3 (HTTPS)

| Özellik | Açıklama |
| :--- | :--- |
| **El Sıkışması** | Tam TLS 1.3 el sıkışma döngüsü (RSA & ECDSA CertificateVerify, HKDF anahtar planı, şifreli kayıt işleme) |
| **RFC 8446** | AAD uzunluğu (§5.2), `Finished` anahtar türetimi (§4.4.4), `close_notify` zarif kapanış (§6.1) |
| **Şifreleme** | AES-128-GCM |
| **Sertifika Doğrulama** | X.509, RFC 5280, `not_before` / `not_after` (CMOS RTC ile), CN ve SAN DNS kayıtları (RFC 6125), joker `*.domain.com` |
| **Zincir Derinliği** | `MAX_CHAIN_DEPTH` (6) sertifika sınırı |
| **İptal Kontrolü** | CRL & OCSP ayrıştırma, fail-closed politika |
| **Kök CA Deposu** | 51 güvenilen kök CA sertifikası (`src/drivers/crypto/roots/`) |

> Let's Encrypt ISRG Root X1 & X2, Google Trust Services, DigiCert, SSL.com, GoDaddy, Starfield, IdenTrust, QuoVadis, Certum, Buypass, Actalis, T-TeleSec, Microsoft, D-TRUST, Telia, SwissSign, OISTE WISeKey, FNMT-RCM, SECOM kökleri dahil.

---

## Ağ Teşhisi Komutları (MinShell)

| Komut | Kullanım | Açıklama |
| :--- | :--- | :--- |
| `ipconfig` | `ipconfig` | `eth0` arabiriminin yerel IP, ağ maskesi, ağ geçidi, DNS, MAC adresi ve DHCP durumunu raporlar |
| `ping` | `ping <IPv4>` | Belirtilen IPv4 hedefine ICMP Echo Request paketi gönderir |
| `ping6` | `ping6 <IPv6>` | Belirtilen IPv6 hedefine ICMPv6 Echo Request paketi gönderir |
| `dns` | `dns <alan_adı>` | Belirtilen sunucu adı için DNS sorgusu iletir |
| `curl` | `curl http://<host>[:port][/yol]` | Soket API üzerinden HTTP GET indirir (durum kodu + gövde; chunked kod çözme ve 3xx yönlendirme takibi desteklenir) |

> **Not:** `curl` `https` desteklemez; şifreli sayfalar için **Web Tarayıcısı** uygulamasını (`https://` şeması) kullanın.

---

## Ağ Sürücüleri

| Sürücü | Donanım | Durum |
| :--- | :--- | :--- |
| `e1000.rs` | Intel e1000 / e1000e | 🟢 Tam |
| `i219.rs` | Intel I219-V | 🟢 Tam |
| `net.rs` | Realtek PCIe, Broadcom NetXtreme | 🟢 Tam |
| `virtio_net.rs` | VirtIO-net | 🟢 Tam |
| `atheros.rs` | Atheros Ethernet | 🟡 Kısmi |
| `marvell.rs` | Marvell Ethernet | 🟡 Kısmi |
| `wifi.rs` | Wi-Fi 802.11 | 🔴 İskelet |
