# minOs HDA + e1000 testi (QEMU, monitör üzerinden)
$q = 'C:\Program Files\qemu\qemu-system-x86_64.exe'
$p = Start-Process -FilePath $q -ArgumentList @(
    '-cdrom', 'F:\minos\minos.iso', '-m', '512M', '-vga', 'std', '-display', 'none',
    '-audiodev', 'driver=sdl,id=snd0',
    '-device', 'intel-hda', '-device', 'hda-output,audiodev=snd0',
    '-netdev', 'user,id=n0', '-device', 'e1000,netdev=n0',
    '-serial', 'file:F:/minos/serial.log',
    '-monitor', 'tcp:127.0.0.1:45496,server,nowait', '-no-reboot'
) -PassThru -WindowStyle Hidden
$c = $null
for ($i = 0; $i -lt 30; $i++) { Start-Sleep -Milliseconds 500; try { $c = New-Object System.Net.Sockets.TcpClient('127.0.0.1', 45496); break } catch { } }
if (-not $c) { Write-Host "port yok"; exit 1 }
$s = $c.GetStream()
function Mon($cmd) { $b=[Text.Encoding]::ASCII.GetBytes("$cmd`n"); $s.Write($b,0,$b.Length); $s.Flush(); Start-Sleep -Milliseconds 130 }
function Send-Cmd($str) { foreach ($ch in $str.ToCharArray()) { if ($ch -eq ' ') { Mon "sendkey spc" } else { Mon "sendkey $ch" } }; Mon "sendkey ret"; Start-Sleep -Milliseconds 350 }

Start-Sleep -Seconds 8
# Tarayıcıyı başlat (menüden 'b'? hayır — tarayıcı menü item 4. indeks = tuş 5?)
# Kullanım planı: Terminal (tuş 4) → minos://net için tarayıcı açma yerine serial test yeterli.
Mon "sendkey 4"
Start-Sleep -Milliseconds 1200
Send-Cmd "help"
Send-Cmd "ls"
Send-Cmd "ps"
Send-Cmd "date"
Send-Cmd "clear"
Send-Cmd "ls"
Start-Sleep -Milliseconds 500
Mon "screendump F:/minos/final_cmd.ppm"
$c.Close(); Start-Sleep -Seconds 1
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
Write-Host "done"
