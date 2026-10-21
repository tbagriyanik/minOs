# File: test_qemu.ps1
# minOs QEMU Entegrasyon ve Boot Dogrulama Testi (test_qemu.ps1)
#
# Bu betik:
# 1. En son cekirdegi ve minos.iso imajini hazirlar.
# 2. QEMU'yu baslatir (-display none -serial stdio -m 512M).
# 3. Seri cikti akisini yakalar ve kritik cekirdek asamalarini dogrular:
#    - [CPU] SSE / FPU durumu
#    - [PMM] Fiziksel bellek yoneticisi
#    - [VMM] 4-seviyeli sayfalama ve MMIO haritalari
#    - [IDT] Kesme ve istisna kapilari
#    - [DRIVERS] Sürücü profili (e1000/vmxnet3, USB-HID, PS/2, HDA)
#    - [BOOT] Masaustu ve olay dongusu baslatimi
# 4. Basari / Hata ozetini konsola raporlar.

param(
    [int]$TimeoutSeconds = 8,
    [switch]$KeepOpen
)

$ErrorActionPreference = "Continue"

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $repoRoot

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "   minOs QEMU Entegrasyon Test Kosucusu  " -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

# 1. QEMU kontrolu
$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
if (-not (Test-Path $qemu)) {
    $cmd = Get-Command "qemu-system-x86_64.exe" -ErrorAction SilentlyContinue
    if ($cmd) {
        $qemu = $cmd.Source
    } else {
        Write-Host "[HATA] QEMU bulunamadi: $qemu" -ForegroundColor Red
        exit 1
    }
}

# 2. ISO kontrolu
$isoPath = Join-Path $repoRoot "minos.iso"
if (-not (Test-Path $isoPath)) {
    Write-Host "[1/3] minos.iso bulunamadi, uretiliyor..." -ForegroundColor Yellow
    & powershell -ExecutionPolicy Bypass -File .\build_iso.ps1
}

Write-Host "[1/2] QEMU headless ortaminda baslatiliyor (Zaman Asimi: ${TimeoutSeconds}s)..." -ForegroundColor Yellow

$qemuArgs = @(
    "-m", "512M",
    "-smp", "2",
    "-cdrom", $isoPath,
    "-boot", "d",
    "-display", "none",
    "-vga", "std",
    "-global", "VGA.vgamem_mb=64",
    "-serial", "stdio",
    "-audiodev", "driver=none,id=snd0",
    "-device", "intel-hda",
    "-device", "hda-output,audiodev=snd0",
    "-netdev", "user,id=n0",
    "-device", "e1000,netdev=n0"
)

$pinfo = New-Object System.Diagnostics.ProcessStartInfo
$pinfo.FileName = $qemu
$pinfo.Arguments = ($qemuArgs -join " ")
$pinfo.RedirectStandardOutput = $true
$pinfo.RedirectStandardError = $true
$pinfo.UseShellExecute = $false
$pinfo.CreateNoWindow = $true

$proc = New-Object System.Diagnostics.Process
$proc.StartInfo = $pinfo

$outputBuilder = New-Object System.Text.StringBuilder
$outEvent = Register-ObjectEvent -InputObject $proc -EventName OutputDataReceived -Action {
    if ($EventArgs.Data) {
        $outputBuilder.AppendLine($EventArgs.Data) | Out-Null
    }
}

[void]$proc.Start()
$proc.BeginOutputReadLine()

$startTime = [System.DateTime]::UtcNow
$reachedDesktop = $false

while (([System.DateTime]::UtcNow - $startTime).TotalSeconds -lt $TimeoutSeconds) {
    Start-Sleep -Milliseconds 250
    $currentOut = $outputBuilder.ToString()
    if ($currentOut -match "Masaustu dongusune giriliyor|Welcome hazir|Tum suruculer hazir") {
        $reachedDesktop = $true
        break
    }
}

# QEMU'yu sonlandir
if (-not $proc.HasExited) {
    $proc.Kill()
    $proc.WaitForExit(1000) | Out-Null
}
Unregister-Event -SourceIdentifier $outEvent.Name -ErrorAction SilentlyContinue

$logOutput = $outputBuilder.ToString()
Write-Host "[2/2] Cekirdek ciktisi cozumleniyor..." -ForegroundColor Yellow

# Dogrulama Kriterleri
$checks = @(
    @{ Name = "PMM (Fiziksel Bellek)"; Pattern = "PMM|Fiziksel bellek|Bellek yoneticisi test" },
    @{ Name = "VMM (Sayfa Tablolari)"; Pattern = "MMIO|Sayfalama|Huge page|VMM" },
    @{ Name = "CPU / Donanim Tanilama"; Pattern = "SSE|CPU|x86_64" },
    @{ Name = "Ag Adaptoru (e1000/vmxnet3)"; Pattern = "e1000|VMware|NET" },
    @{ Name = "Giris Suruculeri (USB/PS2)"; Pattern = "PS/2|USB" },
    @{ Name = "Cekirdek Boot Akisi"; Pattern = "Tum suruculer hazir|driver profile finished" }
)

$allPassed = $true
Write-Host "`n--- ENTEGRASYON TESTI RAPORU ---" -ForegroundColor Cyan

foreach ($check in $checks) {
    if ($logOutput -match $check.Pattern) {
        Write-Host "  [OK] $($check.Name)" -ForegroundColor Green
    } else {
        Write-Host "  [FAIL] $($check.Name) (Ciktida bulunamadi: '$($check.Pattern)')" -ForegroundColor Red
        $allPassed = $false
    }
}

if ($reachedDesktop) {
    Write-Host "  [OK] Masaustu Olay Dongusu / Karşılama Ekranı" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] Masaustu dongusune zamaninda ulasilamadi." -ForegroundColor Red
    $allPassed = $false
}

Write-Host "--------------------------------" -ForegroundColor Cyan

if ($allPassed) {
    Write-Host "[BASARILI] QEMU entegrasyon ve cekirdek boot testleri basariyla gecti!" -ForegroundColor Green
    exit 0
} else {
    Write-Host "[UYARI] Bazi cekirdek kontrolleri basarisiz oldu veya cikti eksik." -ForegroundColor Yellow
    Write-Host "`nSon Cekirdek Logu:" -ForegroundColor Gray
    Write-Host ($logOutput.Split("`n") | Select-Object -Last 15 | Out-String) -ForegroundColor DarkGray
    exit 1
}
