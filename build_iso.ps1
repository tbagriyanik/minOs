# MinOS Boot Edilebilir ISO Olusturma Betiği (PowerShell)
param(
    [string]$IsoName = "minos.iso",
    [string]$OutputDir = "target\iso_root"
)

$OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$ErrorActionPreference = "Stop"

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "       MinOS ISO Olusturucu             " -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

# 1. ISO Kök Dizinini Hazirla
Write-Host "[1/4] ISO Dizin Yapisi Hazirlaniyor..." -ForegroundColor Yellow
if (Test-Path $OutputDir) {
    try {
        Remove-Item -Recurse -Force $OutputDir -ErrorAction SilentlyContinue
    } catch {}
}
New-Item -ItemType Directory -Force -Path "$OutputDir\boot\limine" | Out-Null
New-Item -ItemType Directory -Force -Path "$OutputDir\EFI\BOOT" | Out-Null

# 2. Bootloader ve Konfigürasyon Dosyalarini Kopyala
Write-Host "[2/4] Limine Onyukleyici Dosyalari Yerlestiriliyor..." -ForegroundColor Yellow
if (Test-Path "boot\limine.cfg") {
    Copy-Item -Force "boot\limine.cfg" -Destination "$OutputDir\boot\limine\limine.cfg"
    Copy-Item -Force "boot\limine.cfg" -Destination "$OutputDir\limine.cfg"
}
if (Test-Path "boot\limine.conf") {
    Copy-Item -Force "boot\limine.conf" -Destination "$OutputDir\boot\limine\limine.conf"
    Copy-Item -Force "boot\limine.conf" -Destination "$OutputDir\limine.conf"
}

if (Test-Path "boot\limine\limine-bios-cd.bin") {
    Copy-Item -Force "boot\limine\limine-bios-cd.bin" -Destination "$OutputDir\boot\limine\limine-bios-cd.bin"
}
if (Test-Path "boot\limine\limine-uefi-cd.bin") {
    Copy-Item -Force "boot\limine\limine-uefi-cd.bin" -Destination "$OutputDir\boot\limine\limine-uefi-cd.bin"
}
if (Test-Path "boot\limine\limine-bios.sys") {
    Copy-Item -Force "boot\limine\limine-bios.sys" -Destination "$OutputDir\boot\limine\limine-bios.sys"
    Copy-Item -Force "boot\limine\limine-bios.sys" -Destination "$OutputDir\limine-bios.sys"
}
if (Test-Path "boot\EFI\BOOT\BOOTX64.EFI") {
    Copy-Item -Force "boot\EFI\BOOT\BOOTX64.EFI" -Destination "$OutputDir\EFI\BOOT\BOOTX64.EFI"
}

# 3. Çekirdek İkilisini Derle / Kopyala
Write-Host "[3/4] MinOS ve Linux Cekirdekleri Paketleniyor..." -ForegroundColor Yellow
if (Test-Path "target\x86_64-minos\release\minos-kernel") {
    Copy-Item "target\x86_64-minos\release\minos-kernel" -Destination "$OutputDir\boot\minos-kernel"
} else {
    Write-Host "UYARI: minos-kernel henüz derlenmedi. Dummy çekirdek dosyasi yerlestiriliyor." -ForegroundColor Magenta
    Set-Content -Path "$OutputDir\boot\minos-kernel" -Value "MinOS Kernel Binary"
}

if (Test-Path "boot\linux\vmlinuz") {
    New-Item -ItemType Directory -Force -Path "$OutputDir\boot\linux" | Out-Null
    Copy-Item "boot\linux\vmlinuz" -Destination "$OutputDir\boot\linux\vmlinuz"
    if (Test-Path "boot\linux\initramfs.img") {
        Copy-Item "boot\linux\initramfs.img" -Destination "$OutputDir\boot\linux\initramfs.img"
    }
    if (Test-Path "boot\linux\modloop-virt") {
        Copy-Item "boot\linux\modloop-virt" -Destination "$OutputDir\boot\linux\modloop-virt"
        Copy-Item "boot\linux\modloop-virt" -Destination "$OutputDir\boot\modloop-virt"
    }
    if (Test-Path "boot\linux\localhost.apkovl.tar.gz") {
        Copy-Item "boot\linux\localhost.apkovl.tar.gz" -Destination "$OutputDir\localhost.apkovl.tar.gz"
        Copy-Item "boot\linux\localhost.apkovl.tar.gz" -Destination "$OutputDir\boot\linux\localhost.apkovl.tar.gz"
    }
    # Alpine boot media mount etiketleri
    Set-Content -Path "$OutputDir\.alpine-release" -Value "3.20.0"
}

# Limine Boot Information Table Yamalayici (C#)
$patcherCode = @"
using System;
using System.IO;

public static class LimineIsoPatcher {
    public static bool Patch(string isoPath, string bootBinPath) {
        try {
            byte[] bytes = File.ReadAllBytes(isoPath);
            byte[] bootBytes = File.ReadAllBytes(bootBinPath);

            int catalogLba = -1;
            for (int lba = 16; lba < 32; lba++) {
                int offset = lba * 2048;
                if (bytes[offset] == 0 &&
                    bytes[offset + 1] == 'C' && bytes[offset + 2] == 'D' &&
                    bytes[offset + 3] == '0' && bytes[offset + 4] == '0' && bytes[offset + 5] == '1') {
                    catalogLba = BitConverter.ToInt32(bytes, offset + 71);
                    break;
                }
            }

            if (catalogLba <= 0) return false;

            int entryOffset = (catalogLba * 2048) + 32;
            int bootLba = BitConverter.ToInt32(bytes, entryOffset + 8);
            int bootOffset = bootLba * 2048;

            uint csum = 0;
            for (int i = 64; i < bootBytes.Length; i += 4) {
                csum += BitConverter.ToUInt32(bootBytes, i);
            }

            Array.Copy(BitConverter.GetBytes((uint)16), 0, bytes, bootOffset + 8, 4);
            Array.Copy(BitConverter.GetBytes((uint)bootLba), 0, bytes, bootOffset + 12, 4);
            Array.Copy(BitConverter.GetBytes((uint)bootBytes.Length), 0, bytes, bootOffset + 16, 4);
            Array.Copy(BitConverter.GetBytes(csum), 0, bytes, bootOffset + 20, 4);
            Array.Clear(bytes, bootOffset + 24, 40);

            File.WriteAllBytes(isoPath, bytes);
            return true;
        } catch {
            return false;
        }
    }
}
"@

if (-not ([System.Management.Automation.PSTypeName]'LimineIsoPatcher').Type) {
    Add-Type -TypeDefinition $patcherCode
}

# 4. ISO Üretimi Komutu (xorriso / oscdimg / mkisofs / Native IMAPI2FS)
Write-Host "[4/4] ISO Imaji Uretiliyor..." -ForegroundColor Yellow
$oscdimg = Get-Command "oscdimg.exe" -ErrorAction SilentlyContinue
$xorriso = Get-Command "xorriso.exe" -ErrorAction SilentlyContinue
$mkisofs = Get-Command "mkisofs.exe" -ErrorAction SilentlyContinue

if (-not $xorriso -and (Test-Path "tools\xorriso.exe")) { $xorriso = (Get-Item "tools\xorriso.exe").FullName }
if (-not $oscdimg -and (Test-Path "tools\oscdimg.exe")) { $oscdimg = (Get-Item "tools\oscdimg.exe").FullName }
if (-not $mkisofs -and (Test-Path "tools\mkisofs.exe")) { $mkisofs = (Get-Item "tools\mkisofs.exe").FullName }

if ($oscdimg) {
    $tempIso = "target\temp_build.iso"
    if (Test-Path $tempIso) { Remove-Item -Force $tempIso }
    $bootBin = "$OutputDir\boot\limine\limine-bios-cd.bin"
    if (Test-Path "$OutputDir\boot\limine\limine-uefi-cd.bin") {
        & $oscdimg -m -o -n -d "-bootdata:2#p0,e,b$bootBin#pEF,e,b$OutputDir\boot\limine\limine-uefi-cd.bin" $OutputDir $tempIso
    } else {
        & $oscdimg -m -o -n -d -b"$bootBin" $OutputDir $tempIso
    }
    if (Test-Path $bootBin) {
        $patched = [LimineIsoPatcher]::Patch((Resolve-Path $tempIso).Path, (Resolve-Path $bootBin).Path)
        if ($patched) {
            Write-Host "[BILGI] Limine El Torito Boot Information Table basariyla yamalandi." -ForegroundColor Cyan
        }
    }
    try {
        Copy-Item -Force $tempIso $IsoName
    } catch {
        Copy-Item -Force $tempIso "minos_boot.iso"
        $IsoName = "minos_boot.iso"
    }

    # Release dizinlerine de kopyala
    New-Item -ItemType Directory -Force -Path "target\release" | Out-Null
    New-Item -ItemType Directory -Force -Path "target\x86_64-minos\release" | Out-Null
    Copy-Item -Force $IsoName "target\release\minos.iso"
    Copy-Item -Force $IsoName "target\x86_64-minos\release\minos.iso"

    Write-Host "[BASARILI] $IsoName oscdimg & Limine ile boot edilebilir olusturuldu!" -ForegroundColor Green
} elseif ($xorriso) {
    if (Test-Path "$OutputDir\EFI\BOOT\BOOTX64.EFI") {
        & $xorriso -as mkisofs -b boot/limine/limine-bios-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table --efi-boot EFI/BOOT/BOOTX64.EFI -efi-boot-part --efi-boot-image --protective-msdos-label -o $IsoName $OutputDir
    } else {
        & $xorriso -as mkisofs -b boot/limine/limine-bios-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table -o $IsoName $OutputDir
    }

    if (Test-Path "tools\limine.exe") {
        & "tools\limine.exe" bios-install $IsoName
    }

    Write-Host "[BASARILI] $IsoName xorriso & Limine ile boot edilebilir olusturuldu!" -ForegroundColor Green
} elseif ($mkisofs) {
    & $mkisofs -b boot/limine/limine-bios-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table -o $IsoName $OutputDir
    if (Test-Path "tools\limine.exe") {
        & "tools\limine.exe" bios-install $IsoName
    }
    Write-Host "[BASARILI] $IsoName mkisofs ile olusturuldu!" -ForegroundColor Green
} else {
    Write-Host "Bilgi: xorriso/oscdimg bulunamadi. Yerel Windows IMAPI2FS bileseni kullaniliyor..." -ForegroundColor Cyan
    try {
        $absSource = (Get-Item $OutputDir).FullName
        $absIso = [System.IO.Path]::GetFullPath($IsoName)

        if (Test-Path $absIso) {
            Remove-Item -Force $absIso
        }

        $csCode = @"
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;

public class MinOsIsoWriter {
    public static void SaveIStream(object streamObj, string filePath) {
        IStream stream = (IStream)streamObj;
        using (FileStream fs = new FileStream(filePath, FileMode.Create, FileAccess.Write)) {
            byte[] buffer = new byte[65536];
            IntPtr pcbRead = Marshal.AllocHGlobal(sizeof(int));
            try {
                do {
                    stream.Read(buffer, buffer.Length, pcbRead);
                    int bytesRead = Marshal.ReadInt32(pcbRead);
                    if (bytesRead <= 0) break;
                    fs.Write(buffer, 0, bytesRead);
                } while (true);
            } finally {
                Marshal.FreeHGlobal(pcbRead);
            }
        }
    }
}
"@

        if (-not ([System.Management.Automation.PSTypeName]'MinOsIsoWriter').Type) {
            Add-Type -TypeDefinition $csCode -Language CSharp
        }

        $fsi = New-Object -ComObject IMAPI2FS.MsftFileSystemImage
        $fsi.FreeMediaBlocks = -1
        $fsi.FileSystemsToCreate = 3 # ISO9660 + Joliet
        $fsi.VolumeName = "MINOS"

        $fsi.Root.AddTree($absSource, $false)

        $result = $fsi.CreateResultImage()
        $stream = $result.ImageStream

        [MinOsIsoWriter]::SaveIStream($stream, $absIso)
        Write-Host "[BASARILI] $IsoName Windows IMAPI2FS ile otomatik olusturuldu!" -ForegroundColor Green
    } catch {
        Write-Host "UYARI: ISO otomatik uretilemedi. ISO dizini '$OutputDir' icinde hazirlandi." -ForegroundColor Magenta
        Write-Host "Hata: $_" -ForegroundColor Red
    }
}

Write-Host "`nIslem Tamamlandi!" -ForegroundColor Green
Write-Host "ISO Dosyasi: $(Resolve-Path $IsoName)" -ForegroundColor Green
Write-Host "ISO Kok Dizini: $(Resolve-Path $OutputDir)" -ForegroundColor Gray

