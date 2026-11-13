# File: boot_check.ps1
# minOs ISO'yu bassiz (headless) QEMU'da acar, seri kaydi bir dosyaya yazar.
# Kullanici: powershell -NoProfile -ExecutionPolicy Bypass -File .\boot_check.ps1 -Seconds 60
# Cikti: boot_cow.log (varsayilan) + marker ozeti.
param(
    [int]$Seconds = 60,
    [string]$Log = "boot_cow.log"
)

$ErrorActionPreference = "Continue"
Set-Location (Split-Path -Parent $MyInvocation.MyCommand.Path)

$candidates = @(
    (Join-Path $env:ProgramFiles "qemu\qemu-system-x86_64.exe"),
    "C:\Program Files\qemu\qemu-system-x86_64.exe"
)
$qemu = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $qemu) {
    Write-Host "[HATA] qemu-system-x86_64 bulunamadi." -ForegroundColor Red
    exit 1
}

Remove-Item -LiteralPath $Log -ErrorAction SilentlyContinue
$args = @(
    '-m','512M','-smp','2','-cdrom','minos.iso','-boot','d','-display','none',
    "-serial","file:$Log",'-no-reboot',
    '-device','qemu-xhci','-device','usb-kbd','-device','usb-tablet',
    '-netdev','user,id=n0','-device','e1000,netdev=n0',
    '-device','intel-hda','-device','hda-duplex',
    '-rtc','base=localtime'
)
$proc = Start-Process -FilePath $qemu -ArgumentList $args -PassThru -WindowStyle Hidden
Start-Sleep -Seconds $Seconds
if (-not $proc.HasExited) { $proc.Kill() }
Start-Sleep -Seconds 1

if (-not (Test-Path $Log)) {
    Write-Host "[HATA] Seri log olusmadi." -ForegroundColor Red
    exit 1
}
Write-Host ("[BILGI] {0} -> {1} bayt" -f $Log, (Get-Item $Log).Length) -ForegroundColor Cyan
Get-Content -LiteralPath $Log | Select-String -Pattern 'USER-MEM|COW-TEST|ASLR|SCHED-STAT|PROC-TEST|BASARISIZ|BAŞARISIZ|PANIC|DOUBLE' |
    ForEach-Object { $_.Line }
