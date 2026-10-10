Add-Type -AssemblyName System.Drawing
$bytes = [IO.File]::ReadAllBytes('F:\minos\final_cmd.ppm')
$data = New-Object byte[] ($bytes.Length - 15)
[Array]::Copy($bytes, 15, $data, 0, $data.Length)
$w = 1024; $h = 768
$bmp = New-Object Drawing.Bitmap($w, $h, [Drawing.Imaging.PixelFormat]::Format24bppRgb)
$rect = New-Object Drawing.Rectangle(0, 0, $w, $h)
$bd = $bmp.LockBits($rect, [Drawing.Imaging.ImageLockMode]::WriteOnly, [Drawing.Imaging.PixelFormat]::Format24bppRgb)
[System.Runtime.InteropServices.Marshal]::Copy($data, 0, $bd.Scan0, [Math]::Min($data.Length, $bd.Stride * $h))
# PPM ustten-asagi RGB; GDI 24bpp asagi-ustten BGR. Kanal takasi:
for ($yy = 0; $yy -lt $h; $yy++) {
  $rowOff = $yy * $bd.Stride
  for ($xx = 0; $xx -lt $w; $xx++) {
    $o = $rowOff + $xx * 3
    $t = [System.Runtime.InteropServices.Marshal]::ReadByte($bd.Scan0, $o)
    [System.Runtime.InteropServices.Marshal]::WriteByte($bd.Scan0, $o, [System.Runtime.InteropServices.Marshal]::ReadByte($bd.Scan0, $o + 2))
    [System.Runtime.InteropServices.Marshal]::WriteByte($bd.Scan0, $o + 2, $t)
  }
}
$bmp.UnlockBits($bd)
$bmp.RotateFlip([Drawing.RotateFlipType]::RotateNoneFlipY)
$bmp.Save('F:\minos\final_cmd.png',[Drawing.Imaging.ImageFormat]::Png)
Write-Host "ok"
