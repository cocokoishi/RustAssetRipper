using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Security.Cryptography;

public static class PngPixelHash
{
    public static string Hash(Bitmap bmp)
    {
        Rectangle r = new Rectangle(0, 0, bmp.Width, bmp.Height);
        BitmapData d = bmp.LockBits(r, ImageLockMode.ReadOnly, bmp.PixelFormat);
        try
        {
            int n = Math.Abs(d.Stride) * d.Height;
            byte[] buf = new byte[n];
            Marshal.Copy(d.Scan0, buf, 0, n);
            using (SHA256 s = SHA256.Create())
            {
                return BitConverter.ToString(s.ComputeHash(buf)).Replace("-", "");
            }
        }
        finally
        {
            bmp.UnlockBits(d);
        }
    }
}
