# minOs Boot Edilebilir ISO Olusturma Betiği (PowerShell)
param(
    [string]$IsoName = "minos.iso",
    [string]$OutputDir = "target\iso_root"
)

$OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$ErrorActionPreference = "Stop"

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "       minOs ISO Olusturucu             " -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

# 1. ISO Kök Dizinini Hazirla
Write-Host "[1/4] ISO Dizin Yapisi Hazirlaniyor..." -ForegroundColor Yellow
if (Test-Path $OutputDir) {
    try {
        Remove-Item -Recurse -Force $OutputDir -ErrorAction SilentlyContinue
    } catch {}
}
New-Item -ItemType Directory -Force -Path "$OutputDir\boot\limine" | Out-Null
New-Item -ItemType Directory -Force -Path "$OutputDir\limine" | Out-Null
New-Item -ItemType Directory -Force -Path "$OutputDir\EFI\BOOT" | Out-Null

# 2. Bootloader ve Konfigürasyon Dosyalarini Kopyala
Write-Host "[2/4] Limine Onyukleyici Dosyalari Yerlestiriliyor..." -ForegroundColor Yellow
if (Test-Path "boot\limine.conf") {
    Copy-Item -Force "boot\limine.conf" -Destination "$OutputDir\boot\limine\limine.conf"
    # Limine searches the ISO root first on CD/El Torito boots. Keep the
    # config in the boot directory too for compatibility with older layouts.
    Copy-Item -Force "boot\limine.conf" -Destination "$OutputDir\limine.conf"
    Copy-Item -Force "boot\limine.conf" -Destination "$OutputDir\limine\limine.conf"
    Copy-Item -Force "boot\limine.conf" -Destination "$OutputDir\EFI\BOOT\limine.conf"
}
# Copy background image for Limine
if (Test-Path "boot\cat_small.png") {
    Copy-Item -Force "boot\cat_small.png" -Destination "$OutputDir\boot\cat_small.png"
}


if (Test-Path "boot\limine\limine-bios-cd.bin") {
    Copy-Item -Force "boot\limine\limine-bios-cd.bin" -Destination "$OutputDir\boot\limine\limine-bios-cd.bin"
}

if (Test-Path "boot\limine\limine-bios.sys") {
    Copy-Item -Force "boot\limine\limine-bios.sys" -Destination "$OutputDir\boot\limine\limine-bios.sys"
}

# Limine UEFI El Torito imajı ve EFI ikilileri
if (Test-Path "boot\limine\limine-uefi-cd.bin") {
    Copy-Item -Force "boot\limine\limine-uefi-cd.bin" -Destination "$OutputDir\boot\limine\limine-uefi-cd.bin"
    Write-Host "UEFI El Torito imaji: boot\limine\limine-uefi-cd.bin" -ForegroundColor Cyan
}

if (Test-Path "boot\limine\BOOTX64.EFI") {
    Copy-Item -Force "boot\limine\BOOTX64.EFI" -Destination "$OutputDir\EFI\BOOT\BOOTX64.EFI"
    Write-Host "UEFI x86_64 ikilisi: EFI\BOOT\BOOTX64.EFI" -ForegroundColor Cyan
}
if (Test-Path "boot\limine\BOOTIA32.EFI") {
    Copy-Item -Force "boot\limine\BOOTIA32.EFI" -Destination "$OutputDir\EFI\BOOT\BOOTIA32.EFI"
}

# 3. Çekirdek İkilisini Derle / Kopyala
Write-Host "[3/4] minOs Cekirdegi Paketleniyor..." -ForegroundColor Yellow
$kernelCandidates = @(
    (Get-Item "target\x86_64-minos\release\minos-kernel" -ErrorAction SilentlyContinue),
    (Get-Item "target\x86_64-unknown-none\release\minos-kernel" -ErrorAction SilentlyContinue)
) | Where-Object { $_ -ne $null } | Sort-Object LastWriteTime -Descending

if ($kernelCandidates.Count -gt 0) {
    $selectedKernel = $kernelCandidates[0].FullName
    Write-Host "Kullanilan cekirdek: $selectedKernel" -ForegroundColor Cyan
    Copy-Item $selectedKernel -Destination "$OutputDir\boot\minos-kernel"
} else {
    Write-Host "UYARI: minos-kernel henüz derlenmedi. Dummy Cekirdegi dosyasi yerlestiriliyor." -ForegroundColor Magenta
    Set-Content -Path "$OutputDir\boot\minos-kernel" -Value "minOs Kernel Binary"
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

            // El Torito Boot Record Volume Descriptor, Volume Descriptor
            // Sequence icinde type=0 (Boot Record) olarak yer alir ve
            // genellikle 17. sektordedir. Boot catalog LBA'si bu
            // descriptor'in 71..74 byte'larinda LITTLE-ENDIAN tutulur.
            //
            // UYARI: PVD (type=1) icindeki ayni offset El Torito boot
            // catalog LBA'si DEGILDIR; oradan okumak ISO'yu bozar.
            int bootRecordOffset = -1;
            for (int lba = 16; lba < 32; lba++) {
                int offset = lba * 2048;
                bool isIsoMagic =
                    bytes[offset + 1] == 'C' && bytes[offset + 2] == 'D' &&
                    bytes[offset + 3] == '0' && bytes[offset + 4] == '0' &&
                    bytes[offset + 5] == '1';
                if (isIsoMagic && bytes[offset] == 0) { // 0 = Boot Record descriptor
                    bootRecordOffset = offset;
                    break;
                }
            }

            if (bootRecordOffset < 0) return false;

            int catalogLba = BitConverter.ToInt32(bytes, bootRecordOffset + 71);

            if (catalogLba <= 0 || (catalogLba * 2048) + 64 > bytes.Length) return false;

            // Boot Catalog Validation Entry (32 bayt) sonrasinda ilk boot kaydi gelir.
            int entryOffset = (catalogLba * 2048) + 32;
            int bootLba = BitConverter.ToInt32(bytes, entryOffset + 8);

            // Sinir kontrolu: yamalanacak sektor ISO'nun icinde olmali.
            if (bootLba <= 0 || (bootLba * 2048) + 64 > bytes.Length) return false;

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

# Limine'nin fiziksel USB/UEFI ile uyumlu hybrid ISO tarifi xorriso kullanir.
# xorriso mevcutsa oscdimg yolunu bilincli olarak devre disi birak.
if ($xorriso) {
    $oscdimg = $null
    $mkisofs = $null
} elseif ($oscdimg) {
    Write-Warning "xorriso bulunamadi; oscdimg yalnizca sanal makine testleri icin kullaniliyor. Fiziksel USB icin xorriso kurulmasi onerilir."
}

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
        Write-Host "[HATA] $IsoName dosyasina kopyalanamadi: $_" -ForegroundColor Red
        Write-Host "Dosya baskida olabilir, QEMU'yu kapatip tekrar deneyin." -ForegroundColor Yellow
        throw
    }

    if (Test-Path "tools\limine.exe") {
        & "tools\limine.exe" bios-install $IsoName
    }

    # Release dizinlerine de kopyala
    New-Item -ItemType Directory -Force -Path "target\release" | Out-Null
    New-Item -ItemType Directory -Force -Path "target\x86_64-minos\release" | Out-Null
    Copy-Item -Force $IsoName "target\release\minos.iso"
    Copy-Item -Force $IsoName "target\x86_64-minos\release\minos.iso"

    Write-Host "[BASARILI] $IsoName oscdimg & Limine ile boot edilebilir olusturuldu!" -ForegroundColor Green
} elseif ($xorriso) {
    if (Test-Path "$OutputDir\boot\limine\limine-uefi-cd.bin") {
        # Limine'nin UEFI El Torito imajini kullan. BOOTX64.EFI dosyasi
        # aramak yerine UEFI boot imaji dogrudan ISO'ya eklenmelidir.
        & $xorriso -as mkisofs -R -r -J -hfsplus -apm-block-size 2048 -b boot/limine/limine-bios-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table --efi-boot boot/limine/limine-uefi-cd.bin -efi-boot-part --efi-boot-image --protective-msdos-label -o $IsoName $OutputDir
    } else {
        & $xorriso -as mkisofs -R -r -J -hfsplus -apm-block-size 2048 -b boot/limine/limine-bios-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table -o $IsoName $OutputDir
    }

    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $IsoName)) {
        throw "xorriso ISO uretimi basarisiz oldu (exit code: $LASTEXITCODE)."
    }

    if (Test-Path "tools\limine.exe") {
        & "tools\limine.exe" bios-install $IsoName
    }

    Write-Host "[BASARILI] $IsoName xorriso & Limine ile boot edilebilir olusturuldu!" -ForegroundColor Green
} elseif ($mkisofs) {
    $mkisofsArgs = @('-b', 'boot/limine/limine-bios-cd.bin', '-no-emul-boot', '-boot-load-size', '4', '-boot-info-table')
    if (Test-Path "$OutputDir\boot\limine\limine-uefi-cd.bin") {
        $mkisofsArgs += @('--efi-boot', 'boot/limine/limine-uefi-cd.bin', '-efi-boot-part', '-efi-boot-image', '--protective-msdos-label')
    }
    $mkisofsArgs += @('-o', $IsoName, $OutputDir)
    & $mkisofs @mkisofsArgs
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

