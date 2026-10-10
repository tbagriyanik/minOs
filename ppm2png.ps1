Add-Type -AssemblyName System.Drawing
$bytes = [IO.File]::ReadAllBytes('F:\minos\final_cmd.ppm')
$ms = New-Object IO.MemoryStream(,$bytes[15..($bytes.Length-1)])
$ms.Seek(0,'Begin') | Out-Null
$ms.Position = 0
$ms.Capacity = $bytes.Length - 15
$stride = 1024 * 3
if ($stride % 4 -ne 0) { $stride += (4 - ($stride % 4)) }
$bmp = New-Object Drawing.Bitmap(1024,768,$stride,[Drawing.Imaging.PixelFormat]::Format24bppRgb,$ms)
$bmp.RotateFlip([Drawing.RotateFlipType]::Rotate180FlipNone)
$bmp.Save('F:\minos\final_cmd.png',[Drawing.Imaging.ImageFormat]::Png)
Write-Host "ok"
