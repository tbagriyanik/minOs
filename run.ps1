param(
    [switch]$BuildOnly,
    [switch]$Debug,
    [switch]$UsbDisks,
    [switch]$UsbDiskReadWrite,
    [switch]$UsbHid,
    [ValidateRange(1, 64)][int]$CpuCount = 2,
    [ValidatePattern('^[1-9][0-9]*(M|G)$')][string]$Memory = "512M",
    [string]$DiskImage = ".\disk.qcow2",
    [switch]$Live,
    [switch]$NoIso,
    [switch]$ReprepareDisk,
    [ValidateSet("default", "sdl", "gtk")][string]$Display = "default"
)

# minOs Derle ve QEMU ile Çalistir (PowerShell)
chcp 65001 | Out-Null
$PSDefaultParameterValues['Out-File:Encoding'] = 'utf8'; $PSDefaultParameterValues['Set-Content:Encoding'] = 'utf8';
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$ErrorActionPreference = "Stop"

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "       minOs Derleyici & Baslatici       " -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

# 1. QEMU Yolunu Bul
$qemuCandidates = @(
    "qemu-system-x86_64.exe",
    "C:\Program Files\qemu\qemu-system-x86_64.exe",
    "C:\Program Files (x86)\qemu\qemu-system-x86_64.exe"
)

$qemuExe = $null
foreach ($path in $qemuCandidates) {
    if (Get-Command $path -ErrorAction SilentlyContinue) {
        $qemuExe = $path
        break
    } elseif (Test-Path $path) {
        $qemuExe = $path
        break
    }
}

if (-not $qemuExe) {
    Write-Host "[HATA] QEMU bulunamadi! Lütfen QEMU'nun kurulu olduğundan emin olun." -ForegroundColor Red
    exit 1
}

Write-Host "[1/3] QEMU Tespit Edildi: $qemuExe" -ForegroundColor Green

# 2. Kernel'i Derle
Write-Host "[2/3] minOs Cekirdegi Derleniyor (cargo build)..." -ForegroundColor Yellow
$cargoArgs = @("build", "-Zjson-target-spec", "--target", "x86_64-minos.json", "-Zbuild-std=core,alloc", "-Zbuild-std-features=compiler-builtins-mem")
if (-not $Debug) {
    $cargoArgs += "--release"
}

try {
    & cargo $cargoArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[HATA] Derleme basarisiz oldu." -ForegroundColor Red
        exit $LASTEXITCODE
    }
} catch {
    Write-Host "Derleme sirasinda standart kütüphane derleme bayraklari denetleniyor..." -ForegroundColor Yellow
    & cargo build -Zjson-target-spec --target x86_64-minos.json --release
}

$kernelBin = if ($Debug) { "target\x86_64-minos\debug\minos-kernel" } else { "target\x86_64-minos\release\minos-kernel" }

if (-not (Test-Path $kernelBin)) {
    # Alternatif hedef dosya adi kontrolü
    $kernelBin = if ($Debug) { "target\x86_64-minos\debug\minos" } else { "target\x86_64-minos\release\minos" }
}

if ($BuildOnly) {
    Write-Host "[BASARILI] Derleme tamamlandi: $kernelBin" -ForegroundColor Green
    exit 0
}

# 3. QEMU İle Baslat
Write-Host "[3/3] QEMU Baslatiliyor..." -ForegroundColor Yellow
Write-Host "=========================================" -ForegroundColor Cyan

$qemuArgs = @(
    "-m", $Memory,
    "-smp", "$CpuCount",
    "-serial", "stdio",
    "-vga", "std",
    "-global", "VGA.vgamem_mb=64",
    # SDL ses çıkışı DirectSound'un zorunlu capture/mikrofon açma hatasını önler.
    "-audiodev", "driver=sdl,id=snd0",
    "-device", "intel-hda",
    # Yalnızca çıkış codec'i: DirectSoundCapture / mikrofon başlatılmaz.
    "-device", "hda-output,audiodev=snd0",
    # --- Ağ: e1000 + QEMU user modu (SLIRP) — ARP/ping/TCP sağlar ---
    "-netdev", "user,id=n0,hostfwd=tcp::8080-:80",
    "-device", "e1000,netdev=n0"
)

if ($Display -eq "sdl") {
    $qemuArgs += @("-display", "sdl")
} elseif ($Display -eq "gtk") {
    $qemuArgs += @("-display", "gtk")
}

if ($DiskImage) {
    if (-not (Test-Path -LiteralPath $DiskImage -PathType Leaf)) {
        Write-Host "[BILGI] Bos ve formatsiz disk imaji olusturuluyor: $DiskImage (256MB)..." -ForegroundColor Yellow
        $qemuImgCandidates = @(
            "C:\Program Files\qemu\qemu-img.exe",
            (Join-Path (Split-Path $qemuExe -Parent) "qemu-img.exe"),
            "qemu-img.exe"
        )
        $qemuImgExe = $null
        foreach ($p in $qemuImgCandidates) {
            if (Test-Path $p) { $qemuImgExe = $p; break }
            elseif (Get-Command $p -ErrorAction SilentlyContinue) { $qemuImgExe = $p; break }
        }
        if ($qemuImgExe) {
            & $qemuImgExe create -f qcow2 $DiskImage 256M | Out-Null
            Write-Host "[BASARILI] Formatsiz bos disk eklendi: $DiskImage" -ForegroundColor Green
        } else {
            Write-Host "[UYARI] qemu-img bulunamadi, disk olusturulamadi." -ForegroundColor DarkYellow
        }
    }
    if (Test-Path -LiteralPath $DiskImage -PathType Leaf) {
        if ($ReprepareDisk) {
            Write-Host "[BILGI] ReprepareDisk aktif; build_disk.ps1 calistiriliyor..." -ForegroundColor Yellow
            & powershell -ExecutionPolicy Bypass -File ".\build_disk.ps1" -DiskImage $DiskImage -Force
        }
        $diskFormat = "raw"
        try {
            $hdr = [System.IO.File]::ReadAllBytes((Resolve-Path $DiskImage).Path)[0..3]
            if ($hdr[0] -eq 0x51 -and $hdr[1] -eq 0x46 -and $hdr[2] -eq 0x49 -and $hdr[3] -eq 0xFB) {
                $diskFormat = "qcow2"
            }
        } catch {}
        $qemuArgs += @("-drive", "file=$DiskImage,format=$diskFormat,if=ide")
    }
}

if ($UsbDisks -or $UsbDiskReadWrite) {
    try {
        $usbDisks = Get-Disk | Where-Object {
            $_.BusType -eq "USB" -and -not $_.IsBoot -and -not $_.IsSystem
        }

        foreach ($disk in $usbDisks) {
            $driveId = "usbDisk$($disk.Number)"
            $driveArg = "file=\\.\PhysicalDrive$($disk.Number),format=raw,if=none,id=$driveId"
            if (-not $UsbDiskReadWrite) {
                $driveArg += ",readonly=on"
            }

            $qemuArgs += @("-drive", $driveArg, "-device", "usb-storage,drive=$driveId")
            $accessMode = if ($UsbDiskReadWrite) { "yazma izniyle" } else { "salt okunur" }
            Write-Host "USB disk eklendi: Disk $($disk.Number) - $($disk.FriendlyName) ($accessMode)" -ForegroundColor Yellow
        }

        if (-not $usbDisks) {
            Write-Host "USB disk bulunamadi; USB disk argümani eklenmedi." -ForegroundColor DarkYellow
        }
    } catch {
        Write-Host "[HATA] USB diskleri listelenemedi: $($_.Exception.Message)" -ForegroundColor Red
        exit 1
    }
}

if ($UsbHid) {
    # Yerel xHCI HID sürücüsünü test etmek için USB 3.0 denetleyicisi ve
    # boot protokollü klavye/fare ekle. QEMU varsayılan PS/2 girişini de
    # sunmaya devam eder; bu nedenle aynı tuşlar iki yoldan da gelebilir.
    $qemuArgs += @(
        "-device", "qemu-xhci,id=xhci",
        "-device", "usb-kbd,bus=xhci.0",
        "-device", "usb-mouse,bus=xhci.0"
    )
    Write-Host "USB HID eklendi: qemu-xhci + usb-kbd + usb-mouse" -ForegroundColor Yellow
}

function Test-DiskBootable([string]$filePath) {
    if (-not (Test-Path -LiteralPath $filePath -PathType Leaf)) { return $false }
    try {
        $abs = (Resolve-Path $filePath).Path
        $fs = [System.IO.File]::OpenRead($abs)
        $buf = New-Object byte[] (65536)
        $read = $fs.Read($buf, 0, $buf.Length)
        $fs.Close()
        if ($read -lt 512) { return $false }

        # Eğer dosya QCOW2 ise, qemu-img yardımıyla ya da qcow2 cluster'ı üzerinden konuk sektörüne bak:
        if ($buf[0] -eq 0x51 -and $buf[1] -eq 0x46 -and $buf[2] -eq 0x49 -and $buf[3] -eq 0xFB) {
            # Dosya içi hızlı tarama (Limine MBR ve bootloader dizileri qcow2 veri bloklarında yer alır)
            for ($i = 0; $i -le $read - 6; $i++) {
                if ($buf[$i] -eq 76 -and $buf[$i+1] -eq 73 -and $buf[$i+2] -eq 77 -and $buf[$i+3] -eq 73 -and $buf[$i+4] -eq 78 -and $buf[$i+5] -eq 69) {
                    return $true
                }
            }
            # Eğer ilk 64K'da bulunamadıysa qemu-img veya dosya boyutu kontrolü
            return ((Get-Item $filePath).Length -gt 2000000)
        }

        # RAW disk ise doğrudan LBA 0 sektörüne bakılır:
        if ($buf[510] -ne 0x55 -or $buf[511] -ne 0xAA) { return $false }
        $hasLimine = $false
        for ($i = 0; $i -le $read - 6; $i++) {
            if ($buf[$i] -eq 76 -and $buf[$i+1] -eq 73 -and $buf[$i+2] -eq 77 -and $buf[$i+3] -eq 73 -and $buf[$i+4] -eq 78 -and $buf[$i+5] -eq 69) {
                $hasLimine = $true
                break
            }
        }
        return $hasLimine
    } catch {
        return $false
    }
}

# Boot Seçimi:
# 1) ISO kullanılacaksa (-NoIso verilmemişse ve minos.iso mevcut/üretilebilirse) -> CD'den boot (-boot d)
# 2) ISO yoksa/kullanılmıyorsa -> Sabit diskten boot (-boot c)
$isoExists = (Test-Path "minos.iso") -or (Test-Path "build_iso.ps1")
$useIso = -not $NoIso -and $isoExists

if ($useIso) {
    if (Test-Path ".\build_iso.ps1") {
        & powershell -ExecutionPolicy Bypass -File ".\build_iso.ps1"
    }
    if (Test-Path "minos.iso") {
        Write-Host "[BILGI] ISO bulundu; CD-ROM üzerinden baslatiliyor (-boot d)..." -ForegroundColor Green
        $qemuArgs += @("-boot", "d", "-cdrom", "minos.iso")
    } else {
        Write-Host "[HATA] minos.iso olusturulamadi veya bulunamadi!" -ForegroundColor Red
        exit 1
    }
} else {
    Write-Host "[BILGI] ISO bagli degil; sabit disk kontrol ediliyor..." -ForegroundColor Yellow
    if (-not $DiskImage -or -not (Test-Path -LiteralPath $DiskImage -PathType Leaf)) {
        Write-Host "[HATA] Sabit disk imaji bulunamadi: $DiskImage. Sistem boot edilemez!" -ForegroundColor Red
        exit 1
    }

    $isBootable = Test-DiskBootable $DiskImage
    if (-not $isBootable) {
        Write-Host "[HATA] Disk formatli/onyuklenebilir degil ($DiskImage). Sistem boot edilemez!" -ForegroundColor Red
        Write-Host "[IPUCU] Once ISO ile baslatip 'Kur' uygulamasini calistirin veya build_disk.ps1 ile diski hazirlayin." -ForegroundColor Yellow
        exit 1
    }

    Write-Host "[BASARILI] Disk boot edilebilir dogrulandi; sabit diskten baslatiliyor (-boot c)..." -ForegroundColor Green
    $qemuArgs += @("-boot", "c")
}

& $qemuExe $qemuArgs
