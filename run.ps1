param(
    [switch]$BuildOnly,
    [switch]$Debug,
    [switch]$UsbDisks,
    [switch]$UsbDiskReadWrite,
    [ValidateRange(1, 64)][int]$CpuCount = 2,
    [ValidatePattern('^[1-9][0-9]*(M|G)$')][string]$Memory = "512M",
    [string]$DiskImage
)

# MinOS Derle ve QEMU ile Çalistir (PowerShell)
chcp 65001 | Out-Null
$PSDefaultParameterValues['Out-File:Encoding'] = 'utf8'; $PSDefaultParameterValues['Set-Content:Encoding'] = 'utf8';
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$ErrorActionPreference = "Stop"

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "       MinOS Derleyici & Baslatici       " -ForegroundColor Cyan
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
Write-Host "[2/3] MinOS Çekirdeği Derleniyor (cargo build)..." -ForegroundColor Yellow
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
    "-boot", "d",
    "-serial", "stdio",
    "-vga", "std",
    # SDL ses çıkışı DirectSound'un zorunlu capture/mikrofon açma hatasını önler.
    "-audiodev", "driver=sdl,id=snd0",
    "-device", "intel-hda",
    # Yalnızca çıkış codec'i: DirectSoundCapture / mikrofon başlatılmaz.
    "-device", "hda-output,audiodev=snd0",
    # --- Ağ: e1000 + QEMU user modu (SLIRP) — ARP/ping/TCP sağlar ---
    "-netdev", "user,id=n0,hostfwd=tcp::8080-:80",
    "-device", "e1000,netdev=n0"
)

if ($DiskImage) {
    if (-not (Test-Path -LiteralPath $DiskImage -PathType Leaf)) {
        Write-Host "[HATA] Disk imajı bulunamadi: $DiskImage" -ForegroundColor Red
        exit 1
    }
    $qemuArgs += @("-drive", "file=$DiskImage,format=qcow2,if=ide")
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

if (Test-Path ".\build_iso.ps1") {
    & powershell -ExecutionPolicy Bypass -File ".\build_iso.ps1"
}

if (Test-Path "minos.iso") {
    $qemuArgs += @("-cdrom", "minos.iso")
} elseif (Test-Path "target\iso_root") {
    $qemuArgs += @("-drive", "file=fat:rw:target\iso_root,format=raw")
} elseif (Test-Path $kernelBin) {
    $qemuArgs += @("-kernel", $kernelBin)
} else {
    Write-Host "[HATA] Calistirilacak kernel ikilisi veya minos.iso bulunamadi!" -ForegroundColor Red
    exit 1
}

& $qemuExe $qemuArgs
