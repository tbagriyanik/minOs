# minOs Boot Edilebilir ISO Oluşturma Rehberi

minOs'u hem **UEFI** hem de **Legacy BIOS** sistemlerde boot edilebilir tek bir `.iso` dosyası haline getirmek için aşağıdaki 3 adımı uygulayabilirsiniz.

---

## Yöntem 1: Otomatik PowerShell Betiği ile ISO Oluşturma

Projeye eklenen [`build_iso.ps1`](file:///f:/minos/build_iso.ps1) betiği gerekli dizin yapısını (`target/iso_root`) ve Limine bootloader dosyalarını otomatik hazırlar.

PowerShell üzerinde çalıştırmak için:
```powershell
.\build_iso.ps1
```

---

## Yöntem 2: Limine & xorriso ile Standart ISO Paketleme

Modern x86_64 işletim sistemlerinde en yaygın kullanılan ISO üretim adımları şunlardır:

### 1. ISO Dizin Hiyerarşisini Oluşturun:
```
iso_root/
├── boot/
│   ├── limine/
│   │   ├── limine.cfg              # Bootloader ayar dosyası
│   │   ├── limine-bios.sys         # BIOS bootloader ikilisi
│   │   └── limine-bios-cd.bin      # CD/ISO El Torito önyükleme kaydı
│   └── minos-kernel                # Derlenen minOs çekirdeğiniz
└── EFI/
    └── BOOT/
        └── BOOTX64.EFI             # UEFI 64-bit önyükleyici
```

### 2. ISO İmajını Paketleme Komutu:
```bash
xorriso -as mkisofs \
    -b boot/limine/limine-bios-cd.bin \
    -no-emul-boot -boot-load-size 4 -boot-info-table \
    --efi-boot EFI/BOOT/BOOTX64.EFI \
    -efi-boot-part --efi-boot-image --protective-msdos-label \
    -o minos.iso target/iso_root
```

### 3. Limine BIOS Sektörlerini ISO'ya Yazma:
```bash
limine bios-install minos.iso
```

---

## Yöntem 3: QEMU veya VirtualBox ile Test Etme

Oluşan `minos.iso` dosyasını test etmek için:

```powershell
# QEMU ile doğrudan test:
qemu-system-x86_64 -cdrom minos.iso -m 512M -vga std `
  -audiodev driver=sdl,id=snd0 -device intel-hda -device hda-output,audiodev=snd0
```

VirtualBox'ta VM Ayarları > Ses bölümünde sesi etkinleştirip **Intel HD Audio** denetleyicisini seçin.

- **Gerçek Donanımda Çalıştırma:** Oluşturduğunuz `minos.iso` dosyasını **Rufus** veya **Ventoy** kullanarak bir USB belleğe yazdırıp doğrudan bilgisayarınızı minOs ile başlatabilirsiniz.
