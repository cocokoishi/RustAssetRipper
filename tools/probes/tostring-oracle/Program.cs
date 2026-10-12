// Differential oracle for .NET Double/Single.ToString (invariant culture).
//
// Reads lines on stdin and prints one result line per line:
//   `f32 <hexbits>` / `f64 <hexbits>` -> `<hexbits> <tostring>`
//   `parse <decimal>` -> `<decimal> <f64hexbits>`
// Bare runtime only: no AssetRipper references.
using System.Globalization;

foreach (string? raw in Console.In.ReadToEnd().Split('\n'))
{
    string line = raw.Trim();
    if (line.Length == 0)
    {
        continue;
    }
    if (line.StartsWith("parse ", StringComparison.Ordinal))
    {
        string numeral = line.Substring(6).Trim();
        double value = double.Parse(numeral, CultureInfo.InvariantCulture);
        Console.WriteLine($"{numeral} {BitConverter.DoubleToUInt64Bits(value):X16}");
        continue;
    }
    string[] parts = line.Split(new char[] { ' ', '\t' }, StringSplitOptions.RemoveEmptyEntries);
    if (parts.Length != 2)
    {
        Console.Error.WriteLine($"skip malformed line: {line}");
        continue;
    }
    ulong bits = Convert.ToUInt64(parts[1], 16);
    string text = parts[0] switch
    {
        "f32" => BitConverter.UInt32BitsToSingle((uint)bits).ToString(CultureInfo.InvariantCulture),
        "f64" => BitConverter.UInt64BitsToDouble(bits).ToString(CultureInfo.InvariantCulture),
        _ => throw new Exception($"unknown kind: {parts[0]}"),
    };
    Console.WriteLine($"{parts[1].ToLowerInvariant()} {text}");
}
