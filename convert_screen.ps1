$csharp = @"
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;

public class PpmConverter {
    public static void Convert(string ppmPath, string pngPath) {
        byte[] bytes = File.ReadAllBytes(ppmPath);
        int idx = 0;
        // Skip header
        string header = "";
        while (idx < bytes.Length) {
            char c = (char)bytes[idx++];
            header += c;
            if (header.EndsWith("\n") && header.Split(new char[]{' ', '\r', '\n'}, StringSplitOptions.RemoveEmptyEntries).Length >= 4) {
                break;
            }
        }
        string[] parts = header.Split(new char[]{' ', '\r', '\n', '\t'}, StringSplitOptions.RemoveEmptyEntries);
        int width = int.Parse(parts[1]);
        int height = int.Parse(parts[2]);

        Bitmap bmp = new Bitmap(width, height, PixelFormat.Format24bppRgb);
        BitmapData bd = bmp.LockBits(new Rectangle(0, 0, width, height), ImageLockMode.WriteOnly, PixelFormat.Format24bppRgb);
        
        byte[] rgbValues = new byte[bd.Stride * height];
        for (int y = 0; y < height; y++) {
            for (int x = 0; x < width; x++) {
                int srcIdx = idx + (y * width + x) * 3;
                int dstIdx = y * bd.Stride + x * 3;
                if (srcIdx + 2 < bytes.Length) {
                    rgbValues[dstIdx] = bytes[srcIdx + 2];     // B
                    rgbValues[dstIdx + 1] = bytes[srcIdx + 1]; // G
                    rgbValues[dstIdx + 2] = bytes[srcIdx];     // R
                }
            }
        }
        Marshal.Copy(rgbValues, 0, bd.Scan0, rgbValues.Length);
        bmp.UnlockBits(bd);
        bmp.Save(pngPath, ImageFormat.Png);
        bmp.Dispose();
    }
}
"@

Add-Type -TypeDefinition $csharp -ReferencedAssemblies "System.Drawing.dll"
[PpmConverter]::Convert("screen.ppm", "screen.png")
Write-Host "Done converting screen.png"
