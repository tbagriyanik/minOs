# minOs Wiki – Hoş Geldiniz

minOs (Sarman Edition) v2.1.0, Rust ile geliştirilmiş, `no_std` ve `no_main` kullanan bağımsız bir 64-bit (`x86_64`) işletim sistemidir. Limine v8 önyükleyici ile UEFI ve BIOS ortamlarında çalışır.

> 📄 **Proje:** [GitHub Repository](https://github.com/tbagriyanik/minos)  
> 📄 **Dil:** [English](../en/)

---

## Hızlı Erişim

| Sayfa | İçerik |
| :--- | :--- |
| [Hakkında & Mimari](Hakkinda) | Tasarım felsefesi, `no_std` modeli, PMM ve IDT yapısı |
| [Kurulum ve Çalıştırma](Kurulum-ve-Calistirma) | `run.ps1` seçenekleri, Live USB oluşturma, diske kurulum |
| [Desteklenen Donanım Listesi](Desteklenen-Donanimlar) | Sürücü matrisi, ağ, grafik, depolama, giriş aygıtları |
| [Yerleşik 15 Uygulama](Uygulamalar) | Web Tarayıcısı, Not Defteri, Dosya Yöneticisi, vb. |
| [Ağ ve İnternet](Ag-ve-Internet) | TCP/IP, DNS (A/AAAA), HTTP/1.1, TLS 1.3 (HTTPS), HTTP/2 altyapısı |
| [Dosya Sistemleri](Dosya-Sistemleri) | VFS, FAT32, Ext2, Ext4, exFAT, ISO9660 |
| [Kullanıcı Rehberi](Kullanici-Rehberi) | Masaüstü kısayolları, MinShell komutları |
| [Geliştirici Rehberi](Gelistirici-Rehberi) | Kaynak haritası, birim testleri, derleme |
| [Sürüm Geçmişi](Surum-Gecmisi) | Sürüm notları ve gelecek planları |

---

## Durum Özeti (v2.1.0)

| Ölçüm | Değer |
| :--- | :--- |
| **Rust Derleyici** | 1.101.0-nightly |
| **Toplam Kod Satırı** | ~100.971 satır (Rust) |
| **Birim Testleri** | 195 geçti / 0 hata |
| **Güvenilen Kök CA** | 51 sertifika |
| **Yerleşik Uygulama** | 15 masaüstü uygulaması |
| **Çekirdek / ISO Boyutu** | ~2.0 MB / ~6.0 MB |

---

## Özellik Özetleri

- **TLS 1.3 HTTPS:** Tarayıcı `https://` bağlantılarını RFC 8446 uyumlu olarak tamamlar; IPv6 (AAAA) öncelikli, IPv4 geri düşüşlü bağlantı.
- **Tarayıcı Motoru:** Progresif (SOF2) JPEG çözme, `@font-face` kural ayrıştırma ve `@keyframes` renk/opaklık animasyonu.
- **Dinamik Tema:** 5 tema (Kadife Gece, Gün Işığı, Kutup Mavisi, Yeşil Terminal, Gün Batımı) canlı geçiş.
- **xHCI USB HID:** Yerel USB klavye/fare desteği, PS/2 fallback ile.
- **Ağ:** Intel e1000, I219-V, Realtek PCIe, Broadcom NetXtreme, VirtIO-net, VMware VMXNET3.
- **Depolama:** AHCI/SATA, NVMe, VirtIO-blk, USB Mass Storage, SD/MMC.
- **Dosya Sistemleri:** FAT32, Ext2, Ext4, exFAT (okuma/yazma/biçimlendirme).
- **Kripto:** SHA-256/384/512, HMAC, AES-GCM, RSA, ECDSA, X.509, CRL/OCSP.
- **Kurtarma Konsolu:** `recovery#` etkileşimli metin konsolu.
