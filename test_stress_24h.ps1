# File: test_stress_24h.ps1
# minOs Uzun Sureli Stres Testi ve Otomatik Panik Tespiti (24h Panic Test)
#
# Bu betik:
# 1. Cekirdek ve minos.iso imajini tazelik denetimiyle hazirlar.
# 2. QEMU'yu headless baslatir; seri ciktiyi TCP uzerinden canli yakalar.
# 3. Masaustune ulasana kadar bekler, istege bagli olarak FPS 3D
#    uygulamasini QMP ile acar (gercek is yuku).
# 4. Belirlenen sure (varsayilan 24 saat) boyunca seri akisini surekli
#    tarar ve panik imzalarini otomatik tespit eder:
#      - [KERNEL PANIC] / KERNEL EXCEPTION / DOUBLE FAULT / CIFT HATA
#      - panic:/oops:/BUG: kalipli cekirdek mesajlari
#      - QEMU surecinin beklenmedik sekilde cikmasi (triple fault dahil)
# 5. Her panik icin zaman damgasi, son 60 seri satiri ve rapor dosyasi
#    uretir; test basarisiz olarak sonlanir.
# 6. Sure sonunda (veya Ctrl+C ile) QEMU'yu QMP uzerinden temiz kapatir
#    ve target/stress_report.txt ozetini yazar.
#
# Kullanim:
#   .\test_stress_24h.ps1                      # 24 saat, varsayilan ayarlar
#   .\test_stress_24h.ps1 -Hours 1             # 1 saatlik kisa dogrulama
#   .\test_stress_24h.ps1 -Hours 24 -Fps3d     # FPS 3D is yuku ile 24 saat
#   .\test_stress_24h.ps1 -Hours 2 -Rebuild    # once derle, sonra calistir
#
# Cikis kodlari: 0 = panik yok, 1 = panik/crash tespit edildi,
#               2 = masaustune ulasilamadi veya QEMU baslatilamadi.

param(
    [double]$Hours = 24,
    [switch]$Rebuild,
    [switch]$Fps3d,
    [switch]$Minimal,
    [string]$ReportPath = ""
)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $repoRoot

$durationSeconds = [int]($Hours * 3600)
if ($durationSeconds -le 0) {
    Write-Host "[HATA] -Hours 0'dan buyuk olmali." -ForegroundColor Red
    exit 2
}

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "   minOs 24h Stres / Panik Testi          " -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan
Write-Host ("[BILGI] Sure: {0} saat ({1} sn), FPS3D: {2}, Minimal: {3}" -f $Hours, $durationSeconds, $Fps3d, $Minimal) -ForegroundColor DarkGray

# --- QEMU bul -------------------------------------------------------------
$qemu = $null
foreach ($candidate in @("qemu-system-x86_64", "qemu-system-x86_64.exe")) {
    $cmd = Get-Command $candidate -ErrorAction SilentlyContinue
    if ($cmd) { $qemu = $cmd.Source; break }
}
if (-not $qemu -and (Test-Path "C:\Program Files\qemu\qemu-system-x86_64.exe")) {
    $qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
}
if (-not $qemu) {
    Write-Host "[HATA] QEMU bulunamadi." -ForegroundColor Red
    exit 2
}

# --- ISO tazelik denetimi (test_qemu.ps1 ile ayni mantik) ------------------
$isoPath = Join-Path $repoRoot "minos.iso"
$kernelPath = Join-Path $repoRoot "target\x86_64-minos\release\minos-kernel"

$needBuild = $Rebuild -or (-not (Test-Path $isoPath))
if (-not $needBuild -and (Test-Path $isoPath)) {
    $isoTime = (Get-Item $isoPath).LastWriteTime
    if ((Test-Path $kernelPath) -and ((Get-Item $kernelPath).LastWriteTime -gt $isoTime)) {
        $needBuild = $true
    }
    $recentSrc = Get-ChildItem -Path (Join-Path $repoRoot "src") -Recurse -Filter "*.rs" |
        Where-Object { $_.LastWriteTime -gt $isoTime } | Select-Object -First 1
    if ($recentSrc) { $needBuild = $true }
}

if ($needBuild) {
    Write-Host "[1/4] Cekirdek ve ISO derleniyor..." -ForegroundColor Yellow
    & powershell -ExecutionPolicy Bypass -File .\run.ps1 -BuildOnly
    if ($LASTEXITCODE -ne 0) { Write-Host "[HATA] Derleme basarisiz!" -ForegroundColor Red; exit 2 }
    & powershell -ExecutionPolicy Bypass -File .\build_iso.ps1
    if ($LASTEXITCODE -ne 0) { Write-Host "[HATA] ISO uretimi basarisiz!" -ForegroundColor Red; exit 2 }
} else {
    Write-Host "[1/4] minos.iso guncel." -ForegroundColor Green
}

# --- QEMU baslat -----------------------------------------------------------
$targetDir = Join-Path $repoRoot "target"
if (-not (Test-Path $targetDir)) { New-Item -ItemType Directory -Force -Path $targetDir | Out-Null }
$audioWavPath = Join-Path $targetDir "stress_audio.wav"
if (Test-Path $audioWavPath) { Remove-Item -Force $audioWavPath -ErrorAction SilentlyContinue }

$memMb = if ($Minimal) { 128 } else { 512 }
$smpCpus = if ($Minimal) { 1 } else { 2 }

$qmpProbe = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
$qmpProbe.Start()
$qmpPort = ([System.Net.IPEndPoint]$qmpProbe.LocalEndpoint).Port
$qmpProbe.Stop()

$serialProbe = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
$serialProbe.Start()
$serialPort = ([System.Net.IPEndPoint]$serialProbe.LocalEndpoint).Port
$serialProbe.Stop()

$qemuArgs = @(
    "-m", "$($memMb)M",
    "-smp", "$smpCpus",
    "-cdrom", $isoPath,
    "-boot", "d",
    "-display", "none",
    "-vga", "std",
    "-rtc", "base=utc,clock=host,driftfix=slew",
    "-serial", "tcp:127.0.0.1:$serialPort,server=on,wait=on",
    "-qmp", "tcp:127.0.0.1:$qmpPort,server,nowait",
    "-accel", "tcg,thread=multi"
)
if (-not $Minimal) {
    $qemuArgs += @(
        "-audiodev", "driver=wav,id=snd0,path=$audioWavPath",
        "-device", "intel-hda",
        "-device", "hda-output,audiodev=snd0",
        "-netdev", "user,id=n0",
        "-device", "e1000,netdev=n0"
    )
}

Write-Host "[2/4] QEMU baslatiliyor (RAM=${memMb}M, vCPU=$smpCpus)..." -ForegroundColor Yellow

$pinfo = New-Object System.Diagnostics.ProcessStartInfo
$pinfo.FileName = $qemu
$pinfo.Arguments = ($qemuArgs -join " ")
$pinfo.RedirectStandardOutput = $false
$pinfo.RedirectStandardError = $true
$pinfo.RedirectStandardInput = $false
$pinfo.UseShellExecute = $false
$pinfo.CreateNoWindow = $true
$pinfo.StandardErrorEncoding = [System.Text.Encoding]::UTF8

$proc = New-Object System.Diagnostics.Process
$proc.StartInfo = $pinfo
$errorBuilder = New-Object System.Text.StringBuilder
$errEvent = Register-ObjectEvent -InputObject $proc -EventName ErrorDataReceived -Action {
    if ($EventArgs.Data) { $errorBuilder.AppendLine($EventArgs.Data) | Out-Null }
}
[void]$proc.Start()
$proc.BeginErrorReadLine()

# --- Seri TCP yakalama -----------------------------------------------------
function Connect-Serial([int]$port, [int]$timeoutSeconds = 15) {
    $deadline = [System.DateTime]::UtcNow.AddSeconds($timeoutSeconds)
    while ([System.DateTime]::UtcNow -lt $deadline) {
        try {
            $tcp = New-Object System.Net.Sockets.TcpClient
            $tcp.Connect("127.0.0.1", $port)
            return $tcp
        } catch { Start-Sleep -Milliseconds 100 }
    }
    return $null
}

$serialTcp = Connect-Serial $serialPort 15
$serialStream = $null
$serialBytes = New-Object System.IO.MemoryStream
if ($serialTcp) { $serialStream = $serialTcp.GetStream() }

function Read-Serial {
    if ($script:serialStream) {
        try {
            $buf = New-Object byte[] 65536
            while ($script:serialStream.DataAvailable) {
                $n = $script:serialStream.Read($buf, 0, $buf.Length)
                if ($n -le 0) { break }
                $script:serialBytes.Write($buf, 0, $n)
            }
        } catch {}
    }
    return [System.Text.Encoding]::UTF8.GetString($script:serialBytes.ToArray())
}

# QMP baglantisi (karsilama ekrani + istege bagli FPS 3D + temiz kapatma)
function Connect-Qmp([int]$port) {
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

function Send-KeyTap($qmp, [string]$qcode, [int]$holdMs = 70) {
    if (-not $qmp) { return }
    Send-QmpEvent $qmp ('[{"type":"key","data":{"down":true,"key":{"type":"qcode","data":"' + $qcode + '"}}}]')
    Start-Sleep -Milliseconds $holdMs
    Send-QmpEvent $qmp ('[{"type":"key","data":{"down":false,"key":{"type":"qcode","data":"' + $qcode + '"}}}]')
}

$qmp = Connect-Qmp $qmpPort
if (-not $qmp) {
    Write-Host "[UYARI] QMP baglantisi kurulamadi; FPS3D ve temiz kapatma sinirlari." -ForegroundColor DarkYellow
}

# --- Panik imzalari ---------------------------------------------------------
# Kernel'in kendi panik ekranlari + klasik cekirdek hata kalipli. Buyuk/kucuk
# harf duyarsiz; seri akis zaten normalize edilmis metin.
$panicPatterns = @(
    '\[KERNEL PANIC\]',
    'KERNEL EXCEPTION',
    'DOUBLE FAULT',
    'CIFT HATA',
    'panic:',
    'kernel panic',
    'Oops:',
    'BUG:',
    'general protection fault',
    'triple fault'
)
$panicRegex = [regex]($panicPatterns -join '|')

$panicDetected = $false
$panicSignature = ""
$panicTimestamp = $null
$lastReportLength = 0
$lastHeartbeat = [System.DateTime]::UtcNow
$heartbeatMinutes = 15

# Bellek sizintisi izlemi: cekirdek her 64 scheduler diliminde bir
# `[SCHED-STAT] ... free_kb=N` satiri basar. Ornekler toplanir ve raporda
# ilk/son/min/max olarak ozetlenir.
$schedFreeKb = New-Object System.Collections.Generic.List[long]
$schedFreeRegex = [regex]'\[SCHED-STAT\][^\r\n]*?free_kb=(\d+)'

# --- Masaustune bekleme ------------------------------------------------------
Write-Host "[3/4] Masaustune ulasiliyor..." -ForegroundColor Yellow
$bootDeadline = [System.DateTime]::UtcNow.AddSeconds(180)
$reachedDesktop = $false
$dismissedWelcome = $false

while ([System.DateTime]::UtcNow -lt $bootDeadline) {
    if ($proc.HasExited) {
        Write-Host "[HATA] QEMU masaustune ulasmadan cikti (cokme?)." -ForegroundColor Red
        exit 2
    }
    Start-Sleep -Milliseconds 300
    $bootOut = Read-Serial
    if ($bootOut -match "Ho.geldin d.ng.s." -and -not $dismissedWelcome) {
        $dismissedWelcome = $true
        try { $proc.StandardInput.WriteLine("") } catch {}
    }
    if ($bootOut -match "\[BOOT\] Masa.st. d.ng.s. ba.lad") {
        $reachedDesktop = $true
        break
    }
}

if (-not $reachedDesktop) {
    Write-Host "[HATA] 180 sn icinde masaustune ulasilamadi." -ForegroundColor Red
    try { if ($qmp) { $qmp.Writer.WriteLine('{"execute":"quit"}'); $qmp.Client.Close() } } catch {}
    try { $proc.Kill() } catch {}
    exit 2
}
Write-Host "[OK] Masaustu erisildi; stres dongusu basliyor." -ForegroundColor Green

# --- FPS 3D is yuku (opsiyonel) ----------------------------------------------
if ($Fps3d -and $qmp) {
    Write-Host "[BILGI] FPS 3D uygulamasi aciliyor (QMP: Baslat -> 'fps')..." -ForegroundColor Yellow
    Send-KeyTap $qmp "lsuper" 140
    Start-Sleep -Milliseconds 500
    foreach ($k in @("f", "p", "s")) {
        Send-KeyTap $qmp $k 50
        Start-Sleep -Milliseconds 90
    }
    Start-Sleep -Milliseconds 250
    Send-KeyTap $qmp "ret" 90
    $fpsDeadline = [System.DateTime]::UtcNow.AddSeconds(20)
    while ([System.DateTime]::UtcNow -lt $fpsDeadline) {
        if ((Read-Serial) -match "\[FPS3D\] Uygulama") { break }
        if ($proc.HasExited) { break }
        Start-Sleep -Milliseconds 300
    }
    if ((Read-Serial) -match "\[FPS3D\] Uygulama") {
        Write-Host "[OK] FPS 3D is yuku aktif." -ForegroundColor Green
    } else {
        Write-Host "[UYARI] FPS 3D acilamadi; masaustu yukunde devam ediliyor." -ForegroundColor DarkYellow
    }
}

# --- Stres dongusu: surekli seri tarama + otomatik panik tespiti -------------
$stressStart = [System.DateTime]::UtcNow
$exitCode = 0
$endTime = $stressStart.AddSeconds($durationSeconds)
Write-Host ("[4/4] Stres testi basladi: {0} saat izlenecek. Ctrl+C ile erken durdurabilirsiniz." -f $Hours) -ForegroundColor Cyan

try {
    while ([System.DateTime]::UtcNow -lt $endTime) {
        Start-Sleep -Milliseconds 500

        if ($proc.HasExited) {
            $panicDetected = $true
            $panicSignature = "QEMU sureci beklenmedik sekilde cikti (exit code $($proc.ExitCode))"
            $panicTimestamp = [System.DateTime]::UtcNow
            break
        }

        $out = Read-Serial
        if ($out.Length -gt $lastReportLength) {
            $newText = $out.Substring($lastReportLength)
            $lastReportLength = $out.Length
            $match = $panicRegex.Match($newText)
            if ($match.Success) {
                $panicDetected = $true
                $panicSignature = $match.Value
                $panicTimestamp = [System.DateTime]::UtcNow
                break
            }
            foreach ($mm in $schedFreeRegex.Matches($newText)) {
                $schedFreeKb.Add([long]$mm.Groups[1].Value)
            }
        }

        # Periyodik kalp atisi: ilerleme + son gorulen onemli isaretler.
        if (([System.DateTime]::UtcNow - $lastHeartbeat).TotalMinutes -ge $heartbeatMinutes) {
            $elapsed = [System.DateTime]::UtcNow - $stressStart
            $remaining = $endTime - [System.DateTime]::UtcNow
            Write-Host ("[STRESS] {0:hh\:mm\:ss} gecti, {1:hh\:mm\:ss} kaldi, seri={2:N1} MB" -f $elapsed, $remaining, ($lastReportLength / 1MB)) -ForegroundColor DarkGray
            $lastHeartbeat = [System.DateTime]::UtcNow
        }
    }
} catch [System.Management.Automation.PipelineStoppedException] {
    Write-Host "`n[UYARI] Test kullanici tarafindan durduruldu (Ctrl+C)." -ForegroundColor Yellow
}

# --- Temiz kapanis ----------------------------------------------------------
Write-Host "`nQEMU kapatiliyor..." -ForegroundColor Yellow
try {
    if ($qmp) {
        $qmp.Writer.WriteLine('{"execute":"quit"}')
        Start-Sleep -Milliseconds 300
        $qmp.Client.Close()
    }
} catch {}
try {
    if (-not $proc.HasExited) { $proc.WaitForExit(3000) | Out-Null }
} catch {}
if (-not $proc.HasExited) {
    try { $proc.Kill() } catch {}
    try { $proc.WaitForExit(1000) | Out-Null } catch {}
}
try { if ($serialTcp) { $serialTcp.Close() } } catch {}
Unregister-Event -SourceIdentifier $errEvent.Name -ErrorAction SilentlyContinue

# --- Rapor -------------------------------------------------------------------
$finalOut = Read-Serial
if ($finalOut.Length -gt 0) { $serialBytes = New-Object System.IO.MemoryStream; [void]$serialBytes.Write([System.Text.Encoding]::UTF8.GetBytes($finalOut), 0, $finalOut.Length) }

if ([string]::IsNullOrEmpty($ReportPath)) {
    $ReportPath = Join-Path $targetDir "stress_report.txt"
}

$elapsedTotal = [System.DateTime]::UtcNow - $stressStart
$tailLines = ($finalOut -split "`n") | Select-Object -Last 60

$report = @()
$report += "minOs Stres / 24h Panic Test Raporu"
$report += "======================================"
$report += "Baslangic : $($stressStart.ToString('yyyy-MM-dd HH:mm:ss'))"
$report += "Bitis    : $([System.DateTime]::UtcNow.ToString('yyyy-MM-dd HH:mm:ss'))"
$report += "Gecen    : $([math]::Round($elapsedTotal.TotalHours, 2)) saat"
$report += "Hedef    : $Hours saat"
$report += "FPS3D    : $Fps3d"
$report += "Minimal  : $Minimal"
$report += "Sonuc    : $(if ($panicDetected) { 'PANIK TESPIT EDILDI' } elseif ($elapsedTotal.TotalSeconds -ge ($durationSeconds * 0.95)) { 'BASARILI (sure doldu)' } else { 'ERKEN DURDURULDU' })"
if ($panicDetected) {
    $report += "Imza     : $panicSignature"
    if ($panicTimestamp) { $report += "Zaman    : $($panicTimestamp.ToString('yyyy-MM-dd HH:mm:ss'))" }
}

# Bellek egilimi ozeti (SCHED-STAT free_kb ornekleri)
if ($schedFreeKb.Count -gt 0) {
    $freeFirst = $schedFreeKb[0]
    $freeLast = $schedFreeKb[$schedFreeKb.Count - 1]
    $freeMin = ($schedFreeKb | Measure-Object -Minimum).Minimum
    $freeMax = ($schedFreeKb | Measure-Object -Maximum).Maximum
    $report += ""
    $report += "--- Bellek Egitimi (SCHED-STAT free_kb) ---"
    $report += ("Ornek sayisi : {0}" -f $schedFreeKb.Count)
    $report += ("Ilk free_kb  : {0}" -f $freeFirst)
    $report += ("Son free_kb  : {0}" -f $freeLast)
    $report += ("Min free_kb  : {0}" -f $freeMin)
    $report += ("Max free_kb  : {0}" -f $freeMax)
    if ($freeLast -lt [math]::Floor($freeFirst * 0.5)) {
        $report += "UYARI       : free_kb belirgin dustu; bellek sizintisi suphesi."
    }
}

$report += ""
$report += "--- Son 60 seri satiri ---"
$report += $tailLines

$report | Set-Content -Path $ReportPath -Encoding utf8

# Tam seri akisini artifact/analiz icin ayri dosyaya dok.
$serialLogPath = Join-Path $targetDir "stress_serial.log"
Set-Content -LiteralPath $serialLogPath -Value $finalOut -Encoding utf8

if ($panicDetected) {
    Write-Host "`n[FAIL] PANIK TESPIT EDILDI: $panicSignature" -ForegroundColor Red
    Write-Host "[BILGI] Rapor: $ReportPath" -ForegroundColor DarkGray
    exit 1
} else {
    Write-Host "`n[OK] $($elapsedTotal.TotalHours.ToString('0.00')) saatlik stres testi panik olmadan tamamlandi." -ForegroundColor Green
    Write-Host "[BILGI] Rapor: $ReportPath" -ForegroundColor DarkGray
    exit 0
}
