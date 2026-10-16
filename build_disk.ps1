# minOs Disk Hazirlama Betigi (build_disk.ps1)
# MBR + 1MiB hizali FAT32 bolumu, Limine BIOS boot, kernel + VFS agaci.
param(
    [string]$DiskImage = ".\disk.raw",
    [ValidateRange(64, 4096)][int]$DiskSizeMB = 256,
    [string]$MarkerFile = ".\disk.prepared",
    [switch]$Force
)

$ErrorActionPreference = "Stop"

$PSScriptRoot2 = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $PSScriptRoot2

$qemuImg = "C:\Program Files\qemu\qemu-img.exe"
$limine  = Join-Path $PSScriptRoot2 "tools\limine.exe"
$kernel  = Join-Path $PSScriptRoot2 "target\x86_64-minos\release\minos-kernel"

function Test-DiskInstalled([string]$rawPath) {
    # Ham goruntude 11 baytlik kisa ad "MINOS   SYS" tara.
    $needle = [Text.Encoding]::ASCII.GetBytes("MINOS   SYS")
    $fs = [IO.File]::OpenRead($rawPath)
    try {
        $buf = New-Object byte[] (1MB)
        while (($n = $fs.Read($buf, 0, $buf.Length)) -gt 0) {
            for ($i = 0; $i -le $n - $needle.Length; $i++) {
                $hit = $true
                for ($j = 0; $j -lt $needle.Length; $j++) {
                    if ($buf[$i + $j] -ne $needle[$j]) { $hit = $false; break }
                }
                if ($hit) { return $true }
            }
        }
        return $false
    } finally { $fs.Close() }
}

if (-not (Test-Path $qemuImg)) { Write-Host "[HATA] qemu-img bulunamadi: $qemuImg" -ForegroundColor Red; exit 1 }
if (-not (Test-Path $limine))  { Write-Host "[HATA] limine.exe bulunamadi." -ForegroundColor Red; exit 1 }
if (-not (Test-Path $kernel))  { Write-Host "[HATA] kernel derlenmemis: $kernel" -ForegroundColor Red; exit 1 }

$needPrep = $Force -or -not (Test-Path $DiskImage) -or -not (Test-Path $MarkerFile)
if (-not $needPrep) {
    Write-Host "[BILGI] Disk hazir ($MarkerFile var). Hazirlik atlaniyor." -ForegroundColor Cyan
    exit 0
}
if ((Test-Path $DiskImage) -and (Test-Path $MarkerFile) -and -not $Force) {
    Write-Host "[BILGI] Disk hazir, dokunulmuyor." -ForegroundColor Cyan
    exit 0
}

Write-Host "[1/6] Ham goruntu olusturuluyor ($DiskSizeMB MB)..." -ForegroundColor Yellow
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("minos-disk-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
$vhd = Join-Path $tmp "minos.vhd"

# Bos vir disk VHD olarak olustur; diskpart ile bolumle+formatla.
$used = (Get-PSDrive -PSProvider FileSystem).Name
$letter = $null
foreach ($c in [char[]]'MNOPQRSTUVWXYZIJKLABCDEFGH') { if ($used -notcontains [string]$c) { $letter = $c; break } }
if (-not $letter) { Write-Host "[HATA] Bos surucu harfi yok." -ForegroundColor Red; exit 1 }

$dp = @"
create vdisk file="$vhd" maximum=$DiskSizeMB type=fixed
select vdisk file="$vhd"
attach vdisk
create partition primary align=1024
active
format fs=fat32 quick label=MINOS
assign letter=$letter
"@
Set-Content -Path (Join-Path $tmp "create.txt") -Value $dp -Encoding ascii
& diskpart /s (Join-Path $tmp "create.txt") | Out-Null
Start-Sleep -Seconds 2
$root = "${letter}:\"
if (-not (Test-Path $root)) { Write-Host "[HATA] Bolum baglanamadi ($letter)." -ForegroundColor Red; exit 1 }

Write-Host "[2/6] VFS agaci yaziliyor..." -ForegroundColor Yellow
$dirs = @(
    "boot\limine", "root\Documents", "root\Media", "root\Downloads",
    "root\Trash", "etc", "var\log", "media\usb0", "media\hdd0", "media\cdrom"
)
foreach ($d in $dirs) { New-Item -ItemType Directory -Force -Path (Join-Path $root $d) | Out-Null }

# Boot dosyalari
Copy-Item -Force "boot\limine.conf" (Join-Path $root "limine.conf")
Copy-Item -Force "boot\limine.conf" (Join-Path $root "boot\limine\limine.conf")
Copy-Item -Force "boot\limine\limine-bios.sys" (Join-Path $root "limine-bios.sys")
Copy-Item -Force "boot\limine\limine-bios.sys" (Join-Path $root "boot\limine\limine-bios.sys")
Copy-Item -Force $kernel (Join-Path $root "boot\minos-kernel")
Copy-Item -Force $kernel (Join-Path $root "minos-kernel")

# VFS varsayilan icerigi (vfs.rs init_default_hierarchy ile ayni)
$enc = [Text.Encoding]::ASCII
[IO.File]::WriteAllBytes((Join-Path $root "root\Documents\notlar.txt"), $enc.GetBytes("minOs Not Defteri`n================`nBu dosya dogrudan VFS ve disk uzerinde saklanmaktadir.`nMetin duzenleyip [Kaydet] butonuyla kalici hale getirebilirsiniz.`n"))
[IO.File]::WriteAllBytes((Join-Path $root "root\Documents\readme.txt"), $enc.GetBytes("minOs x86_64 Sarman Edition v2.0`n--------------------------------`n* Hizli ve hafif mikrocekirdek mimarisi`n* Gelismis dosya yoneticisi ve klasor destegi`n* USB, HDD ve CD-ROM suruculeri aktif`n"))
[IO.File]::WriteAllBytes((Join-Path $root "root\Documents\proje_fikirleri.txt"), $enc.GetBytes("Gelecek Surum Planlari:`n1. FAT32 kalici dizin agaci gelistirmesi`n2. TCP/IP istemci socket API`n3. Coklu gorev zamanlayici`n"))
[IO.File]::WriteAllBytes((Join-Path $root "root\Documents\sekiller.3d"), $enc.GetBytes("# minOs 3D Sahne Testi`nshape=cube`nauto_rotate=true`nwireframe=false`n"))
[IO.File]::WriteAllBytes((Join-Path $root "root\Documents\kup.3d"), $enc.GetBytes("# minOs 3D Kup Modeli`nshape=cube`nauto_rotate=true`nwireframe=false`n"))
[IO.File]::WriteAllBytes((Join-Path $root "root\Documents\silindir.3d"), $enc.GetBytes("# minOs 3D Silindir Modeli`nshape=cylinder`nauto_rotate=true`nwireframe=false`n"))
[IO.File]::WriteAllBytes((Join-Path $root "root\Documents\kure.3d"), $enc.GetBytes("# minOs 3D Kure Modeli`nshape=sphere`nauto_rotate=true`nwireframe=false`n"))
[IO.File]::WriteAllBytes((Join-Path $root "etc\settings.cfg"), $enc.GetBytes("theme=velvet;volume=80;keyboard=tr;language=tr;res=1280x720;"))
[IO.File]::WriteAllBytes((Join-Path $root "var\log\kernel.log"), $enc.GetBytes("[minOs] Kernel v2.0 online.`n[Storage] ATA 4 channels initialized.`n[VFS] Virtual File System mounted.`n[GUI] Compositor active.`n"))
[IO.File]::WriteAllBytes((Join-Path $root "boot\limine.conf"), $enc.GetBytes("timeout: 5`n`n/minOs Live Environment`n    protocol: limine`n    kernel_path: boot():/boot/minos-kernel`n    kernel_cmdline: live`n    resolution: 1280x720x32`n`n/minOs Installed System`n    protocol: limine`n    kernel_path: boot():/boot/minos-kernel`n    kernel_cmdline: installed`n    resolution: 1280x720x32`n"))
[IO.File]::WriteAllBytes((Join-Path $root "root\Media\kedi_maskotu.bmp"), ([byte[]]@(0x42,0x4D,0x36,0,0,0,0,0,0,0,0x36,0,0,0,0x28,0,0,0)))
[IO.File]::WriteAllBytes((Join-Path $root "root\Media\lofi_ring0.wav"), ([byte[]]@(0x52,0x49,0x46,0x46,0x24,0,0,0,0x57,0x41,0x56,0x45,0x66,0x6D,0x74,0x20,0x10,0,0,0,1,0,2,0,0x44,0xAC,0,0)))
[IO.File]::WriteAllBytes((Join-Path $root "root\Media\retro_drive.mp4"), ([byte[]]@(0,0,0,0x20,0x66,0x74,0x79,0x70,0x69,0x73,0x6F,0x6D,0,0,2,0,0x69,0x73,0x6F,0x6D,0x69,0x73,0x6F,0x32,0x61,0x76,0x63,0x31,0x6D,0x70,0x34,0x31)))
[IO.File]::WriteAllBytes((Join-Path $root "BENIOKU.TXT"), $enc.GetBytes("minOs Kurulumu Basariyla Tamamlandi!`n====================================`nBu bolumde tam okuma ve yazma hakkiniz bulunmaktadir.`n"))
[IO.File]::WriteAllBytes((Join-Path $root "settings.cfg"), $enc.GetBytes("volume=75;muted=0;res=1280x720;night=0;night_level=55;keyboard=tr;language=tr;zoom=100;region=0;dhcp=1;theme=velvet"))
# Kurulum isareti EN SON yazilir.
[IO.File]::WriteAllBytes((Join-Path $root "MINOS.SYS"), $enc.GetBytes("minOs Persistent Install`nFAT32 data and settings installed`n"))

Write-Host "[3/6] VHD ayriliyor..." -ForegroundColor Yellow
$dp2 = @"
select vdisk file="$vhd"
detach vdisk
"@
Set-Content -Path (Join-Path $tmp "detach.txt") -Value $dp2 -Encoding ascii
& diskpart /s (Join-Path $tmp "detach.txt") | Out-Null
Start-Sleep -Seconds 2

Write-Host "[4/6] RAW'a cevriliyor + Limine BIOS kuruluyor..." -ForegroundColor Yellow
$raw = Join-Path $tmp "minos.raw"
& $qemuImg convert -f vpc -O raw $vhd $raw
if ($LASTEXITCODE -ne 0) { Write-Host "[HATA] qemu-img convert basarisiz." -ForegroundColor Red; exit 1 }
& $limine bios-install $raw
# limine stderr'e bilgi basar ama exit 0 olmalidir; powershell native hata yakalama icin kontrol:
if ($LASTEXITCODE -ne 0) { Write-Host "[HATA] limine bios-install basarisiz." -ForegroundColor Red; exit 1 }

Write-Host "[5/6] Disk imaji kaydediliyor: $DiskImage ..." -ForegroundColor Yellow
Remove-Item -Force $DiskImage -ErrorAction SilentlyContinue
if ($DiskImage.EndsWith(".raw", [System.StringComparison]::OrdinalIgnoreCase)) {
    Copy-Item -Force $raw $DiskImage
} else {
    & $qemuImg convert -f raw -O qcow2 $raw $DiskImage
    if ($LASTEXITCODE -ne 0) { Write-Host "[HATA] qcow2 convert basarisiz." -ForegroundColor Red; exit 1 }
}

Write-Host "[6/6] Dogrulama..." -ForegroundColor Yellow
$installed = Test-DiskInstalled $DiskImage
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
if (-not $installed) { Write-Host "[HATA] MINOS.SYS goruntude bulunamadi." -ForegroundColor Red; exit 1 }
Set-Content -Path $MarkerFile -Value ("prepared=" + (Get-Date -Format o)) -Encoding ascii
Write-Host "[BASARILI] Disk hazir: $DiskImage (kurulu, bootable)" -ForegroundColor Green
