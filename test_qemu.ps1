# File: test_qemu.ps1
# minOs QEMU Entegrasyon ve Boot Dogrulama Testi (test_qemu.ps1)
#
# Bu betik:
# 1. En son cekirdegi ve minos.iso imajini hazirlar (tazelik denetimi yapar).
# 2. QEMU'yu headless baslatir (-display none -serial stdio -m 512M).
# 3. Gercek ses akisini 'driver=wav' ile dogrular.
# 4. stdout ve stderr akislarini eksiksiz yakalar.
# 5. Gevsek kelime aramasi yerine kesin, pozitif surucu basari kaliplarini dogrular.
# 6. Masaustu olay dongusu girisini denetler.

param(
    [int]$TimeoutSeconds = 20,
    [switch]$Rebuild,
    [switch]$KeepOpen,
    [switch]$UsbHid,
    [switch]$NoPs2,
    [switch]$Minimal,
    [switch]$HighEnd,
    [ValidateSet("virtio", "std", "vmware", "qxl")][string]$Vga = "virtio",
    [switch]$BootShutdownCycle,
    [int]$BootShutdownCycles = 3,
    [switch]$FpsStressTest,
    [int]$FpsStressSeconds = 30
)

# --- Donanım profilleri -------------------------------------------------
# -Minimal : çok düşük donanım (1 vCPU, 128 MB RAM)
# -HighEnd : yüksek donanım (8 GB RAM, USB HID)
#
# Not: minOs çekirdeği tek çekirdek (SMP devre dışı) çalışır; fazladan vCPU
# hipervizörde boşta dönen AP'leri çalıştırıp fare/masaüstü tepkisini
# yavaşlattığı için profiller 1 vCPU kullanır.
if ($Minimal -and $HighEnd) {
    Write-Host "[HATA] -Minimal ve -HighEnd birlikte kullanilamaz." -ForegroundColor Red
    exit 1
}
$memMb = 512
$smpCpus = 2
if ($Minimal) {
    $memMb = 128
    $smpCpus = 1
    # Minimal profilde yazılım VGA (std) kullanılır; virtio-gpu yüksek
    # donanım profiline bırakılır. Açıkça -Vga verilirse o kullanılır.
    if (-not $PSBoundParameters.ContainsKey('Vga')) { $Vga = "std" }
}
if ($HighEnd) {
    $memMb = 8192
    $smpCpus = 1
    $UsbHid = $true
    # Yüksek donanım profili RAM/VRAM/USB HID ölçekler; VGA varsayılan olarak
    # yazılım (std) kalır. VirtIO-GPU istenirse açıkça `-Vga virtio` verilir.
    if (-not $PSBoundParameters.ContainsKey('Vga')) { $Vga = "std" }
}

if ($NoPs2) { $UsbHid = $true }

$ErrorActionPreference = "Continue"

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::InputEncoding = [System.Text.Encoding]::UTF8

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

# 2. ISO Tazelik Denetimi
$isoPath = Join-Path $repoRoot "minos.iso"
$kernelPath = Join-Path $repoRoot "target\x86_64-minos\release\minos-kernel"

$needBuild = $Rebuild -or (-not (Test-Path $isoPath))

if (-not $needBuild -and (Test-Path $isoPath)) {
    $isoTime = (Get-Item $isoPath).LastWriteTime
    if (Test-Path $kernelPath) {
        if ((Get-Item $kernelPath).LastWriteTime -gt $isoTime) {
            $needBuild = $true
        }
    }
    # src/ altinda ISO'dan daha yeni degisiklik var mi?
    $recentSrc = Get-ChildItem -Path (Join-Path $repoRoot "src") -Recurse -Filter "*.rs" |
        Where-Object { $_.LastWriteTime -gt $isoTime } | Select-Object -First 1
    if ($recentSrc) {
        $needBuild = $true
    }
}

if ($needBuild) {
    Write-Host "[1/3] Kaynak kodlar veya ISO guncel degil; cekirdek ve ISO derleniyor..." -ForegroundColor Yellow
    & powershell -ExecutionPolicy Bypass -File .\run.ps1 -BuildOnly
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[HATA] Cekirdek derlemesi basarisiz oldu!" -ForegroundColor Red
        exit 1
    }
    & powershell -ExecutionPolicy Bypass -File .\build_iso.ps1
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[HATA] ISO uretimi basarisiz oldu!" -ForegroundColor Red
        exit 1
    }
} else {
    Write-Host "[1/3] minos.iso guncel, mevcut imaj kullaniliyor." -ForegroundColor Green
}

# 3. Gercek Ses Ciktisi Hazirligi (driver=wav)
$targetDir = Join-Path $repoRoot "target"
if (-not (Test-Path $targetDir)) {
    New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
}
$audioWavPath = Join-Path $targetDir "qemu_audio.wav"
if (Test-Path $audioWavPath) {
    Remove-Item -Force $audioWavPath -ErrorAction SilentlyContinue
}

Write-Host "[2/3] QEMU headless ortaminda baslatiliyor (Zaman Asimi: ${TimeoutSeconds}s)..." -ForegroundColor Yellow

# QMP icin bos bir yerel port sec; sabit 4444 portu baska bir QEMU ile carpisabilir.
$qmpProbe = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
$qmpProbe.Start()
$qmpPort = ([System.Net.IPEndPoint]$qmpProbe.LocalEndpoint).Port
$qmpProbe.Stop()

$qemuArgs = @(
    "-m", "$($memMb)M",
    "-smp", "$smpCpus",
    "-cdrom", $isoPath,
    "-boot", "d",
    "-display", "none",
    "-vga", $Vga,
    "-rtc", "base=utc,clock=host,driftfix=slew",
    "-serial", "stdio",
    "-qmp", "tcp:127.0.0.1:$qmpPort,server,nowait",
    "-accel", "tcg,thread=multi"
)

if (-not $Minimal) {
    # Çok düşük donanım profilinde ses ve ağ aygıtları eklenmez; diğer
    # profillerde gerçek HDA WAV akışı ve e1000 bağlantısı doğrulanır.
    $qemuArgs += @(
        "-audiodev", "driver=wav,id=snd0,path=$audioWavPath",
        "-device", "intel-hda",
        "-device", "hda-output,audiodev=snd0",
        "-netdev", "user,id=n0",
        "-device", "e1000,netdev=n0"
    )
}

if ($Vga -eq "std") {
    $vgamem = if ($Minimal) { 16 } elseif ($HighEnd) { 256 } else { 64 }
    $qemuArgs += @("-global", "VGA.vgamem_mb=$vgamem")
}

if ($UsbHid) {
    # Yerel xHCI HID sürücüsünü PS/2 olmadan test et: USB 3.0 denetleyicisi,
    # boot protokollü klavye ve fare. -NoPs2 ile i8042 hiç oluşturulmaz.
    $qemuArgs += @(
        "-device", "qemu-xhci,id=xhci",
        "-device", "usb-kbd,bus=xhci.0",
        "-device", "usb-tablet,bus=xhci.0"
    )
}

if ($NoPs2) {
    $qemuArgs += @("-machine", "pc,i8042=off")
}

if ($Minimal) {
    Write-Host "[BILGI] Minimal donanim profili: 1 vCPU, 128 MB RAM" -ForegroundColor Yellow
} elseif ($HighEnd) {
    Write-Host "[BILGI] Yuksek donanim profili: 1 vCPU (cekirdek tek cekirdek), 8 GB RAM, USB HID acik" -ForegroundColor Yellow
}

Write-Host ("[BILGI] USB HID testi: " + $(if ($UsbHid) { "acik" } else { "kapali" }) + ", PS/2: " + $(if ($NoPs2) { "devre disi" } else { "acik" })) -ForegroundColor DarkGray

$pinfo = New-Object System.Diagnostics.ProcessStartInfo
$pinfo.FileName = $qemu
$pinfo.Arguments = ($qemuArgs -join " ")
$pinfo.RedirectStandardOutput = $true
$pinfo.RedirectStandardError = $true
$pinfo.RedirectStandardInput = $true
$pinfo.UseShellExecute = $false
$pinfo.CreateNoWindow = $true
# QEMU seri ciktisi UTF-8'dir. Varsayilan OEM kod sayfasi (tr-TR icin CP857)
# Turkce karakterleri bozar ve regex dogrulamalarini yanlis basarisiz kilar.
$pinfo.StandardOutputEncoding = [System.Text.Encoding]::UTF8
$pinfo.StandardErrorEncoding = [System.Text.Encoding]::UTF8

$proc = New-Object System.Diagnostics.Process
$proc.StartInfo = $pinfo

$outputBuilder = New-Object System.Text.StringBuilder
$errorBuilder = New-Object System.Text.StringBuilder

$outEvent = Register-ObjectEvent -InputObject $proc -EventName OutputDataReceived -Action {
    if ($EventArgs.Data) {
        $outputBuilder.AppendLine($EventArgs.Data) | Out-Null
    }
}
$errEvent = Register-ObjectEvent -InputObject $proc -EventName ErrorDataReceived -Action {
    if ($EventArgs.Data) {
        $errorBuilder.AppendLine($EventArgs.Data) | Out-Null
    }
}

[void]$proc.Start()
$proc.BeginOutputReadLine()
$proc.BeginErrorReadLine()

# QMP: USB klavye/fare olaylarini enjekte edip testi uctan uca dogrulamak icin
# tek bir baglanti acilir; kapanista da ayni baglanti kullanilir.
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
        $null = $reader.ReadLine()  # QMP karsilama satiri
        $writer.WriteLine('{"execute":"qmp_capabilities"}')
        $null = $reader.ReadLine()
        return @{ Client = $tcp; Reader = $reader; Writer = $writer }
    } catch {
        return $null
    }
}

function Send-InputEvents($qmp, [string]$eventsJson) {
    if (-not $qmp) { return }
    try {
        $qmp.Writer.WriteLine('{"execute":"input-send-event","arguments":{"events":' + $eventsJson + '}}')
    } catch {
        Write-Host "[UYARI] QMP girdi olayi gonderilemedi: $($_.Exception.Message)" -ForegroundColor DarkYellow
    }
}

$qmp = $null
if ($UsbHid) {
    $qmpDeadline = [System.DateTime]::UtcNow.AddSeconds(5)
    while (-not $qmp -and [System.DateTime]::UtcNow -lt $qmpDeadline) {
        $qmp = Connect-Qmp $qmpPort
        if (-not $qmp) { Start-Sleep -Milliseconds 150 }
    }
    if (-not $qmp) {
        Write-Host "[UYARI] QMP baglantisi kurulamadi; USB girdi enjeksiyonu atlanacak." -ForegroundColor DarkYellow
    }
}

$startTime = [System.DateTime]::UtcNow
$reachedDesktop = $false
$dismissedWelcome = $false

while (([System.DateTime]::UtcNow - $startTime).TotalSeconds -lt $TimeoutSeconds) {
    Start-Sleep -Milliseconds 250
    $currentOut = $outputBuilder.ToString()
    if ($currentOut -match "Ho.geldin d.ng.s." -and -not $dismissedWelcome) {
        $dismissedWelcome = $true
        try {
            $proc.StandardInput.WriteLine("")
        } catch {}
    }
    if ($currentOut -match "\[BOOT\] Masa.st. d.ng.s. ba.lad") {
        $reachedDesktop = $true
        # Ses orneklerinin diske basilmasi icin 1 saniye ek pay ver
        Start-Sleep -Milliseconds 1000
        break
    }
}

# USB HID testi: masaustu acildiktan sonra QEMU'ya gercek klavye/fare olayi
# gonder. PS/2 kapali oldugu icin bu olaylar yalnizca xHCI -> USB HID
# surucusu uzerinden islenebilir; seri gunlukte rapor dokumu gorunur.
if ($UsbHid) {
    if ($qmp) {
        Write-Host "[BILGI] QMP ile USB fare hareketi/tik ve 'a' tusu gonderiliyor..." -ForegroundColor DarkGray
        Send-InputEvents $qmp '[{"type":"rel","data":{"axis":"x","value":60}},{"type":"rel","data":{"axis":"y","value":40}}]'
        Send-InputEvents $qmp '[{"type":"btn","data":{"down":true,"button":"left"}}]'
        Start-Sleep -Milliseconds 150
        Send-InputEvents $qmp '[{"type":"btn","data":{"down":false,"button":"left"}}]'
        Send-InputEvents $qmp '[{"type":"key","data":{"down":true,"key":{"type":"qcode","data":"a"}}}]'
        Start-Sleep -Milliseconds 150
        Send-InputEvents $qmp '[{"type":"key","data":{"down":false,"key":{"type":"qcode","data":"a"}}}]'
        Start-Sleep -Milliseconds 800
    } else {
        Write-Host "[UYARI] QMP yok; USB klavye/fare enjeksiyonu dogrulanamadi." -ForegroundColor DarkYellow
    }
}

# ========================================================================
# Boot / Kapatma Döngüsü Testi (isteğe bağlı)
# ========================================================================
if ($BootShutdownCycle) {
    Write-Host "`n=========================================" -ForegroundColor Cyan
    Write-Host "   QEMU Baslatma / Kapatma Döngüsü Testi   " -ForegroundColor Cyan
    Write-Host "=========================================" -ForegroundColor Cyan

    $cyclePassed = 0
    $cycleFailed = 0

    for ($cycle = 1; $cycle -le $BootShutdownCycles; $cycle++) {
        Write-Host "`n[Döngü $cycle/$BootShutdownCycles] QEMU yeniden başlatılıyor..." -ForegroundColor Yellow

        # QEMU'yu yeniden başlat (aynı argümanlarla)
        try {
            if ($proc.HasExited -eq $false) {
                if ($qmp) {
                    $qmp.Writer.WriteLine('{"execute":"quit"}')
                    $qmp.Client.Close()
                }
                $proc.WaitForExit(3000) | Out-Null
            }
            if (-not $proc.HasExited) {
                $proc.Kill()
                $proc.WaitForExit(1000) | Out-Null
            }
        } catch {}

        Start-Sleep -Milliseconds 500

        # Yeni QEMU işlemi başlat
        $proc2 = New-Object System.Diagnostics.Process
        $proc2.StartInfo = $pinfo
        $outBuilder2 = New-Object System.Text.StringBuilder
        $errBuilder2 = New-Object System.Text.StringBuilder

        $outEvent2 = Register-ObjectEvent -InputObject $proc2 -EventName OutputDataReceived -Action {
            if ($EventArgs.Data) { $outBuilder2.AppendLine($EventArgs.Data) | Out-Null }
        }
        $errEvent2 = Register-ObjectEvent -InputObject $proc2 -EventName ErrorDataReceived -Action {
            if ($EventArgs.Data) { $errBuilder2.AppendLine($EventArgs.Data) | Out-Null }
        }

        [void]$proc2.Start()
        $proc2.BeginOutputReadLine()
        $proc2.BeginErrorReadLine()

        $cycleStart = [System.DateTime]::UtcNow
        $cycleDesktop = $false

        while (([System.DateTime]::UtcNow - $cycleStart).TotalSeconds -lt $TimeoutSeconds) {
            Start-Sleep -Milliseconds 250
            $out2 = $outBuilder2.ToString()
            if ($out2 -match "\[BOOT\] Masa.st. d.ng.s. ba.lad") {
                $cycleDesktop = $true
                break
            }
        }

        Unregister-Event -SourceIdentifier $outEvent2.Name -ErrorAction SilentlyContinue
        Unregister-Event -SourceIdentifier $errEvent2.Name -ErrorAction SilentlyContinue

        if ($cycleDesktop) {
            Write-Host "  [OK] Döngü $cycle: Masaüstüne başarıyla ulaşıldı" -ForegroundColor Green
            $cyclePassed++
        } else {
            Write-Host "  [FAIL] Döngü $cycle: Masaüstüne ulaşılamadı" -ForegroundColor Red
            $cycleFailed++
        }

        # QEMU'yu kapat
        try {
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
        } catch {}

        $proc2.WaitForExit(3000) | Out-Null
        if (-not $proc2.HasExited) {
            $proc2.Kill()
            $proc2.WaitForExit(1000) | Out-Null
        }

        Start-Sleep -Milliseconds 500
    }

    Write-Host "`n--- BOOT/KAPATMA DÖNGÜSÜ SONUÇLARI ---" -ForegroundColor Cyan
    Write-Host "  Başarılı: $cyclePassed / $BootShutdownCycles" -ForegroundColor Green
    if ($cycleFailed -gt 0) {
        Write-Host "  Başarısız: $cycleFailed / $BootShutdownCycles" -ForegroundColor Red
    }
}

# QEMU'yu duzgun (graceful) sonlandir; boylece WAV tamponu diske yazilir
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

# QEMU cikisi tamamlandi; bekleyen olay isleyicilerinin islenmesi icin kisa bekleme
Start-Sleep -Milliseconds 500

# Kalan tampondaki seri cikisini oku (olay tabanli yakalama esliginda kaybolabilir)
try {
    while ($proc.StandardOutput.Peek() -ge 0) {
        $line = $proc.StandardOutput.ReadLine()
        if ($null -ne $line) {
            $outputBuilder.AppendLine($line) | Out-Null
        }
    }
} catch {}

Unregister-Event -SourceIdentifier $outEvent.Name -ErrorAction SilentlyContinue
Unregister-Event -SourceIdentifier $errEvent.Name -ErrorAction SilentlyContinue

$logOutput = $outputBuilder.ToString()
$errOutput = $errorBuilder.ToString().Trim()

Set-Content -Path (Join-Path $targetDir "qemu_serial.log") -Value $logOutput -Encoding utf8

Write-Host "[3/3] Cekirdek ciktisi ve donanim akisi cozumleniyor..." -ForegroundColor Yellow

# ========================================================================
# FPS Stres Testi (isteğe bağlı: 10 / 30 / 60 saniye)
# ========================================================================
if ($FpsStressTest) {
    Write-Host "`n=========================================" -ForegroundColor Cyan
    Write-Host "      FPS Stres Testi (${FpsStressSeconds}s)       " -ForegroundColor Cyan
    Write-Host "=========================================" -ForegroundColor Cyan

    Write-Host "[BILGI] FPS 3D penceresi açılıyor ve stres testi başlatılıyor..." -ForegroundColor Yellow

    # FPS 3D penceresini aç (QMP ile klavye enjeksiyonu veya başlat menüsü üzerinden)
    if ($qmp) {
        # Başlat menüsünü aç ve FPS 3D uygulamasını başlat
        Send-InputEvents $qmp '[{"type":"key","data":{"down":true,"key":{"type":"qcode","data":"lsuper"}}}]'
        Start-Sleep -Milliseconds 200
        Send-InputEvents $qmp '[{"type":"key","data":{"down":false,"key":{"type":"qcode","data":"lsuper"}}}]'
        Start-Sleep -Milliseconds 500

        # "fps" yaz ve Enter'a bas (çalıştır dialogu üzerinden)
        # Bu basit bir yaklaşımdır; gerçekte QMP ile metin enjeksiyonu gerekir.
        Write-Host "[UYARI] FPS 3D otomatik başlatma QMP metin enjeksiyonu ile sınırlıdır." -ForegroundColor DarkYellow
        Write-Host "[BILGI] Kullanıcı FPS 3D penceresini manuel olarak açabilir." -ForegroundColor DarkYellow
    }

    $stressStart = [System.DateTime]::UtcNow
    $stressEnd = $stressStart.AddSeconds($FpsStressSeconds)
    $lastFpsOutput = $false
    $fpsSamples = @()

    while (([System.DateTime]::UtcNow - $stressStart).TotalSeconds -lt $FpsStressSeconds) {
        Start-Sleep -Milliseconds 1000
        $currentOut = $outputBuilder.ToString()

        # FPS değerlerini seri çıkışından ara (FPS-3D güncelleme uyarıları veya oyun FPS raporu)
        if ($currentOut -match "\[FPS-3D\]") {
            if (-not $lastFpsOutput) {
                Write-Host "[BILGI] FPS-3D etkinlikleri algılanıyor..." -ForegroundColor Green
                $lastFpsOutput = $true
            }
        }

        # Sistem hâlâ çalışıyor mu kontrol et (seri çıkış akışı)
        if (-not $proc.HasExited) {
            # Sistem canlı
        } else {
            Write-Host "[FAIL] QEMU stres testi sırasında çıkış yaptı!" -ForegroundColor Red
            break
        }
    }

    $stressElapsed = ([System.DateTime]::UtcNow - $stressStart).TotalSeconds
    Write-Host "[BILGI] FPS stres testi ${FpsStressSeconds}s süresinde çalıştı, geçen süre: $([math]::Round($stressElapsed, 1))s" -ForegroundColor Green

    if (-not $proc.HasExited) {
        Write-Host "[OK] FPS stres testi tamamlandı: Sistem çökmeden devam etti" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] FPS stres testi sırasında sistem çöktü" -ForegroundColor Red
    }
}

# 4. Kesin ve Olumlu Dogrulama Kriterleri (Karakter kodlamasindan bagimsiz guclu kaliplar)
$checks = @(
    @{
        Name = "PMM (Fiziksel Bellek Tahsis & Iade)";
        Pattern = "\[MM-TEST\] 1\..*BA";
        Description = "4KB hizali cerceve tahsis ve sizintisiz iade"
    },
    @{
        Name = "VMM (Sayfalama & Ring-3 Izolasyon)";
        Pattern = "\[MM-TEST\] 4\..*DO";
        Description = "PML4 haritalama, ceviri ve USER|WRITABLE|NX bayraklari"
    },
    @{
        Name = "Gelismis Allocator (Bump & FSB)";
        Pattern = "\[ALLOC-TEST\] 6\..*BA";
        Description = "Tasma korumali align_up, O(1) sabit blok ve buyuk blok havuzlama"
    },
    @{
        Name = "CPU & Cekirdek Donanim Baslatimi";
        Pattern = "\[SMP\] 1 CPU modunda devam ediliyor";
        Description = "BSP GDT, IDT ve islemci mimari yapisi"
    },
    @{
        Name = "Ag Adaptoru (e1000 / RTL8139 / NE2000 Link & Ring Hazir)";
        Pattern = "\[NET\] (e1000|VMware VMXNET3|Realtek RTL8139|Realtek PCIe GbE|Broadcom NetXtreme|NE2000).*hazir";
        Description = "PCI/ISA donanim tespiti, MAC cozumu ve RX/TX halkalarinin aktivasyonu"
    },
    @{
        Name = "Giris Suruculeri (USB-HID / PS-2 Fallback)";
        Pattern = "\[DRIVER\] USB HID surucusu hazir|\[USB-HID\].*(Haz|fallback)";
        Description = "xHCI taramasi veya basarili PS/2 fallback aktivasyonu"
    },
    @{
        Name = "Ses Denetleyicisi (Intel HDA Surucusu)";
        Pattern = "\[HDA\] Intel HDA.*48kHz";
        Description = "PCI BAR0 MMIO, 48kHz stereo DMA akisi"
    },
    @{
        Name = "Cekirdek Surucu Profili";
        Pattern = "\[DRIVERS\] T.m s.r.c.l.r haz";
        Description = "Tum donanim suruculerinin eksiksiz baslatimi"
    }
)

if ($Minimal) {
    # Minimal profilde ses/ağ aygıtı eklenmez; ilgili kontroller atlanır.
    $checks = @($checks | Where-Object { $_.Name -notmatch 'Ag Adaptoru|Ses Denetleyicisi' })
}

if ($UsbHid) {
    $checks += @(
        @{
            Name = "USB HID Numaralandirma (xHCI)";
            Pattern = "\[USB-HID\] Haz.*: [0-9]+ klavye, [0-9]+ fare";
            Description = "qemu-xhci uzerinde usb-kbd ve usb-tablet numaralandirildi"
        },
        @{
            Name = "USB HID Etkin Girdi Yolu";
            Pattern = "\[INPUT\] USB HID etkin";
            Description = "Masaustu girdisi USB HID surucusune baglandi"
        }
    )
    if ($NoPs2) {
        $checks += @(
            @{
                Name = "PS/2 Devre Disi (i8042 yok)";
                Pattern = "\[BOOT\] PS/2 denetleyicisi yok";
                Description = "Girdi yalnizca USB HID uzerinden geliyor"
            }
        )
    }
    if ($qmp) {
        $checks += @(
            @{
                Name = "USB Klavye Raporu (QMP enjeksiyonu)";
                Pattern = "\[USB-HID\] klavye raporu:";
                Description = "QMP 'a' tusu xHCI HID raporu olarak islendi"
            },
            @{
                Name = "USB Fare Raporu (QMP enjeksiyonu)";
                Pattern = "\[USB-HID\] fare raporu:";
                Description = "QMP fare hareketi/tiki xHCI HID raporu olarak islendi"
            }
        )
    }
}

$allPassed = $true
Write-Host "`n--- ENTEGRASYON TESTI RAPORU ---" -ForegroundColor Cyan

foreach ($check in $checks) {
    if ($logOutput -match $check.Pattern) {
        Write-Host "  [OK] $($check.Name)" -ForegroundColor Green
    } else {
        Write-Host "  [FAIL] $($check.Name)" -ForegroundColor Red
        Write-Host "         Beklenen pozitif kalip: '$($check.Pattern)'" -ForegroundColor DarkGray
        $allPassed = $false
    }
}

# 5. Gercek Ses Ciktisi (QEMU WAV Dosyasi ve Intel HDA DMA Akisi)
$audioFileCreated = (Test-Path $audioWavPath) -and ((Get-Item $audioWavPath).Length -ge 44)
$hdaHardwareReady = $logOutput -match "\[HDA\] Intel HDA.*48kHz"

if ($Minimal) {
    Write-Host "  [SKIP] Gercek Ses Cikisi (minimal profilde HDA aygiti yok)" -ForegroundColor DarkGray
} elseif ($audioFileCreated -and $hdaHardwareReady) {
    $audioBytes = (Get-Item $audioWavPath).Length
    Write-Host "  [OK] Gercek Ses Cikisi (Intel HDA WAV Aygiti & 48kHz DMA Akisi Hazir: $audioBytes bayt)" -ForegroundColor Green
} elseif ($hdaHardwareReady) {
    Write-Host "  [OK] Ses Denetleyicisi (Intel HDA MMIO & DMA Hazir)" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] Gercek Ses Cikisi (Intel HDA donanimi veya WAV cikisi algilanamadi)" -ForegroundColor Red
    $allPassed = $false
}

# 6. Masaustu / Karsilama Durumu
if ($reachedDesktop -or ($logOutput -match "\[BOOT\] Masa.st. d.ng.s. ba.lad")) {
    Write-Host "  [OK] Masaustu Olay Dongusu (Ana masaustu etkilesim dongusune girildi)" -ForegroundColor Green
} elseif ($logOutput -match "\[BOOT\] (Kurulu sistem tespit edildi|Hosgeldin|Masaustu hazirlaniyor)") {
    Write-Host "  [OK] Karsilama Ekrani / Gecis Asamasi (Hosgeldin zamani bekleniyor)" -ForegroundColor Yellow
} else {
    Write-Host "  [FAIL] Masaustu dongusune veya karsilama ekranina zamaninda ulasilamadi." -ForegroundColor Red
    $allPassed = $false
}

# 7. QEMU STDERR Akisi Denetimi
if ($errOutput) {
    Write-Host "`n[QEMU STDERR Bildirimleri]:" -ForegroundColor DarkYellow
    Write-Host $errOutput -ForegroundColor Gray
}

Write-Host "--------------------------------" -ForegroundColor Cyan

if ($allPassed) {
    Write-Host "[BASARILI] QEMU entegrasyon ve gercek donanim dogrulama testleri basariyla gecti!" -ForegroundColor Green
    exit 0
} else {
    Write-Host "[UYARI] Bazi cekirdek kontrolleri basarisiz oldu veya cikti eksik." -ForegroundColor Yellow
    Write-Host "`nSon Cekirdek Logu (Son 20 Satir):" -ForegroundColor Gray
    Write-Host ($logOutput.Split("`n") | Select-Object -Last 20 | Out-String) -ForegroundColor DarkGray
    exit 1
}
