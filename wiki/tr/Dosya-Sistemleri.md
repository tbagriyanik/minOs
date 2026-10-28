# Dosya Sistemleri

minOs VFS katmanı farklı dosya sistemlerini destekler; ancak her dosya sisteminin yetenek düzeyi farklıdır.

---

## Dosya Sistemi Karşılaştırması

| Dosya Sistemi | Destek Düzeyi | Açıklama |
| :--- | :--- | :--- |
| **FAT32** | **Tam Okuma / Yazma / Biçimlendirme / Kalıcılık** | MBR/GPT disklerde tam dosya okuma, yazma, oluşturma, silme; `settings.cfg` ve `VFS.DAT` sistem kalıcılığı. |
| **Ext2** | **Okuma / Yazma / Biçimlendirme** | `src/fs/ext2.rs` üzerinden ATA disk bölümlerine mount etme, dizin listeleme ve dosya okuma/yazma. |
| **Ext4** | **Okuma / Yazma / Biçimlendirme** | `src/fs/ext4.rs` (biçimlendirme) + `src/fs/ext4_rw.rs` (okuma/yazma) ile 1024 baytlık blok, 256 baytlık inode ve çoklu blok grubu desteklenir. Blok eşleme ext2 uyumludur (direct + single/double indirect); **extents ağacı, journal (JBD2), htree (dir_index) ve inline data desteklenmez** — bu özellikleri etkin birimler mount edilmez. Linux `ext4` sürücüsü birimi sorunsuz bağlar. |
| **exFAT** | **Okuma / Yazma / Biçimlendirme** | `src/fs/exfat.rs` (biçimlendirme: VBR, FAT, allocation bitmap, up-case tablosu) + `src/fs/exfat_rw.rs` (okuma/yazma: kök/alt dizin, dosya oluşturma/silme, FAT zincirli çok kümeli dosyalar, Unicode adlar). Long-name girdilerinin tam doğrulaması ve timestamp/ACL yazımı henüz yoktur. |
| **ISO9660** | **Salt Okunur (Read-only)** | Canlı CD/DVD ve kurulum medyası içeriğini okuma desteği. |

---

## VFS (Sanal Dosya Sistemi)

- `src/fs/vfs.rs` — Sanal dosya sistemi arayüzü
- `src/fs/diskfs.rs` — Disk okuyucuları (FAT32 + ISO9660)
- `src/fs/part.rs` — MBR/GPT bölümleme
- `src/fs/block.rs` — Blok cihaz soyutlama

---

## Kalıcılık (Persistence)

- `settings.cfg` — Sistem ayarları kalıcı depolama
- `VFS.DAT` — VFS ağacı anlık görüntüsü
- `MINOS.SYS` — Kurulum işaret dosyası
- `BENIOKU.TXT` — Okuma dosyası

---

## Disk Biçimlendirme (MinShell)

| Komut | Kullanım | Açıklama |
| :--- | :--- | :--- |
| `format` | `format <fat32\|ext4\|exfat\|ext2> <0..3>` | Bağlı ATA sabit diski belirtilen dosya sisteminde biçimlendirir |
