using System;
using System.Collections.Generic;
using System.Security.Cryptography;

// Minimal Ogg page/packet splitter for differential analysis (not product code).
public static class OggPacketHash
{
    public static List<byte[]> Packets(byte[] data)
    {
        List<byte[]> packets = new List<byte[]>();
        List<byte> current = new List<byte>();
        int pos = 0;
        while (pos + 27 <= data.Length)
        {
            if (data[pos] != (byte)'O' || data[pos + 1] != (byte)'g' || data[pos + 2] != (byte)'g' || data[pos + 3] != (byte)'S')
            {
                throw new Exception("bad OggS at " + pos);
            }
            int segCount = data[pos + 26];
            if (pos + 27 + segCount > data.Length) throw new Exception("truncated segment table at " + pos);
            int cursor = pos + 27 + segCount;
            for (int s = 0; s < segCount; s++)
            {
                int segLen = data[pos + 27 + s];
                if (cursor + segLen > data.Length) throw new Exception("truncated segment data at " + pos);
                for (int i = 0; i < segLen; i++) current.Add(data[cursor + i]);
                cursor += segLen;
                if (segLen < 255)
                {
                    packets.Add(current.ToArray());
                    current = new List<byte>();
                }
            }
            pos = cursor;
        }
        return packets;
    }

    public static string Sha256Many(List<byte[]> packets, int from, int count)
    {
        using (SHA256 s = SHA256.Create())
        {
            int end = Math.Min(packets.Count, from + count);
            for (int i = from; i < end; i++)
            {
                byte[] len = BitConverter.GetBytes(packets[i].Length);
                s.TransformBlock(len, 0, len.Length, null, 0);
                s.TransformBlock(packets[i], 0, packets[i].Length, null, 0);
            }
            s.TransformFinalBlock(new byte[0], 0, 0);
            return BitConverter.ToString(s.Hash).Replace("-", "");
        }
    }
}
