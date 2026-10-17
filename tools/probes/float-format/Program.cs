// How exactly do `float.ToString()` and `double.ToString()` print under the invariant culture?
//
// `DefaultJsonWalker.VisitPrimitive` writes floats and doubles by handing them to the writer,
// which calls `ToString()` - there is no explicit format string, so the result is .NET's
// shortest-round-trip default. The Rust port matched it with `Display` and claimed in a comment
// that the two agree "for the magnitudes Unity stores". This probe tests that claim.
//
// Run (from the RustAssetripper repository root):
//   dotnet run --project tools/probes/float-format/FloatFormatProbe.csproj -c Release
using System.Globalization;

public static class Program
{
    public static int Main()
    {
        // The boundary hunt: powers of ten either side of the fixed/scientific switch.
        Console.WriteLine("# f32 exponent boundaries");
        foreach (int exponent in new[] { -6, -5, -4, 6, 7, 8, 9, 10, 11, 12 })
        {
            float value = (float)Math.Pow(10, exponent);
            Console.WriteLine($"BOUNDARY|f32|1e{exponent}|{Describe(value)}");
        }
        Console.WriteLine("# f64 exponent boundaries");
        foreach (int exponent in new[] { -8, -7, -6, -5, -4, 14, 15, 16, 17, 18 })
        {
            double value = Math.Pow(10, exponent);
            Console.WriteLine($"BOUNDARY|f64|1e{exponent}|{Describe(value)}");
        }

        Console.WriteLine("# values that separate the plausible rules");
        Report<float>("f32", new[]
        {
            0.0f, -0.0f, 1.0f, -1.0f, 0.5f, 100.0f, 0.0001f, 1e-5f, 1e-7f,
            1e7f, 1e8f, 1e15f, 1e16f, 1e20f, 3.4028235e38f, 1.1754944e-38f,
            1.1920929e-7f, 0.1f, 1.0f / 3.0f, 123456.789f, 16777216.0f,
            0.30000001f, 2.5f, 1e-45f, 1234.5f, 0.000123f, 9999999f,
            float.NaN, float.PositiveInfinity, float.NegativeInfinity,
        });
        Report<double>("f64", new[]
        {
            0.0, -0.0, 1.0, -1.0, 0.5, 100.0, 0.0001, 1e-5, 1e-7,
            1e7, 1e15, 1e16, 1e17, 1e20, 1.7976931348623157e308, 5e-324,
            2.2250738585072014e-308, 0.1, 1.0 / 3.0, 123456.789,
            0.30000000000000004, 2.5, 1e-323,
            double.NaN, double.PositiveInfinity, double.NegativeInfinity,
        });
        return 0;
    }

    /// The invariant-culture default `ToString`, which is what the walker writes.
    private static string Describe(float value) => value.ToString(CultureInfo.InvariantCulture);

    private static string Describe(double value) => value.ToString(CultureInfo.InvariantCulture);

    private static void Report<T>(string kind, T[] values)
    {
        foreach (T value in values)
        {
            string bits = value switch
            {
                float f => BitConverter.SingleToUInt32Bits(f).ToString("X8"),
                double d => BitConverter.DoubleToUInt64Bits(d).ToString("X16"),
                _ => "",
            };
            string text = value switch
            {
                float f => Describe(f),
                double d => Describe(d),
                _ => "",
            };
            Console.WriteLine($"VALUE|{kind}|{bits}|{text}");
        }
    }
}
