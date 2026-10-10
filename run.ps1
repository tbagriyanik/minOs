# MinOS Derle ve QEMU ile Çalıştır (PowerShell)
param(
    [switch]$BuildOnly = $false,
    [switch]$Debug = $false
)

$ErrorActionPreference = "Stop"

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "       MinOS Derleyici & Başlatıcı       " -ForegroundColor Cyan
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
    Write-Host "[HATA] QEMU bulunamadı! Lütfen QEMU'nun kurulu olduğundan emin olun." -ForegroundColor Red
    exit 1
}

Write-Host "[1/3] QEMU Tespit Edildi: $qemuExe" -ForegroundColor Green

# 2. Kernel'ı Derle
Write-Host "[2/3] MinOS Çekirdeği Derleniyor (cargo build)..." -ForegroundColor Yellow
$cargoArgs = @("build", "-Zjson-target-spec", "--target", "x86_64-minos.json", "-Zbuild-std=core,alloc", "-Zbuild-std-features=compiler-builtins-mem")
if (-not $Debug) {
    $cargoArgs += "--release"
}

try {
    & cargo $cargoArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[HATA] Derleme başarısız oldu." -ForegroundColor Red
        exit $LASTEXITCODE
    }
} catch {
    Write-Host "Derleme sırasında standart kütüphane derleme bayrakları denetleniyor..." -ForegroundColor Yellow
    & cargo build -Zjson-target-spec --target x86_64-minos.json --release
}

$kernelBin = if ($Debug) { "target\x86_64-minos\debug\minos-kernel" } else { "target\x86_64-minos\release\minos-kernel" }

if (-not (Test-Path $kernelBin)) {
    # Alternatif hedef dosya adı kontrolü
    $kernelBin = if ($Debug) { "target\x86_64-minos\debug\minos" } else { "target\x86_64-minos\release\minos" }
}

if ($BuildOnly) {
    Write-Host "[BAŞARILI] Derleme tamamlandı: $kernelBin" -ForegroundColor Green
    exit 0
}

# 3. QEMU İle Başlat
Write-Host "[3/3] QEMU Başlatılıyor..." -ForegroundColor Yellow
Write-Host "=========================================" -ForegroundColor Cyan

$qemuArgs = @(
    "-m", "128M",
    "-boot", "d",
    "-serial", "stdio",
    "-vga", "std"
)

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
    Write-Host "[HATA] Çalıştırılacak kernel ikilisi veya minos.iso bulunamadı!" -ForegroundColor Red
    exit 1
}

& $qemuExe $qemuArgs
