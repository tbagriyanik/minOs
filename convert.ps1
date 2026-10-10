param(
    [string]$InputPpm = "fresh_screen.ppm",
    [string]$OutputPng = "fresh_screen.png"
)

Add-Type -AssemblyName System.Drawing

$bytes = [System.IO.File]::ReadAllBytes($InputPpm)
$headerEnd = 0
$newlines = 0
for ($i = 0; $i -lt 100; $i++) {
    if ($bytes[$i] -eq 10) {
        $newlines++
        if ($newlines -eq 3) {
            $headerEnd = $i + 1
            break
        }
    }
}

$bmp = New-Object System.Drawing.Bitmap 1280, 720
$rect = New-Object System.Drawing.Rectangle 0, 0, 1280, 720
$bmpData = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::WriteOnly, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
$ptr = $bmpData.Scan0
$stride = $bmpData.Stride
$rgb = New-Object byte[] ($stride * 720)

for ($y = 0; $y -lt 720; $y++) {
    for ($x = 0; $x -lt 1280; $x++) {
        $srcIdx = $headerEnd + ($y * 1280 + $x) * 3
        $dstIdx = $y * $stride + $x * 3
        $rgb[$dstIdx] = $bytes[$srcIdx + 2]     # B
        $rgb[$dstIdx + 1] = $bytes[$srcIdx + 1] # G
        $rgb[$dstIdx + 2] = $bytes[$srcIdx]     # R
    }
}

[System.Runtime.InteropServices.Marshal]::Copy($rgb, 0, $ptr, $rgb.Length)
$bmp.UnlockBits($bmpData)
$bmp.Save($OutputPng, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host "$OutputPng created successfully"
