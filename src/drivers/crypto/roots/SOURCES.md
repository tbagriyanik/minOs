# Kök / Ara Sertifika Kaynakları (`.der`)

Bu dosya, `src/drivers/crypto/roots/` klasöründeki her bir `.der` sertifika dosyasının
kaynak CA'sını, resmi indirme sayfasını ve indirme tarihini belgelemektedir.

## Amaç

CA kök sertifikaları genelde herkese açık ve serbestçe dağıtılabilir (tarayıcılar ve
işletim sistemleri zaten bunları dağıtır). Ancak bazı CA'ların ticari marka (trademark)
kısıtlamaları olabilir. Bu dosya şeffaflık ve "iyi niyetli kullanım" (good faith use)
kanıtı sağlamak için tutulmaktadır.

## Sertifika Listesi

| # | Dosya | CA / Sertifika | Resmi Kaynak (URL) | İndirme Tarihi |
|---|-------|----------------|-------------------|----------------|
| 1 | `isrgrootx1.der` | ISRG Root X1 (Let's Encrypt) | https://letsencrypt.org/certificates/ | [YYYY-MM-DD] |
| 2 | `isrgrootx2.der` | ISRG Root X2 (Let's Encrypt) | https://letsencrypt.org/certificates/ | [YYYY-MM-DD] |
| 3 | `isrg-root-x1-cross-signed.der` | ISRG Root X1 (cross-signed) | https://letsencrypt.org/certificates/ | [YYYY-MM-DD] |
| 4 | `root-x2-by-x1.der` | ISRG Root X2 (cross-signed by X1) | https://letsencrypt.org/certificates/ | [YYYY-MM-DD] |
| 5 | `int-ye1.der` | Ye (Let's Encrypt) Intermediate | https://letsencrypt.org/certificates/ | [YYYY-MM-DD] |
| 6 | `root-ye.der` | Ye Root | https://letsencrypt.org/certificates/ | [YYYY-MM-DD] |
| 7 | `root-ye-by-x2.der` | Ye Root (cross-signed by X2) | https://letsencrypt.org/certificates/ | [YYYY-MM-DD] |
| 8 | `digicert_global_root_ca.der` | DigiCert Global Root CA | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 9 | `digicert_global_root_g2.der` | DigiCert Global Root G2 | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 10 | `DigiCertAssuredIDRootCA.der` | DigiCert Assured ID Root CA | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 11 | `DigiCertAssuredIDRootG2.der` | DigiCert Assured ID Root G2 | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 12 | `DigiCertAssuredIDRootG3.der` | DigiCert Assured ID Root G3 | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 13 | `DigiCertSMIMERSA4096RootG5.der` | DigiCert SMIME RSA 4096 Root G5 | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 14 | `DigiCertSMIMEECCP384RootG5.der` | DigiCert SMIME ECC P384 Root G5 | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 15 | `DigiCertClientRSA4096RootG5.der` | DigiCert Client RSA 4096 Root G5 | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 16 | `DigiCertClientECCP384RootG5.der` | DigiCert Client ECC P384 Root G5 | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 17 | `DigiCertCSRSA4096RootG5.der` | DigiCert C RSA 4096 Root G5 | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 18 | `DigiCertCSECCP384RootG5.der` | DigiCert C ECC P384 Root G5 | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 19 | `DigiCertTLSRSA4096RootG5.der` | DigiCert TLS RSA 4096 Root G5 | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 20 | `DigiCertTLSECCP384RootG5.der` | DigiCert TLS ECC P384 Root G5 | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 21 | `gts_root_r1.der` | GTS Root R1 (Google Trust Services) | https://pki.google.com/ | [YYYY-MM-DD] |
| 22 | `gts_root_r2.der` | GTS Root R2 (Google Trust Services) | https://pki.google.com/ | [YYYY-MM-DD] |
| 23 | `globalsign_root_r46.der` | GlobalSign Root R46 | https://www.globalsign.com/en/root-certificates | [YYYY-MM-DD] |
| 24 | `globalsign_ecc_root_ca_r4.der` | GlobalSign ECC Root CA R4 | https://www.globalsign.com/en/root-certificates | [YYYY-MM-DD] |
| 25 | `globalsign_ecc_root_ca_r5.der` | GlobalSign ECC Root CA R5 | https://www.globalsign.com/en/root-certificates | [YYYY-MM-DD] |
| 26 | `godaddy_root_g2.der` | GoDaddy Root G2 | https://certs.godaddy.com/repository | [YYYY-MM-DD] |
| 27 | `starfield_root_g2.der` | Starfield Root G2 | https://certs.godaddy.com/repository | [YYYY-MM-DD] |
| 28 | `BaltimoreCyberTrustRoot.der` | Baltimore CyberTrust Root | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 29 | `CybertrustGlobalRoot.der` | Cybertrust Global Root | https://www.digicert.com/kb/digicert-root-certificates.htm | [YYYY-MM-DD] |
| 30 | `identrust_commercial_root_ca_1.der` | IdenTrust Commercial Root CA 1 | https://www.identrust.com/certificates/root-certificates | [YYYY-MM-DD] |
| 31 | `identrust_public_sector_root_ca_1.der` | IdenTrust Public Sector Root CA 1 | https://www.identrust.com/certificates/root-certificates | [YYYY-MM-DD] |
| 32 | `microsoft_rsa_root_ca_2017.der` | Microsoft RSA Root CA 2017 | https://www.microsoft.com/en-us/pki/roots | [YYYY-MM-DD] |
| 33 | `microsoft_ecc_root_ca_2017.der` | Microsoft ECC Root CA 2017 | https://www.microsoft.com/en-us/pki/roots | [YYYY-MM-DD] |
| 34 | `quovadis_root_ca_1_g3.der` | QuoVadis Root CA 1 G3 | https://www.quovadisglobal.com/repository | [YYYY-MM-DD] |
| 35 | `quovadis_root_ca_2_g3.der` | QuoVadis Root CA 2 G3 | https://www.quovadisglobal.com/repository | [YYYY-MM-DD] |
| 36 | `quovadis_root_ca_3_g3.der` | QuoVadis Root CA 3 G3 | https://www.quovadisglobal.com/repository | [YYYY-MM-DD] |
| 37 | `certum_trusted_root_ca.der` | Certum Trusted Root CA | https://www.certum.pl/certificates/ | [YYYY-MM-DD] |
| 38 | `certum_trusted_network_ca.der` | Certum Trusted Network CA | https://www.certum.pl/certificates/ | [YYYY-MM-DD] |
| 39 | `buypass_class_2_root_ca.der` | Buypass Class 2 Root CA | https://www.buypass.com/certificates | [YYYY-MM-DD] |
| 40 | `buypass_class_3_root_ca.der` | Buypass Class 3 Root CA | https://www.buypass.com/certificates | [YYYY-MM-DD] |
| 41 | `actalis_authentication_root_ca.der` | Actalis Authentication Root CA | https://www.actalis.it/certificates | [YYYY-MM-DD] |
| 42 | `t_telsec_globalroot_class_3.der` | T-TeleSec GlobalRoot Class 3 | https://www.tele-trust.de/certificates | [YYYY-MM-DD] |
| 43 | `t_telsec_globalroot_class_2.der` | T-TeleSec GlobalRoot Class 2 | https://www.tele-trust.de/certificates | [YYYY-MM-DD] |
| 44 | `d_trust_root_class_3_ca_2_2009.der` | D-TRUST Root Class 3 CA 2 2009 | https://www.d-trust.net/certificates | [YYYY-MM-DD] |
| 45 | `d_trust_root_class_3_ca_2_ev_2009.der` | D-TRUST Root Class 3 CA 2 EV 2009 | https://www.d-trust.net/certificates | [YYYY-MM-DD] |
| 46 | `telia_root_ca_v2.der` | Telia Root CA v2 | https://www.telia.com/certificates | [YYYY-MM-DD] |
| 47 | `swisssign_rsa_tls_root_ca_2022_1.der` | SwissSign RSA TLS Root CA 2022-1 | https://www.swisssign.com/certificates | [YYYY-MM-DD] |
| 48 | `oiste_wisekey_global_root_gb_ca.der` | OISTE WISeKey Global Root GB CA | https://www.oiste.org/certificates | [YYYY-MM-DD] |
| 49 | `oiste_wisekey_global_root_gc_ca.der` | OISTE WISeKey Global Root GC CA | https://www.oiste.org/certificates | [YYYY-MM-DD] |
| 50 | `fnmt_rcm_ac_raiz.der` | FNMT-RCM AC RAIZ | https://www.fnmt.es/certificates | [YYYY-MM-DD] |
| 51 | `secom_security_communication_rootca2.der` | SECOM Security Communication RootCA2 | https://www.secom.co.jp/certificates | [YYYY-MM-DD] |
| 52 | `sslcom_tls_ecc_root_ca_2022.der` | SSL.com TLS ECC Root CA 2022 | https://www.ssl.com/certificates | [YYYY-MM-DD] |

## Notlar

- `[YYYY-MM-DD]` alanları, ilgili `.der` dosyasının resmi CA sayfasından indirildiği tarihiyle doldurulmalıdır.
- Her CA'nın kök sertifikası genelde "public domain" veya serbest dağıtılabilir niteliktedir; ancak CA'nın marka/logosu kullanımı için ticari marka kısıtlamaları olabilir. Bu dosya yalnızca sertifika dosyalarının kaynağını belgelemeyi amaçlar, marka kullanımına izin vermez.
- Yeni bir `.der` dosyası eklendiğinde bu dosya da güncellenmelidir.

## İlişkili Kod

Sertifikalar `src/drivers/crypto/x509.rs` dosyasında `include_bytes!` ile derleme zamanında gömülür:

```rust
pub const ISRG_ROOT_X1_DER: &[u8] = include_bytes!("roots/isrgrootx1.der");
// ...
```
