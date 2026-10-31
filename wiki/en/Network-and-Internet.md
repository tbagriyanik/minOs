# Network and Internet

minOs network stack includes TCP/IP protocol, DNS resolution (A/AAAA), HTTP/1.1, TLS 1.3 (HTTPS) and a prepared HTTP/2 client infrastructure.

---

## Network Stack (netstack.rs)

| Feature | Description |
| :--- | :--- |
| **IPv4/IPv6** | `IpAddress` (V4/V6), RFC 4291 address parser (`::` compression and `[...]` format) |
| **ICMPv6** | Echo Request/Reply (`ping6`), Neighbor Discovery (Solicitation/Advertisement) |
| **ETHERTYPE_IPV6** | 0x86DD support |
| **Multicast MAC** | RFC 2464 Multicast MAC mapping |
| **Pseudo-header Checksum** | RFC 8200 pseudo-header checksum calculation |
| **Socket API** | `AF_INET6` (10), `resolve_ip()` / `resolve_ipv6()`, IPv6 address validation in web browser |
| **DNS A / AAAA** | `dns_query()` + `dns_parse_a()` (QTYPE=A) and `dns_query_aaaa()` + `dns_parse_aaaa()` (QTYPE=AAAA, RFC 3596); `resolve_host()` / `resolve_host_aaaa()` (5-minute cache) and `resolve_host_ip()` which tries both |

> **IPv6 Limitation:** Host name resolution supports AAAA, but the IPv4 path is not retried automatically after an established IPv6 connection fails, and `dns`/`nslookup` output does not report AAAA records yet.

---

## HTTP/1.1

| Feature | Description |
| :--- | :--- |
| **Protocol** | HTTP/1.1, `Connection: close`, `Host` and `User-Agent: minOs/2.1` headers |
| **Methods** | GET, POST (`application/x-www-form-urlencoded`) |
| **Redirects** | HTTP 3xx (301, 302, 303, 307, 308) and `Location:` header resolver (max 3 hops) |
| **Cookies** | `Set-Cookie:` headers automatically saved and `Cookie:` header sent based on domain/path matching |
| **Chunked Transfer** | `Transfer-Encoding: chunked` responses decoded in place |
| **Timeout** | 15-second network timeout |
| **Buffers** | 64 KB TCP receive buffer, 64 KB dynamic response buffer |
| **IPv6-First Connection** | AAAA is resolved before the request and IPv6 is used via `connect_v6` (the TLS handshake runs over IPv6 for HTTPS); falls back to the IPv4 `fetch_once` path when AAAA is missing or fails |

---

## HTTP/2 (Infrastructure)

An RFC 7540 client core lives in `src/drivers/http.rs`.

| Feature | Description |
| :--- | :--- |
| **Frame Layer** | `h2` module: `build_frame`, `parse_frame_header`, `build_window_update`; DATA, HEADERS, SETTINGS, PING, GOAWAY, RST_STREAM, WINDOW_UPDATE constants |
| **HPACK** | `encode_get_headers` encodes `:method GET`, `:scheme`, `:authority`, `:path` (static table indices) |
| **Connection** | `Http2Connection::connect` (client preface + server SETTINGS wait), `get()` (multiplexed streams on one socket, odd stream IDs), `close()` (GOAWAY) |
| **Convenience API** | `http2_get(url, &mut buf)` |
| **Limits** | No TLS record encryption on HTTPS, no `Upgrade: h2c` / ALPN `h2` negotiation, and no HPACK dynamic table or full header decoding yet (only `:status 200` is detected heuristically) |

> The browser still uses **HTTP/1.1**; it will switch once the HTTP/2 path is complete.

---

## TLS 1.3 (HTTPS)

| Feature | Description |
| :--- | :--- |
| **Handshake** | Full TLS 1.3 handshake loop (RSA & ECDSA CertificateVerify, HKDF key schedule, encrypted record processing) |
| **RFC 8446** | AAD length (§5.2), `Finished` key derivation (§4.4.4), graceful `close_notify` (§6.1) |
| **Encryption** | AES-128-GCM |
| **Certificate Verification** | X.509, RFC 5280, `not_before` / `not_after` (against CMOS RTC clock), CN and SAN DNS records (RFC 6125), wildcard `*.domain.com` |
| **Chain Depth Limit** | `MAX_CHAIN_DEPTH` (6) certificates |
| **Revocation Checking** | CRL & OCSP parsing, fail-closed policy |
| **Root CA Store** | 51 trusted root CA certificates (`src/drivers/crypto/roots/`) |

> Includes Let's Encrypt ISRG Root X1 & X2, Google Trust Services, DigiCert, SSL.com, GoDaddy, Starfield, IdenTrust, QuoVadis, Certum, Buypass, Actalis, T-TeleSec, Microsoft, D-TRUST, Telia, SwissSign, OISTE WISeKey, FNMT-RCM, SECOM roots.

---

## Network Diagnostic Commands (MinShell)

| Command | Usage | Description |
| :--- | :--- | :--- |
| `ipconfig` | `ipconfig` | Reports `eth0` interface local IP, subnet mask, gateway, DNS, MAC address and DHCP status |
| `ping` | `ping <IPv4>` | Sends ICMP Echo Request packet to specified IPv4 target |
| `ping6` | `ping6 <IPv6>` | Sends ICMPv6 Echo Request packet to specified IPv6 target |
| `dns` | `dns <domain>` | Sends DNS query for specified server name |
| `curl` | `curl http://<host>[:port][/path]` | HTTP GET download via socket API (status code + body; chunked decoding and 3xx redirect following supported) |

> **Note:** `curl` does not support `https`; for encrypted pages use the **Web Browser** application (`https://` scheme).

---

## Network Drivers

| Driver | Hardware | Status |
| :--- | :--- | :--- |
| `e1000.rs` | Intel e1000 / e1000e | 🟢 Full |
| `i219.rs` | Intel I219-V | 🟢 Full |
| `net.rs` | Realtek PCIe, Broadcom NetXtreme | 🟢 Full |
| `virtio_net.rs` | VirtIO-net | 🟢 Full |
| `atheros.rs` | Atheros Ethernet | 🟡 Partial |
| `marvell.rs` | Marvell Ethernet | 🟡 Partial |
| `wifi.rs` | Wi-Fi 802.11 | 🔴 Skeleton |
