# File: test_perf.ps1
# minOs Performans Testi Dogrulama Betigi (test_perf.ps1)
#
# Bu betik:
# 1. Limine yapilandirmasina gecici olarak `perftest` belirteci ekler; boylece
#    ilk boot girdisi Kontrol Paneli "Testler" sekmesini ac ve performans
#    testini otomatik baslatir (GUI otomasyonu gerektirmez).
# 2. Cekirdegi ve ISO imajini gunceller.
# 3. QEMU'yu headless baslatir (-display none, seri konsol TCP uzerinden).
# 4. Seri konsoldaki `[PERF]` satirini yakalar ve puani dogrular.
# 5. Limine yapilandirmasini geri alir ve temiz ISO'yu yeniden uretir.
#
# Kullanim:
#   powershell -ExecutionPolicy Bypass -File .\test_perf.ps1
#   powershell -ExecutionPolicy Bypass -File .\test_perf.ps1 -MinScore 75
#   powershell -ExecutionPolicy Bypass -File .\test_perf.ps1 -Vga std

param(
    # Kabul edilebilir en dusuk puan (varsayilan: sanal makinede 70).
    [int]$MinScore = 70,
    [int]$TimeoutSeconds = 240,
    [switch]$Rebuild,
    [ValidateSet("virtio", "std", "vmware", "qxl")][string]$Vga = "virtio",
    [switch]$Minimal,
    [switch]$KeepLogs
)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $repoRoot

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "   minOs Performans Testi Dogrulama     " -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

# --- 1. Limine yapilandirmasina gecici `perftest` belirteci ----------------
$limineConf = Join-Path $repoRoot "boot\limine.conf"
$confBackup = [IO.File]::ReadAllText($limineConf)
$confPatched = $confBackup -replace "(?m)^(\s*kernel_cmdline:\s*live full)\s*$", "`$1 perftest"
if ($confPatched -eq $confBackup) {
    Write-Host "[HATA] Limine 'live full' girdisi bulunamadi (perftest eklenemedi)." -ForegroundColor Red
    exit 1
}

function Restore-Conf {
    [IO.File]::WriteAllText($limineConf, $confBackup, (New-Object System.Text.UTF8Encoding($false)))
    Write-Host "[BILGI] Limine yapilandirmasi geri alindi." -ForegroundColor DarkGray
}

try {
    [IO.File]::WriteAllText($limineConf, $confPatched, (New-Object System.Text.UTF8Encoding($false)))
    Write-Host "[1/4] Limine yapilandirmasina 'perftest' eklendi." -ForegroundColor Yellow

    # --- 2. Derleme / ISO tazeligi -----------------------------------------
    $isoPath = Join-Path $repoRoot "minos.iso"
    $kernelPath = Join-Path $repoRoot "target\x86_64-minos\release\minos-kernel"

    $needBuild = $Rebuild -or (-not (Test-Path $isoPath))
    if (-not $needBuild -and (Test-Path $isoPath)) {
        $isoTime = (Get-Item $isoPath).LastWriteTime
        if ((Test-Path $kernelPath) -and (Get-Item $kernelPath).LastWriteTime -gt $isoTime) {
            $needBuild = $true
        }
        $recentSrc = Get-ChildItem -Path (Join-Path $repoRoot "src") -Recurse -Filter "*.rs" |
            Where-Object { $_.LastWriteTime -gt $isoTime } | Select-Object -First 1
        if ($recentSrc) { $needBuild = $true }
    }

    if ($needBuild) {
        Write-Host "[2/4] Cekirdek ve ISO derleniyor..." -ForegroundColor Yellow
        & powershell -ExecutionPolicy Bypass -File .\run.ps1 -BuildOnly
        if ($LASTEXITCODE -ne 0) { Write-Host "[HATA] Cekirdek derlemesi basarisiz." -ForegroundColor Red; exit 1 }
    }
    # Limine yapilandirmasi degistigi icin ISO her zaman yeniden uretilir.
    & powershell -ExecutionPolicy Bypass -File .\build_iso.ps1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Write-Host "[HATA] ISO uretimi basarisiz." -ForegroundColor Red; exit 1 }
    Write-Host "[2/4] ISO hazir." -ForegroundColor Green

    # --- 3. QEMU ------------------------------------------------------------
    $qemu = $null
    foreach ($c in @("qemu-system-x86_64.exe", "$env:ProgramFiles\qemu\qemu-system-x86_64.exe")) {
        $cmd = Get-Command $c -ErrorAction SilentlyContinue
        if ($cmd) { $qemu = $cmd.Source; break }
        if (Test-Path $c) { $qemu = $c; break }
    }
    if (-not $qemu) { Write-Host "[HATA] QEMU bulunamadi." -ForegroundColor Red; exit 1 }

    $memMb = 512
    $smpCpus = 2
    if ($Minimal) { $memMb = 128; $smpCpus = 1 }

    function Get-FreePort {
        $l = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
        $l.Start(); $p = ([System.Net.IPEndPoint]$l.LocalEndpoint).Port; $l.Stop(); return $p
    }
    $serialPort = Get-FreePort
    $qmpPort = Get-FreePort

    $qemuArgs = @(
        "-m", "$($memMb)M", "-smp", "$smpCpus",
        "-cpu", "max",
        "-cdrom", $isoPath, "-boot", "d",
        "-display", "none", "-vga", $Vga,
        "-rtc", "base=utc,clock=host,driftfix=slew",
        "-serial", "tcp:127.0.0.1:$serialPort,server=on,wait=on",
        "-qmp", "tcp:127.0.0.1:$qmpPort,server,nowait",
        "-accel", "tcg,thread=multi"
    )
    if ($Vga -eq "std") { $qemuArgs += @("-global", "VGA.vgamem_mb=64") }

    Write-Host "[3/4] QEMU baslatiliyor (VGA=$Vga, ${memMb}M, ${smpCpus} vCPU)..." -ForegroundColor Yellow

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $qemu
    $psi.Arguments = ($qemuArgs -join " ")
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardError = $true
    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $psi
    [void]$proc.Start()
    $proc.BeginErrorReadLine()

    # Seri konsol (TCP): canlı okuma, kayıpsız yakalama.
    $serialBytes = New-Object System.IO.MemoryStream
    $serialStream = $null
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        try {
            $tcp = New-Object System.Net.Sockets.TcpClient
            $tcp.Connect("127.0.0.1", $serialPort)
            $serialStream = $tcp.GetStream()
            break
        } catch { Start-Sleep -Milliseconds 200 }
    }
    if (-not $serialStream) { Write-Host "[HATA] Seri baglanti kurulamadi." -ForegroundColor Red; exit 1 }

    function Read-Serial {
        try {
            $buf = New-Object byte[] 16384
            while ($script:serialStream.DataAvailable) {
                $n = $script:serialStream.Read($buf, 0, $buf.Length)
                if ($n -le 0) { break }
                $script:serialBytes.Write($buf, 0, $n)
            }
        } catch {}
        return [System.Text.Encoding]::UTF8.GetString($script:serialBytes.ToArray())
    }

    # --- 4. Sonuc bekle ----------------------------------------------------
    Write-Host "[4/4] Performans sonucu bekleniyor..." -ForegroundColor Yellow
    $perfLine = $null
    $start = Get-Date
    while (((Get-Date) - $start).TotalSeconds -lt $TimeoutSeconds) {
        Start-Sleep -Milliseconds 500
        $out = Read-Serial
        if ($out -match "\[PERF\] (.*)") { $perfLine = $Matches[1]; break }
        if ($proc.HasExited) { break }
    }

    $logPath = Join-Path $repoRoot "target\perf_test_serial.log"
    $logDir = Split-Path -Parent $logPath
    if (-not (Test-Path $logDir)) { New-Item -ItemType Directory -Force -Path $logDir | Out-Null }
    [IO.File]::WriteAllText($logPath, (Read-Serial), [System.Text.Encoding]::UTF8)

    # QEMU her zaman kapatılır; aksi halde stderr pipe'ı açık kalır ve
    # çağıran kabuk (pipe) sonsuza kadar bekler.
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue

    if (-not $perfLine) {
        Write-Host "[HATA] [PERF] ciktisi alinamadi." -ForegroundColor Red
        Write-Host "  Seri gunluk: $logPath" -ForegroundColor DarkGray
        exit 1
    }

    Write-Host ""
    Write-Host "---------------- PERFORMANS SONUCU ----------------" -ForegroundColor Cyan
    $fields = @{}
    foreach ($part in ($perfLine -split "\s+")) {
        if ($part -match "^([a-z_]+)=([0-9]+)$") { $fields[$Matches[1]] = [int]$Matches[2] }
    }
    Write-Host ("  Kare hizi         : {0} FPS" -f $fields["fps"]) -ForegroundColor White
    Write-Host ("  Render kapasitesi : {0} FPS" -f $fields["cap_fps"]) -ForegroundColor White
    Write-Host ("  CPU               : {0} Mops/s" -f $fields["cpu_mops"]) -ForegroundColor White
    Write-Host ("  Bellek            : {0} MB/s" -f $fields["mem_mbps"]) -ForegroundColor White
    Write-Host ("  Grafik            : {0} Mpix/s" -f $fields["gfx_mpix"]) -ForegroundColor White
    Write-Host ("  Kare sapmasi      : {0} us" -f $fields["jitter_us"]) -ForegroundColor White
    Write-Host ("  PUAN              : {0} / 100" -f $fields["score"]) -ForegroundColor White
    Write-Host "-----------------------------------------------------" -ForegroundColor Cyan
    Write-Host "  Seri gunluk: $logPath" -ForegroundColor DarkGray

    $score = [int]$fields["score"]
    if ($score -lt $MinScore) {
        Write-Host "[FAIL] Puan $score, beklenen en dusuk $MinScore." -ForegroundColor Red
        exit 1
    }
    Write-Host "[BASARILI] Puan $score >= $MinScore." -ForegroundColor Green
    exit 0
}
finally {
    Restore-Conf
    # Temiz ISO'yu geri uret (perftest belirteci olmadan).
    if (Test-Path (Join-Path $repoRoot "build_iso.ps1")) {
        & powershell -ExecutionPolicy Bypass -File (Join-Path $repoRoot "build_iso.ps1") | Out-Null
        Write-Host "[BILGI] Temiz ISO yeniden uretildi." -ForegroundColor DarkGray
    }
}
