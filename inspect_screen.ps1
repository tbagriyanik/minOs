Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap("screen.png")
Write-Host "Width: $($bmp.Width), Height: $($bmp.Height)"
$colors = @{}
for ($y = 0; $y -lt $bmp.Height; $y += 4) {
    for ($x = 0; $x -lt $bmp.Width; $x += 4) {
        $c = $bmp.GetPixel($x, $y)
        $hex = "#{0:X2}{1:X2}{2:X2}" -f $c.R, $c.G, $c.B
        $colors[$hex] = 1
    }
}
Write-Host "Found $($colors.Keys.Count) unique colors:"
$colors.Keys | ForEach-Object { Write-Host " - $_" }
$bmp.Dispose()
