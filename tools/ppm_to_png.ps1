param(
    [Parameter(Mandatory = $true)][string]$Ppm,
    [Parameter(Mandatory = $true)][string]$Png
)

# PPM (P6) -> PNG dönüşümü (Windows System.Drawing).
Add-Type -AssemblyName System.Drawing

$bytes = [System.IO.File]::ReadAllBytes($Ppm)
# Başlık: "P6\n<w> <h>\n255\n"
$pos = 0
function Read-Token([byte[]]$b, [ref]$p) {
    $sb = New-Object System.Text.StringBuilder
    while ($true) {
        $c = [char]$b[$p.Value]
        $p.Value++
        if ($c -eq "`n") { break }
        if ($c -eq ' ' -or $c -eq "`t") {
            if ($sb.Length -gt 0) { break }
            continue
        }
        if ($c -eq '#') {
            while ($b[$p.Value] -ne 10) { $p.Value++ }
            $p.Value++
            continue
        }
        [void]$sb.Append($c)
    }
    $sb.ToString()
}
$magic = Read-Token $bytes ([ref]$pos)
if ($magic -ne 'P6') { throw "PPM P6 bekleniyordu: $magic" }
$w = [int](Read-Token $bytes ([ref]$pos))
$h = [int](Read-Token $bytes ([ref]$pos))
$maxv = [int](Read-Token $bytes ([ref]$pos))
if ($maxv -ne 255) { throw "PPM max degeri 255 olmali" }

$bmp = New-Object System.Drawing.Bitmap $w, $h
$rect = New-Object System.Drawing.Rectangle 0, 0, $w, $h
$data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::WriteOnly, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
$stride = $data.Stride
$row = New-Object byte[] ($stride)
for ($y = 0; $y -lt $h; $y++) {
    for ($x = 0; $x -lt $w; $x++) {
        $i = $pos + ($y * $w + $x) * 3
        $row[$x * 3 + 0] = $bytes[$i + 2]   # BGR
        $row[$x * 3 + 1] = $bytes[$i + 1]
        $row[$x * 3 + 2] = $bytes[$i + 0]
    }
    [System.Runtime.InteropServices.Marshal]::Copy($row, 0, [IntPtr]($data.Scan0.ToInt64() + $y * $stride), $stride)
}
$bmp.UnlockBits($data)
$bmp.Save($Png, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host "$Png yazildi ($w x $h)"
