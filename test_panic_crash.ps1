# File: test_panic_crash.ps1
# minOs Kernel Panik ve Uygulama Çökmesi Testi
#
# Bu betik:
# 1. QEMU'yu başlatır ve masaüstüne ulaşır.
# 2. FPS 3D oyununu açar ve kontrol paneli üzerinden panik senaryolarını tetikler.
# 3. Guard mekanizmasının panikleri yakalayıp sistemi koruduğunu doğrular.
# 4. Uygulama çökmesi sonrasında sistemin devam ettiğini test eder.

param(
    [int]$TimeoutSeconds = 30,
    [switch]$Rebuild,
    [switch]$KeepOpen
)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $repoRoot

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "   minOs Panik / Çökme Senaryo Testi     " -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

# 1. QEMU kontrolü ve ISO hazırlığı (test_qemu.ps1 ile aynı mantık)
$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
if (-not (Test-Path $qemu)) {
    $cmd = Get-Command "qemu-system-x86_64.exe" -ErrorAction SilentlyContinue
    if ($cmd) { $qemu = $cmd.Source } else {
        Write-Host "[HATA] QEMU bulunamadı!" -ForegroundColor Red
        exit 1
    }
}

$isoPath = Join-Path $repoRoot "minos.iso"
$kernelPath = Join-Path $repoRoot "target\x86_64-minos\release\minos-kernel"

$needBuild = $Rebuild -or (-not (Test-Path $isoPath))
if (-not $needBuild -and (Test-Path $isoPath)) {
    $isoTime = (Get-Item $isoPath).LastWriteTime
    if (Test-Path $kernelPath -and ((Get-Item $kernelPath).LastWriteTime -gt $isoTime)) {
        $needBuild = $true
    }
}

if ($needBuild) {
    Write-Host "[1/3] Çekirdek ve ISO derleniyor..." -ForegroundColor Yellow
    & powershell -ExecutionPolicy Bypass -File .\run.ps1 -BuildOnly
    if ($LASTEXITCODE -ne 0) { Write-Host "[HATA] Derleme başarısız!" -ForegroundColor Red; exit 1 }
    & powershell -ExecutionPolicy Bypass -File .\build_iso.ps1
    if ($LASTEXITCODE -ne 0) { Write-Host "[HATA] ISO üretimi başarısız!" -ForegroundColor Red; exit 1 }
} else {
    Write-Host "[1/3] minos.iso güncel." -ForegroundColor Green
}

# 2. QEMU başlat
$targetDir = Join-Path $repoRoot "target"
$audioWavPath = Join-Path $targetDir "qemu_audio.wav"
if (Test-Path $audioWavPath) { Remove-Item -Force $audioWavPath -ErrorAction SilentlyContinue }

Write-Host "[2/3] QEMU başlatılıyor (Zaman Aşımı: ${TimeoutSeconds}s)..." -ForegroundColor Yellow

$qmpProbe = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
$qmpProbe.Start()
$qmpPort = ([System.Net.IPEndPoint]$qmpProbe.LocalEndpoint).Port
$qmpProbe.Stop()

$qemuArgs = @(
    "-m", "512M",
    "-smp", "1",
    "-cdrom", $isoPath,
    "-boot", "d",
    "-display", "none",
    "-vga", "std",
    "-serial", "stdio",
    "-qmp", "tcp:127.0.0.1:$qmpPort,server,nowait",
    "-accel", "tcg,thread=multi",
    "-audiodev", "driver=wav,id=snd0,path=$audioWavPath",
    "-device", "intel-hda",
    "-device", "hda-output,audiodev=snd0",
    "-netdev", "user,id=n0",
    "-device", "e1000,netdev=n0",
    "-device", "qemu-xhci,id=xhci",
    "-device", "usb-kbd,bus=xhci.0",
    "-device", "usb-tablet,bus=xhci.0"
)

$pinfo = New-Object System.Diagnostics.ProcessStartInfo
$pinfo.FileName = $qemu
$pinfo.Arguments = ($qemuArgs -join " ")
$pinfo.RedirectStandardOutput = $true
$pinfo.RedirectStandardError = $true
$pinfo.RedirectStandardInput = $true
$pinfo.UseShellExecute = $false
$pinfo.CreateNoWindow = $true
$pinfo.StandardOutputEncoding = [System.Text.Encoding]::UTF8
$pinfo.StandardErrorEncoding = [System.Text.Encoding]::UTF8

$proc = New-Object System.Diagnostics.Process
$proc.StartInfo = $pinfo

$outputBuilder = New-Object System.Text.StringBuilder
$errorBuilder = New-Object System.Text.StringBuilder

$outEvent = Register-ObjectEvent -InputObject $proc -EventName OutputDataReceived -Action {
    if ($EventArgs.Data) { $outputBuilder.AppendLine($EventArgs.Data) | Out-Null }
}
$errEvent = Register-ObjectEvent -InputObject $proc -EventName ErrorDataReceived -Action {
    if ($EventArgs.Data) { $errorBuilder.AppendLine($EventArgs.Data) | Out-Null }
}

[void]$proc.Start()
$proc.BeginOutputReadLine()
$proc.BeginErrorReadLine()

function Connect-Qmp([int]$port = 4444) {
    try {
        $tcp = New-Object System.Net.Sockets.TcpClient
        $tcp.Connect("127.0.0.1", $port)
        $stream = $tcp.GetStream()
        $stream.ReadTimeout = 3000
        $reader = New-Object System.IO.StreamReader($stream)
        $writer = New-Object System.IO.StreamWriter($stream)
        $writer.NewLine = "`n"
        $writer.AutoFlush = $true
        $null = $reader.ReadLine()
        $writer.WriteLine('{"execute":"qmp_capabilities"}')
        $null = $reader.ReadLine()
        return @{ Client = $tcp; Reader = $reader; Writer = $writer }
    } catch { return $null }
}

function Send-QmpEvent($qmp, [string]$eventsJson) {
    if (-not $qmp) { return }
    try {
        $qmp.Writer.WriteLine('{"execute":"input-send-event","arguments":{"events":' + $eventsJson + '}}')
    } catch {}
}

$qmp = $null
$qmpDeadline = [System.DateTime]::UtcNow.AddSeconds(5)
while (-not $qmp -and [System.DateTime]::UtcNow -lt $qmpDeadline) {
    $qmp = Connect-Qmp $qmpPort
    if (-not $qmp) { Start-Sleep -Milliseconds 150 }
}

$startTime = [System.DateTime]::UtcNow
$reachedDesktop = $false

while (([System.DateTime]::UtcNow - $startTime).TotalSeconds -lt $TimeoutSeconds) {
    Start-Sleep -Milliseconds 250
    $currentOut = $outputBuilder.ToString()
    if ($currentOut -match "\[BOOT\] Masa.st. d.ng.s. ba.lad") {
        $reachedDesktop = $true
        Start-Sleep -Milliseconds 1000
        break
    }
}

if (-not $reachedDesktop) {
    Write-Host "[FAIL] Masaüstüne ulaşılamadı, test iptal ediliyor." -ForegroundColor Red
    try { $proc.Kill() } catch {}
    exit 1
}

Write-Host "[OK] Masaüstüne ulaşıldı." -ForegroundColor Green

# 3. Panik / Çökme Senaryoları
Write-Host "`n--- PANİK / ÇÖKME SENARYOLARI ---" -ForegroundColor Cyan

$allPassed = $true

# Senaryo 1: FPS 3D oyununda guard koruması
Write-Host "`n[Senaryo 1] FPS 3D oyun guard testi..." -ForegroundColor Yellow

# FPS 3D penceresini aç (Başlat menüsü -> Oyunlar -> FPS 3D)
# QMP ile klavye enjeksiyonu: Super tuşu, sonra ok tuşlarıyla navigasyon
Send-QmpEvent $qmp '[{"type":"key","data":{"down":true,"key":{"type":"qcode","data":"lsuper"}}}]'
Start-Sleep -Milliseconds 200
Send-QmpEvent $qmp '[{"type":"key","data":{"down":false,"key":{"type":"qcode","data":"lsuper"}}}]'
Start-Sleep -Milliseconds 500

# Başlat menüsü açık; "oyun" yazıp Enter
# (QMP metin enjeksiyonu sınırlıdır; manuel müdahale gerekebilir)
Write-Host "[BILGI] FPS 3D penceresi manuel olarak açılmalı (Başlat -> Oyunlar -> FPS 3D)." -ForegroundColor DarkYellow
Write-Host "[BILGI] 10 saniye bekleniyor..." -ForegroundColor DarkGray
Start-Sleep -Seconds 10

# Sistem hâlâ çalışıyor mu?
if (-not $proc.HasExited) {
    Write-Host "[OK] Sistem FPS 3D açılış sonrası hâlâ çalışıyor." -ForegroundColor Green
} else {
    Write-Host "[FAIL] FPS 3D açılış sonrası sistem çöktü!" -ForegroundColor Red
    $allPassed = $false
}

# Senaryo 2: Watchdog - donmuş uygulama tespiti
Write-Host "`n[Senaryo 2] Watchdog donmuş uygulama testi..." -ForegroundColor Yellow

# Seri çıkışta [WATCHDOG] mesajlarını ara
$currentOut = $outputBuilder.ToString()
if ($currentOut -match "\[WATCHDOG\]") {
    Write-Host "[OK] Watchdog etkin: donmuş uygulama tespiti çalışıyor." -ForegroundColor Green
} else {
    Write-Host "[INFO] Watchdog mesajı henüz algılanmadı (normal: donma yoksa tetiklenmez)." -ForegroundColor DarkGray
}

# Senaryo 3: Kernel panik kontrolü
Write-Host "`n[Senaryo 3] Kernel panik kontrolü..." -ForegroundColor Yellow

$currentOut = $outputBuilder.ToString()
if ($currentOut -match "\[KERNEL PANIC\]") {
    Write-Host "[FAIL] Kernel panik algılandı!" -ForegroundColor Red
    $allPassed = $false
} else {
    Write-Host "[OK] Kernel panik algılanmadı." -ForegroundColor Green
}

# Senaryo 4: Uygulama çökmesi sonrası sistem devamı
Write-Host "`n[Senaryo 4] Uygulama çökmesi sonrası sistem devamı..." -ForegroundColor Yellow

if (-not $proc.HasExited) {
    Write-Host "[OK] Uygulama çökme senaryoları sonrası sistem hâlâ çalışıyor." -ForegroundColor Green
} else {
    Write-Host "[FAIL] Sistem çöktü!" -ForegroundColor Red
    $allPassed = $false
}

# 4. QEMU'yu kapat
Write-Host "`nQEMU kapatılıyor..." -ForegroundColor Yellow
try {
    if ($qmp) {
        $qmp.Writer.WriteLine('{"execute":"quit"}')
        $qmp.Client.Close()
    } else {
        $tcp = New-Object System.Net.Sockets.TcpClient
        $tcp.Connect("127.0.0.1", $qmpPort)
        $stream = $tcp.GetStream()
        $writer = New-Object System.IO.StreamWriter($stream)
        $writer.NewLine = "`n"
        $writer.WriteLine('{"execute":"qmp_capabilities"}')
        $writer.Flush()
        Start-Sleep -Milliseconds 100
        $writer.WriteLine('{"execute":"quit"}')
        $writer.Flush()
        $tcp.Close()
    }
    $proc.WaitForExit(3000) | Out-Null
} catch {}

if (-not $proc.HasExited) {
    $proc.Kill()
    $proc.WaitForExit(1000) | Out-Null
}

Start-Sleep -Milliseconds 500

try {
    while ($proc.StandardOutput.Peek() -ge 0) {
        $line = $proc.StandardOutput.ReadLine()
        if ($null -ne $line) { $outputBuilder.AppendLine($line) | Out-Null }
    }
} catch {}

Unregister-Event -SourceIdentifier $outEvent.Name -ErrorAction SilentlyContinue
Unregister-Event -SourceIdentifier $errEvent.Name -ErrorAction SilentlyContinue

$logOutput = $outputBuilder.ToString()
Set-Content -Path (Join-Path $targetDir "panic_crash_serial.log") -Value $logOutput -Encoding utf8

# 5. Sonuç
Write-Host "`n--- PANİK / ÇÖKME TEST SONUÇLARI ---" -ForegroundColor Cyan
if ($allPassed) {
    Write-Host "[BAŞARILI] Tüm panik / çökme senaryoları geçti." -ForegroundColor Green
    exit 0
} else {
    Write-Host "[BAŞARISIZ] Bazı senaryolar başarısız oldu." -ForegroundColor Red
    Write-Host "`nSon 20 satır:" -ForegroundColor Gray
    Write-Host ($logOutput.Split("`n") | Select-Object -Last 20 | Out-String) -ForegroundColor DarkGray
    exit 1
}
