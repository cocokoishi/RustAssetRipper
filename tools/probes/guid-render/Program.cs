// What exact text does the reference's `UnityGuid.ToString()` produce?
//
// `FileIdentifier.GetFilePath` returns `UnityGuid.ToString()` for an `AssetType.Meta` identifier, and
// the same rendering reaches every `.meta` guid line, so the word/nibble order and the letter case
// are load-bearing for every document comparison. The port implements the order as "each 32-bit
// word's nibbles least significant first" in `serialized_file::metadata`; this probe measures the
// reference's answer on words chosen to expose an order or signedness mistake, including the sign
// bit, all-ones, and mixed patterns.
//
// Run (from the RustAssetripper repository root, with a COMPLETE reference build):
//   dotnet run --project tools/probes/guid-render/GuidRenderProbe.csproj -c Release -- <refroot>
using System.Reflection;
using System.Runtime.Loader;

public static class Program
{
    public static int Main(string[] args)
    {
        string root = args.Length > 0
            ? args[0]
            : Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "Temp", "opencode", "ar-ref", "Source", "0Bins", "AssetRipper.GUI.Free", "Release");
        AssemblyLoadContext.Default.Resolving += (_, name) =>
        {
            string candidate = Path.Combine(root, $"{name.Name}.dll");
            return File.Exists(candidate) ? Assembly.LoadFrom(candidate) : null;
        };

        // `UnityGuid` is defined in AssetRipper.Primitives; load it through LoadFrom so the resolver
        // above can satisfy its own dependencies.
        Assembly primitives = Assembly.LoadFrom(Path.Combine(root, "AssetRipper.Primitives.dll"));
        Type? guid = primitives.GetType("AssetRipper.Primitives.UnityGuid")
            ?? SafeTypes(primitives).FirstOrDefault(t => t.Name == "UnityGuid");
        if (guid is null)
        {
            Console.WriteLine("PROBE|no-unityguid");
            foreach (Type t in SafeTypes(primitives).Select(t => t).Take(0))
            {
                _ = t;
            }
            return 0;
        }
        Console.WriteLine($"PROBE|type|{guid.FullName}");

        // The four-word constructor, if it exists, keeps the probe independent of the internal
        // layout: the words are supplied in the order the serialized file stores them.
        ConstructorInfo? ctor = guid.GetConstructors()
            .FirstOrDefault(c => c.GetParameters().Length == 4
                && c.GetParameters().All(p => p.ParameterType == typeof(uint)));
        if (ctor is null)
        {
            Console.WriteLine($"PROBE|no-word-constructor|{string.Join(";", guid.GetConstructors().Select(c => string.Join(",", c.GetParameters().Select(p => p.ParameterType.Name))))}");
            return 0;
        }

        uint[][] cases =
        {
            new uint[] { 0u, 0u, 0u, 0u },
            new uint[] { 1u, 0u, 0u, 0u },
            new uint[] { 0x01020304u, 0xA1B2C3D4u, 0x0F1E2D3Cu, 0x99887766u },
            new uint[] { 0x80000000u, 0x00000000u, 0xFFFFFFFFu, 0x7FFFFFFFu },
            new uint[] { 0x12345678u, 0x9ABCDEF0u, 0xDEADBEEFu, 0x0BADF00Du },
            new uint[] { 0xFFFFFFFFu, 0xFFFFFFFFu, 0xFFFFFFFFu, 0xFFFFFFFFu },
        };

        foreach (uint[] words in cases)
        {
            object value = ctor.Invoke(words.Cast<object>().ToArray());
            string text = value.ToString() ?? "";
            // A value that does not round-trip back through the parser would be a stronger defect
            // than a formatting difference, so report both.
            string words_text = string.Join(",", words.Select(w => w.ToString("X8")));
            Console.WriteLine($"GUID|{words_text}|{text}");
        }
        return 0;
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
