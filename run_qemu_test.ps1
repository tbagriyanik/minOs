$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
$qmpPort = 4446

$qemuArgs = @(
    "-m", "8192M",
    "-smp", "8",
    "-cdrom", "minos.iso",
    "-boot", "d",
    "-display", "none",
    "-vga", "std",
    "-global", "VGA.vgamem_mb=256",
    "-rtc", "base=utc,clock=host,driftfix=slew",
    "-serial", "stdio",
    "-qmp", "tcp:127.0.0.1:$qmpPort,server,nowait",
    "-audiodev", "driver=wav,id=snd0,path=target\qemu_audio.wav",
    "-device", "intel-hda",
    "-device", "hda-output,audiodev=snd0",
    "-netdev", "user,id=n0",
    "-device", "e1000,netdev=n0",
    "-device", "qemu-xhci,id=xhci",
    "-device", "usb-kbd,bus=xhci.0",
    "-device", "usb-mouse,bus=xhci.0"
)

$pinfo = New-Object System.Diagnostics.ProcessStartInfo
$pinfo.FileName = $qemu
$pinfo.Arguments = ($qemuArgs -join " ")
$pinfo.RedirectStandardOutput = $true
$pinfo.RedirectStandardError = $true
$pinfo.UseShellExecute = $false
$pinfo.CreateNoWindow = $true
$pinfo.StandardOutputEncoding = [System.Text.Encoding]::UTF8
$pinfo.StandardErrorEncoding = [System.Text.Encoding]::UTF8

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

Start-Sleep -Seconds 25

if (-not $proc.HasExited) {
    $proc.Kill()
    $proc.WaitForExit(1000) | Out-Null
}

Start-Sleep -Milliseconds 500

Unregister-Event -SourceIdentifier $outEvent.Name -ErrorAction SilentlyContinue

$logOutput = $outputBuilder.ToString()
Set-Content -Path "target\qemu_manual_test.log" -Value $logOutput -Encoding utf8

Write-Host "=== QEMU OUTPUT ==="
Write-Host $logOutput
