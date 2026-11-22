# File: tools/fuzz_desktop.ps1
# minOs masaustu kararsizlik "fuzz" testi:
#   QEMU'yu headless (WHpx) baslatir, QMP uzerinden rastgele fare/klavye
#   olaylari ve uygulama acilisleri gonderir, seri gunlugu [GUARD] / PANIC
#   kaliplari icin tarar.
#
# Kullanim:
#   powershell -ExecutionPolicy Bypass -File .\tools\fuzz_desktop.ps1 -Minutes 15
#
# Cikti: tools\fuzz_log_<zaman>.txt (tam seri gunlugu) + konsol ozeti.

param(
    [int]$Minutes = 15,
    [string]$Vga = "std",
    [int]$VgamemMb = 64,
    [string]$Memory = "512M",
    [ValidateSet("whpx", "tcg")][string]$Accel = "tcg",
    [string]$DiskImage = ".\disk.qcow2",
    [string]$Iso = ".\minos.iso",
    [switch]$Ps2,          # USB yerine PS/2 girisi (run.ps1 varsayilani)
    [switch]$KeepOpen
)

$ErrorActionPreference = "Continue"
chcp 65001 | Out-Null

$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
if (-not (Test-Path $qemu)) { $qemu = "qemu-system-x86_64.exe" }

$stamp = Get-Date -Format "yyyyMMdd_HHmmss"
$logPath = Join-Path $PSScriptRoot "fuzz_log_$stamp.txt"
$serialBytes = New-Object System.IO.MemoryStream
$serialStream = $null
$serialTcp = $null

# --- Bos portlar ---------------------------------------------------------
function Get-FreePort {
    $l = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
    $l.Start(); $p = ([System.Net.IPEndPoint]$l.LocalEndpoint).Port; $l.Stop(); return $p
}
$qmpPort = Get-FreePort
$serialPort = Get-FreePort

# --- QEMU argumanlari ----------------------------------------------------
$qemuArgs = @(
    "-m", $Memory,
    "-smp", "1",
    "-cpu", "max",
    "-cdrom", $Iso,
    "-boot", "d",
    "-display", "none",
    "-rtc", "base=utc,clock=host,driftfix=slew",
    "-serial", "tcp:127.0.0.1:$serialPort,server=on,wait=on",
    "-qmp", "tcp:127.0.0.1:$qmpPort,server,nowait"
)
if ($Accel -eq "whpx") {
    $qemuArgs += @("-accel", "whpx")
} else {
    $qemuArgs += @("-accel", "tcg,thread=multi")
}
$qemuArgs += @("-vga", $Vga, "-global", "VGA.vgamem_mb=$VgamemMb")

if (Test-Path -LiteralPath $DiskImage -PathType Leaf) {
    $fmt = "raw"
    $hdr = [System.IO.File]::ReadAllBytes((Resolve-Path $DiskImage).Path)[0..3]
    if ($hdr[0] -eq 0x51 -and $hdr[1] -eq 0x46 -and $hdr[2] -eq 0x49 -and $hdr[3] -eq 0xFB) { $fmt = "qcow2" }
    $qemuArgs += @("-drive", "file=$DiskImage,format=$fmt,if=ide")
}
$qemuArgs += @("-netdev", "user,id=n0", "-device", "e1000,netdev=n0")

if ($Ps2) {
    $qemuArgs += @("-device", "piix3-ide")  # varsayilan pc makinesi zaten icerir; no-op
} else {
    $qemuArgs += @(
        "-machine", "pc,i8042=off",
        "-device", "qemu-xhci,id=xhci",
        "-device", "usb-kbd,bus=xhci.0",
        "-device", "usb-tablet,bus=xhci.0"
    )
}

Write-Host "[Fuzz] QEMU baslatiliyor: $($qemuArgs -join ' ')" -ForegroundColor Cyan

$pinfo = New-Object System.Diagnostics.ProcessStartInfo
$pinfo.FileName = $qemu
$pinfo.Arguments = ($qemuArgs -join " ")
$pinfo.UseShellExecute = $false
$pinfo.CreateNoWindow = $true
$pinfo.RedirectStandardError = $true
$pinfo.StandardErrorEncoding = [System.Text.Encoding]::UTF8
$proc = New-Object System.Diagnostics.Process
$proc.StartInfo = $pinfo
$errBuilder = New-Object System.Text.StringBuilder
Register-ObjectEvent -InputObject $proc -EventName ErrorDataReceived -Action {
    if ($EventArgs.Data) { $errBuilder.AppendLine($EventArgs.Data) | Out-Null }
} | Out-Null
[void]$proc.Start()
$proc.BeginErrorReadLine()

# --- Seri okuma ----------------------------------------------------------
function Connect-Tcp([int]$port, [int]$timeoutSec = 20) {
    $deadline = [System.DateTime]::UtcNow.AddSeconds($timeoutSec)
    while ([System.DateTime]::UtcNow -lt $deadline) {
        try {
            $t = New-Object System.Net.Sockets.TcpClient
            $t.Connect("127.0.0.1", $port)
            return $t
        } catch { Start-Sleep -Milliseconds 150 }
    }
    return $null
}

function Read-SerialAll {
    if ($script:serialStream) {
        try {
            $buf = New-Object byte[] 16384
            while ($script:serialStream.DataAvailable) {
                $n = $script:serialStream.Read($buf, 0, $buf.Length)
                if ($n -le 0) { break }
                $script:serialBytes.Write($buf, 0, $n)
            }
        } catch {}
    }
    return [System.Text.Encoding]::UTF8.GetString($script:serialBytes.ToArray())
}

function Flush-SerialLog {
    $txt = Read-SerialAll
    if ($txt.Length -gt 0) {
        [System.IO.File]::AppendAllText($logPath, $txt, [System.Text.Encoding]::UTF8)
    }
}

$serialTcp = Connect-Tcp $serialPort 20
if ($serialTcp) { $serialStream = $serialTcp.GetStream() } else {
    Write-Host "[HATA] Seri port baglanamadi." -ForegroundColor Red; exit 1
}

# --- QMP -----------------------------------------------------------------
function Connect-Qmp {
    try {
        $tcp = New-Object System.Net.Sockets.TcpClient
        $tcp.Connect("127.0.0.1", $qmpPort)
        $stream = $tcp.GetStream(); $stream.ReadTimeout = 3000
        $reader = New-Object System.IO.StreamReader($stream)
        $writer = New-Object System.IO.StreamWriter($stream)
        $writer.NewLine = "`n"; $writer.AutoFlush = $true
        $null = $reader.ReadLine()
        $writer.WriteLine('{"execute":"qmp_capabilities"}')
        $null = $reader.ReadLine()
        return @{ Client = $tcp; Reader = $reader; Writer = $writer }
    } catch { return $null }
}

function Send-Events($qmp, [string]$eventsJson) {
    if (-not $qmp) { return }
    try { $qmp.Writer.WriteLine('{"execute":"input-send-event","arguments":{"events":' + $eventsJson + '}}') }
    catch {}
}

function Send-KeyTap($qmp, [string]$qcode, [int]$holdMs = 60) {
    Send-Events $qmp ('[{"type":"key","data":{"down":true,"key":{"type":"qcode","data":"' + $qcode + '"}}}]')
    Start-Sleep -Milliseconds $holdMs
    Send-Events $qmp ('[{"type":"key","data":{"down":false,"key":{"type":"qcode","data":"' + $qcode + '"}}}]')
}

function Send-AbsMove($qmp, [int]$x, [int]$y) {
    # usb-tablet: 0..32767
    Send-Events $qmp ('[{"type":"abs","data":{"axis":"x","value":' + $x + '}},{"type":"abs","data":{"axis":"y","value":' + $y + '}}]')
}

function Send-Click($qmp, [string]$button = "left") {
    Send-Events $qmp ('[{"type":"btn","data":{"down":true,"button":"' + $button + '"}}]')
    Start-Sleep -Milliseconds ([System.Random]::new().Next(40, 110))
    Send-Events $qmp ('[{"type":"btn","data":{"down":false,"button":"' + $button + '"}}]')
}

# --- Masaustu bekle -------------------------------------------------------
Write-Host "[Fuzz] Masaustu bekleniyor..." -ForegroundColor Yellow
$deadline = [System.DateTime]::UtcNow.AddSeconds(120)
$reached = $false
while ([System.DateTime]::UtcNow -lt $deadline) {
    if ($proc.HasExited) { break }
    $out = Read-SerialAll
    if ($out -match "\[BOOT\] Desktop loop started") { $reached = $true; break }
    Start-Sleep -Milliseconds 300
}
if (-not $reached) {
    Flush-SerialLog
    Write-Host "[HATA] Masaustu baslamadi veya QEMU cikti. Log: $logPath" -ForegroundColor Red
    if (-not $KeepOpen -and -not $proc.HasExited) { $proc.Kill() }
    exit 1
}
Write-Host "[Fuzz] Masaustu hazir. Fuzzing ${Minutes} dk basliyor..." -ForegroundColor Green

$qmp = $null
$qmpDeadline = [System.DateTime]::UtcNow.AddSeconds(10)
while (-not $qmp -and [System.DateTime]::UtcNow -lt $qmpDeadline) {
    $qmp = Connect-Qmp
    if (-not $qmp) { Start-Sleep -Milliseconds 200 }
}
if (-not $qmp) { Write-Host "[UYARI] QMP yok; sadece seri izleniyor." -ForegroundColor DarkYellow }

# --- Fuzz dongusu --------------------------------------------------------
$rng = New-Object System.Random
$keys = @("a","b","c","d","e","f","g","h","i","j","k","l","m","n","o","p","q","r","s","t","u","v","w","x","y","z",
          "0","1","2","3","4","5","6","7","8","9","ret","esc","tab","spc","backspace",
          "up","down","left","right","f1","f2","f3","f4","f5","f6","delete","home","end","pgup","pgdn")
$appQueries = @("not", "hesap", "dosya", "ayar", "term", "müzik", "görev", "paint", "boya", "medya", "video", "ayarlar")

$endTime = [System.DateTime]::UtcNow.AddMinutes($Minutes)
$iter = 0
$guardHits = 0
$panicHits = 0
$lastFlush = [System.DateTime]::UtcNow

while ([System.DateTime]::UtcNow -lt $endTime) {
    $iter++
    if ($proc.HasExited) { Write-Host "`n[Fuzz] QEMU kendiliginden cikti!" -ForegroundColor Red; break }

    # Periyodik seri tarama
    if (([System.DateTime]::UtcNow - $lastFlush).TotalSeconds -ge 5) {
        Flush-SerialLog
        $lastFlush = [System.DateTime]::UtcNow
    }
    $snap = Read-SerialAll
    $guardHits += ([regex]::Matches($snap, "\[GUARD\]")).Count
    $panicHits += ([regex]::Matches($snap, "KERNEL PANIC|\[PANIC\]")).Count
    if ($panicHits -gt 0) { Write-Host "`n[Fuzz] PANIC tespit edildi!" -ForegroundColor Red; break }

    if (-not $qmp) { Start-Sleep -Milliseconds 500; continue }

    $action = $rng.Next(100)
    try {
        if ($action -lt 35) {
            # Rastgele imlec hareketi (birkac ara adimla)
            $tx = $rng.Next(200, 32000); $ty = $rng.Next(200, 20000)
            $steps = $rng.Next(2, 6)
            for ($s = 0; $s -lt $steps; $s++) {
                Send-AbsMove $qmp ([int]($tx * ($s+1) / $steps)) ([int]($ty * ($s+1) / $steps))
                Start-Sleep -Milliseconds $rng.Next(15, 60)
            }
        } elseif ($action -lt 60) {
            # Tiklama / surukleme
            $x = $rng.Next(200, 32000); $y = $rng.Next(200, 20000)
            Send-AbsMove $qmp $x $y
            Start-Sleep -Milliseconds $rng.Next(50, 200)
            $btn = @("left","left","left","right","middle")[$rng.Next(5)]
            if ($rng.Next(100) -lt 20) {
                # surukleme: basili tut + hareket + birak
                Send-Events $qmp ('[{"type":"btn","data":{"down":true,"button":"' + $btn + '"}}]')
                for ($s = 0; $s -lt $rng.Next(3, 8); $s++) {
                    Send-AbsMove $qmp ($x + $rng.Next(-4000, 4000)) ($y + $rng.Next(-3000, 3000))
                    Start-Sleep -Milliseconds $rng.Next(20, 80)
                }
                Send-Events $qmp ('[{"type":"btn","data":{"down":false,"button":"' + $btn + '"}}]')
            } else {
                Send-Click $qmp $btn
                if ($rng.Next(100) -lt 25) { Start-Sleep -Milliseconds $rng.Next(60, 140); Send-Click $qmp $btn } # cift tik
            }
        } elseif ($action -lt 80) {
            # Klavye
            $k = $keys[$rng.Next($keys.Count)]
            if ($rng.Next(100) -lt 10) {
                Send-Events $qmp ('[{"type":"key","data":{"down":true,"key":{"type":"qcode","data":"ctrl"}}},{"type":"key","data":{"down":true,"key":{"type":"qcode","data":"' + $k + '"}}}]')
                Start-Sleep -Milliseconds 70
                Send-Events $qmp ('[{"type":"key","data":{"down":false,"key":{"type":"qcode","data":"' + $k + '"}}},{"type":"key","data":{"down":false,"key":{"type":"qcode","data":"ctrl"}}}]')
            } else {
                Send-KeyTap $qmp $k
            }
        } elseif ($action -lt 92) {
            # Baslat menusu + uygulama arama + ac
            Send-KeyTap $qmp "lsuper" 120
            Start-Sleep -Milliseconds $rng.Next(300, 700)
            $q = $appQueries[$rng.Next($appQueries.Count)]
            foreach ($ch in $q.ToCharArray()) {
                $kc = if ($ch -match '[a-zçğıöşü]') { [string]$ch } else { "?" }
                if ($ch -eq 'ç') { $kc = "c" } elseif ($ch -eq 'ğ') { $kc = "g" }
                elseif ($ch -eq 'ı') { $kc = "i" } elseif ($ch -eq 'ö') { $kc = "o" }
                elseif ($ch -eq 'ş') { $kc = "s" } elseif ($ch -eq 'ü') { $kc = "u" }
                elseif ($ch -eq ' ') { $kc = "spc" }
                if ($kc -ne "?") { Send-KeyTap $qmp $kc 45; Start-Sleep -Milliseconds 40 }
            }
            Start-Sleep -Milliseconds $rng.Next(200, 600)
            Send-KeyTap $qmp "ret" 80
            Start-Sleep -Milliseconds $rng.Next(800, 2000)
        } else {
            # Fare tekerlegi (varsa) + uzun bekleme
            Send-Events $qmp '[{"type":"btn","data":{"down":true,"button":"wheel"}},{"type":"btn","data":{"down":false,"button":"wheel"}}]'
            Start-Sleep -Milliseconds $rng.Next(200, 800)
        }
    } catch {}

    Start-Sleep -Milliseconds $rng.Next(60, 260)
}

Flush-SerialLog
$snap = Read-SerialAll
$guardHits += ([regex]::Matches($snap, "\[GUARD\]")).Count
$panicHits += ([regex]::Matches($snap, "KERNEL PANIC|\[PANIC\]")).Count

Write-Host "`n[Fuzz] Tamamlandi: $iter iterasyon, $guardHits guard olayi, $panicHits panic." -ForegroundColor Cyan
Write-Host "[Fuzz] Seri gunlugu: $logPath" -ForegroundColor Cyan

if (-not $KeepOpen -and -not $proc.HasExited) { $proc.Kill() }
if ($proc.HasExited -or $panicHits -gt 0 -or $guardHits -gt 0) { exit 2 } else { exit 0 }
