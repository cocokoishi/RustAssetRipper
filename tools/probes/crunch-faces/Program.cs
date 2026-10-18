// What does the frozen reference's Crunch entry point actually decode?
//
// `RES-TEXTURE-CRUNCH-001` records that "multi-face/multi-mip payloads ... remain unverified".
// The reference calls `Crunch.TryDecompress(data, out bytes)` / `UnityCrunch.TryDecompress(...)`
// with **no dimension or face arguments**, and `TextureConverter` afterwards divides the result by
// `options.Depth`. This probe answers two questions the ledger cannot answer by reading the Rust
// side alone:
//
//   1. Does a single-face CRN decode to exactly one face of pixels, so that the reference's
//      `Length / Depth` step is the only place multiple layers are accounted for?
//   2. What does the reference do with a payload whose declared face count is greater than one?
//
// Run (from the RustAssetripper repository root, with the isolated reference build present):
//   dotnet run --project tools/probes/crunch-faces/CrunchFacesProbe.csproj -c Release -- <refroot>
using System.Reflection;
using System.Security.Cryptography;

public static class Program
{
    private static readonly string[] Fixtures =
    {
        "classic_dxt1.crn",
        "classic_dxt5.crn",
        "unity_dxt1.crn",
        "unity_dxt5.crn",
        "unity_etc1.crn",
        "unity_etc2a.crn",
        "multi_face_multimip.crn",
    };

    public static int Main(string[] args)
    {
        string referenceRoot = args.Length > 0
            ? args[0]
            : Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "Temp", "opencode", "ar-ref", "Source", "0Bins", "AssetRipper.GUI.Free", "Release");
        if (args.Length < 2)
        {
            Console.WriteLine("PROBE|usage|<referenceRoot> <fixtureDir>");
            return 1;
        }

        string fixtureDir = args[1];

        Assembly? classic = Load(referenceRoot, "AssetRipper.Conversions.Crunch.dll");
        Assembly? unity = Load(referenceRoot, "AssetRipper.Conversions.UnityCrunch.dll");
        Console.WriteLine($"PROBE|classic|{classic?.GetName().Name ?? "<missing>"}");
        Console.WriteLine($"PROBE|unity|{unity?.GetName().Name ?? "<missing>"}");

        foreach (string name in Fixtures)
        {
            string path = Path.Combine(fixtureDir, name);
            if (!File.Exists(path))
            {
                Console.WriteLine($"MISSING|{name}");
                continue;
            }
            byte[] data = File.ReadAllBytes(path);
            // The CRN header carries the dimensions and level count the reference does not need.
            (int width, int height, int levels, int faces) = ReadCrnHeader(data);
            Console.WriteLine(
                $"INPUT|{name}|{data.Length}|w={width}|h={height}|levels={levels}|faces={faces}");

            Report("classic", classic, name, data);
            Report("unity", unity, name, data);
        }
        return 0;
    }

    private static void Report(string tag, Assembly? assembly, string name, byte[] data)
    {
        if (assembly is null)
        {
            Console.WriteLine($"OUT|{tag}|{name}|<no-assembly>");
            return;
        }

        // The public entry point is `TryDecompress(ReadOnlySpan<byte>, out byte[])`.
        Type? handler = SafeTypes(assembly)
            .FirstOrDefault(t => t.GetMethods(BindingFlags.Public | BindingFlags.Static)
                .Any(m => m.Name == "TryDecompress"));
        if (handler is null)
        {
            Console.WriteLine($"OUT|{tag}|{name}|<no-TryDecompress>");
            return;
        }

        MethodInfo method = handler
            .GetMethods(BindingFlags.Public | BindingFlags.Static)
            .First(m => m.Name == "TryDecompress");
        ParameterInfo[] signature = method.GetParameters();
        object[] parameters = new object[signature.Length];
        // `TryDecompress(ReadOnlySpan<byte> data, out byte[] bytes)` cannot take a byte[] boxed as
        // an object, so the span is built and passed through a strongly typed delegate.
        byte[]? decoded = null;
        bool ok;
        try
        {
            if (signature[0].ParameterType == typeof(ReadOnlySpan<byte>))
            {
                // Boxing a span is impossible, so invoke through a delegate instead.
                ok = InvokeWithSpan(method, data, out decoded);
            }
            else
            {
                parameters[0] = data;
                ok = (bool)method.Invoke(null, parameters)!;
                decoded = parameters[1] as byte[];
            }
        }
        catch (Exception ex)
        {
            Console.WriteLine($"OUT|{tag}|{name}|<threw {ex.GetType().Name}: {ex.Message.Split('\n')[0]}>");
            return;
        }
        if (!ok || decoded is not byte[] result)
        {
            Console.WriteLine($"OUT|{tag}|{name}|<failed>");
            return;
        }
        string digest = Convert.ToHexString(SHA256.HashData(result))[..16];
        // `TextureConverter` divides by Depth; a single-face CRN must therefore yield exactly one
        // face's worth of pixels for that division to be lossless.
        Console.WriteLine($"OUT|{tag}|{name}|ok|{result.Length}|{digest}");
    }

    /// Invoke a `TryDecompress(ReadOnlySpan<byte>, out byte[])` overload.
    ///
    /// A `ReadOnlySpan<byte>` cannot be boxed into the `object[]` reflection expects, so the call
    /// is made through a delegate whose signature matches the method exactly.
    private static bool InvokeWithSpan(MethodInfo method, ReadOnlySpan<byte> data, out byte[]? decoded)
    {
        var invoker = (TryDecompressSpan)method.CreateDelegate(typeof(TryDecompressSpan));
        return invoker(data, out decoded);
    }

    private delegate bool TryDecompressSpan(ReadOnlySpan<byte> data, out byte[]? decoded);

    /// The CRN header's big-endian dimensions and single-byte level/face counts.
    private static (int Width, int Height, int Levels, int Faces) ReadCrnHeader(byte[] data)
    {
        if (data.Length < 18)
        {
            return (-1, -1, -1, -1);
        }
        // CRN stores width and height as big-endian u16 values at offsets 12 and 14, followed by
        // one-byte level and face counts at offsets 16 and 17.
        int width = (data[12] << 8) | data[13];
        int height = (data[14] << 8) | data[15];
        return (width, height, data[16], data[17]);
    }

    private static Assembly? Load(string root, string file)
    {
        string path = Path.Combine(root, file);
        return File.Exists(path) ? Assembly.LoadFrom(path) : null;
    }

    private static IEnumerable<Type> SafeTypes(Assembly assembly)
    {
        try
        {
            return assembly.GetTypes();
        }
        catch (ReflectionTypeLoadException ex)
        {
            return ex.Types.Where(t => t is not null)!;
        }
    }
}
