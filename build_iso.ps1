# MinOS Boot Edilebilir ISO Oluşturma Betiği (PowerShell)
param(
    [string]$IsoName = "minos.iso",
    [string]$OutputDir = "target\iso_root"
)

$ErrorActionPreference = "Stop"

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "       MinOS ISO Oluşturucu             " -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

# 1. ISO Kök Dizinini Hazırla
Write-Host "[1/4] ISO Dizin Yapısı Hazırlanıyor..." -ForegroundColor Yellow
if (Test-Path $OutputDir) {
    try {
        Remove-Item -Recurse -Force $OutputDir -ErrorAction SilentlyContinue
    } catch {}
}
New-Item -ItemType Directory -Force -Path "$OutputDir\boot\limine" | Out-Null
New-Item -ItemType Directory -Force -Path "$OutputDir\EFI\BOOT" | Out-Null

# 2. Bootloader ve Konfigürasyon Dosyalarını Kopyala
Write-Host "[2/4] Limine Önyükleyici Dosyaları Yerleştiriliyor..." -ForegroundColor Yellow
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
Write-Host "[3/4] MinOS Çekirdeği Paketleniyor..." -ForegroundColor Yellow
if (Test-Path "target\x86_64-minos\release\minos-kernel") {
    Copy-Item "target\x86_64-minos\release\minos-kernel" -Destination "$OutputDir\boot\minos-kernel"
} else {
    Write-Host "UYARI: minos-kernel henüz derlenmedi. Dummy çekirdek dosyası yerleştiriliyor." -ForegroundColor Magenta
    Set-Content -Path "$OutputDir\boot\minos-kernel" -Value "MinOS Kernel Binary"
}

# Limine Boot Information Table Yamalayıcı (C#)
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
Write-Host "[4/4] ISO İmajı Üretiliyor..." -ForegroundColor Yellow
$oscdimg = Get-Command "oscdimg.exe" -ErrorAction SilentlyContinue
$xorriso = Get-Command "xorriso.exe" -ErrorAction SilentlyContinue
$mkisofs = Get-Command "mkisofs.exe" -ErrorAction SilentlyContinue

if (-not $xorriso -and (Test-Path "tools\xorriso.exe")) { $xorriso = (Get-Item "tools\xorriso.exe").FullName }
if (-not $oscdimg -and (Test-Path "tools\oscdimg.exe")) { $oscdimg = (Get-Item "tools\oscdimg.exe").FullName }
if (-not $mkisofs -and (Test-Path "tools\mkisofs.exe")) { $mkisofs = (Get-Item "tools\mkisofs.exe").FullName }

if ($oscdimg) {
    $bootBin = "$OutputDir\boot\limine\limine-bios-cd.bin"
    if (Test-Path "$OutputDir\boot\limine\limine-uefi-cd.bin") {
        & $oscdimg -m -o -n -d "-bootdata:2#p0,e,b$bootBin#pEF,e,b$OutputDir\boot\limine\limine-uefi-cd.bin" $OutputDir $IsoName
    } else {
        & $oscdimg -m -o -n -d -b"$bootBin" $OutputDir $IsoName
    }
    if (Test-Path $bootBin) {
        $patched = [LimineIsoPatcher]::Patch((Resolve-Path $IsoName).Path, (Resolve-Path $bootBin).Path)
        if ($patched) {
            Write-Host "[BİLGİ] Limine El Torito Boot Information Table başarıyla yamalandı." -ForegroundColor Cyan
        }
    }
    Write-Host "[BAŞARILI] $IsoName oscdimg & Limine ile boot edilebilir oluşturuldu!" -ForegroundColor Green
} elseif ($xorriso) {
    if (Test-Path "$OutputDir\EFI\BOOT\BOOTX64.EFI") {
        & $xorriso -as mkisofs -b boot/limine/limine-bios-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table --efi-boot EFI/BOOT/BOOTX64.EFI -efi-boot-part --efi-boot-image --protective-msdos-label -o $IsoName $OutputDir
    } else {
        & $xorriso -as mkisofs -b boot/limine/limine-bios-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table -o $IsoName $OutputDir
    }

    if (Test-Path "tools\limine.exe") {
        & "tools\limine.exe" bios-install $IsoName
    }

    Write-Host "[BAŞARILI] $IsoName xorriso & Limine ile boot edilebilir oluşturuldu!" -ForegroundColor Green
} elseif ($mkisofs) {
    & $mkisofs -b boot/limine/limine-bios-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table -o $IsoName $OutputDir
    if (Test-Path "tools\limine.exe") {
        & "tools\limine.exe" bios-install $IsoName
    }
    Write-Host "[BAŞARILI] $IsoName mkisofs ile oluşturuldu!" -ForegroundColor Green
} else {
    Write-Host "Bilgi: xorriso/oscdimg bulunamadı. Yerel Windows IMAPI2FS bileşeni kullanılıyor..." -ForegroundColor Cyan
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
        Write-Host "[BAŞARILI] $IsoName Windows IMAPI2FS ile otomatik oluşturuldu!" -ForegroundColor Green
    } catch {
        Write-Host "UYARI: ISO otomatik üretilemedi. ISO dizini '$OutputDir' içinde hazırlandı." -ForegroundColor Magenta
        Write-Host "Hata: $_" -ForegroundColor Red
    }
}

Write-Host "`nİşlem Tamamlandı!" -ForegroundColor Green
Write-Host "ISO Dosyası: $(Resolve-Path $IsoName)" -ForegroundColor Green
Write-Host "ISO Kök Dizini: $(Resolve-Path $OutputDir)" -ForegroundColor Gray

