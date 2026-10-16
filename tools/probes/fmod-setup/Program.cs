// Dump the Vorbis setup-header table the frozen reference actually ships.
//
// The Rust port carries a 161-entry table copied from `fsbex` 0.3.0. This probe prints the same
// table out of `Fmod5Sharp.dll`, which is what the reference loads at runtime, so the two can be
// compared entry for entry instead of assumed equal.
//
// Run (from the RustAssetRipper repository root, with the isolated reference build in place):
//   dotnet run --project tools/probes/fmod-setup/FmodSetupProbe.csproj -c Release -- <refroot>
using System.Reflection;
using System.Security.Cryptography;

public static class Program
{
    public static int Main(string[] args)
    {
        string root = args.Length > 0
            ? args[0]
            : Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "Temp", "opencode", "ar-ref", "Source", "0Bins", "AssetRipper.GUI.Free", "Release");
        string dll = Path.Combine(root, "Fmod5Sharp.dll");
        if (!File.Exists(dll))
        {
            Console.WriteLine($"PROBE|missing|{dll}");
            return 1;
        }

        Assembly assembly = Assembly.LoadFrom(dll);
        Console.WriteLine($"PROBE|assembly|{assembly.GetName().Name}|{assembly.GetName().Version}");

        // Find any type holding a dictionary whose keys are setup-header CRCs and whose values are
        // the header bytes. The table's exact owner has changed between versions, so this searches
        // rather than hard-coding a type name.
        // Find the Vorbis data type first, then any static member that holds a map keyed by uint and
        // valued by it. The containing member name is compiler-mangled, so the value type is the
        // reliable selector.
        Type vorbisData = SafeTypes(assembly).FirstOrDefault(t => t.Name == "FmodVorbisData");
        Console.WriteLine($"PROBE|vorbisType|{vorbisData?.FullName ?? "<none>"}");

        // The table lives behind a generated singleton property, so it is found by name rather
        // than by guessing a field or a value type.
        foreach (Type type in SafeTypes(assembly))
        {
            foreach (PropertyInfo property in type.GetProperties(
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static))
            {
                if (!property.Name.Contains("Vorbis", StringComparison.Ordinal))
                {
                    continue;
                }
                object value;
                try
                {
                    value = property.GetValue(null);
                }
                catch (Exception ex)
                {
                    Console.WriteLine($"PROBE|getter-failed|{property.Name}|{ex.GetType().Name}");
                    continue;
                }
                int count = value is System.Collections.ICollection collection ? collection.Count : -1;
                Console.WriteLine(
                    $"PROBE|property|{type.FullName}.{property.Name}|{value?.GetType().Name}|count={count}");
            }
            foreach (MethodInfo method in type.GetMethods(
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static))
            {
                if (!method.Name.Contains("Vorbis", StringComparison.Ordinal)
                    || method.GetParameters().Length != 0)
                {
                    continue;
                }
                object value;
                try
                {
                    value = method.Invoke(null, null);
                }
                catch (Exception ex)
                {
                    Console.WriteLine($"PROBE|invoke-failed|{method.Name}|{ex.GetType().Name}");
                    continue;
                }
                if (value is not null && value.GetType().Name.Contains("Vorbis"))
                {
                    Console.WriteLine(
                        $"PROBE|method|{type.FullName}.{method.Name}|{value.GetType().FullName}");
                }
            }
        }

        int reported = 0;
        foreach (Type type in SafeTypes(assembly))
        {
            foreach (MemberInfo member in type.GetMembers(
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static))
            {
                Type memberType = member switch
                {
                    FieldInfo field => field.FieldType,
                    PropertyInfo property => property.PropertyType,
                    MethodInfo method => method.ReturnType,
                    _ => null!,
                };
                if (memberType is null || !memberType.IsGenericType)
                {
                    continue;
                }
                Type definition = memberType.GetGenericTypeDefinition();
                Type[] arguments = memberType.GetGenericArguments();
                if (definition != typeof(Dictionary<,>) || arguments[0] != typeof(uint))
                {
                    continue;
                }
                object value = member switch
                {
                    FieldInfo field => field.GetValue(null),
                    PropertyInfo property => property.GetValue(null),
                    MethodInfo method => method.Invoke(null, null),
                    _ => null,
                };
                if (value is not System.Collections.IDictionary map || map.Count == 0)
                {
                    continue;
                }
                Console.WriteLine(
                    $"PROBE|candidate|{type.FullName}.{member.Name}|count={map.Count}|value={arguments[1].Name}");
            }
        }

        foreach (Type type in SafeTypes(assembly))
        {
            foreach (FieldInfo field in type.GetFields(
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static))
            {
                if (!field.FieldType.IsGenericType)
                {
                    continue;
                }

                Type definition = field.FieldType.GetGenericTypeDefinition();
                Type[] arguments = field.FieldType.GetGenericArguments();
                if (definition != typeof(Dictionary<,>) || arguments[0] != typeof(uint))
                {
                    continue;
                }
                // The frequency table is keyed by small codes; the Vorbis setup table is keyed by
                // CRC32 values and its value type is named `FmodVorbisData`. Selecting on the value
                // type keeps a future version's frequency table out of the comparison.
                if (!arguments[1].Name.Contains("Vorbis"))
                {
                    continue;
                }

                object value = field.GetValue(null);
                if (value is not System.Collections.IDictionary map || map.Count == 0)
                {
                    continue;
                }

                Console.WriteLine($"PROBE|table|{type.FullName}.{field.Name}|{map.Count}");
                List<string> keys = new();
                foreach (System.Collections.DictionaryEntry entry in map)
                {
                    string key = ((uint)entry.Key).ToString("X8");
                    keys.Add(key);
                    // The header bytes matter as much as the keys: a table with the right keys and
                    // wrong payloads would decode silence. Print a digest of each value.
                    byte[] payload = ExtractBytes(entry.Value);
                    Console.WriteLine($"VALUE|{key}|{payload.Length}|{Convert.ToHexString(SHA256.HashData(payload))[..16]}");
                }
                keys.Sort(StringComparer.Ordinal);
                foreach (string key in keys)
                {
                    Console.WriteLine($"KEY|{key}");
                }
                reported++;
            }
        }

        if (reported == 0)
        {
            Console.WriteLine("PROBE|no-table-found");
        }
        return 0;
    }

    /// The header bytes behind one table entry.
    ///
    /// The value type is `FmodVorbisData`; its payload is exposed either as a byte array property
    /// or a byte-array field, so both are tried and anything else reports an empty payload rather
    /// than guessing.
    private static byte[] ExtractBytes(object value)
    {
        Type type = value.GetType();
        foreach (PropertyInfo property in type.GetProperties(
            BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance))
        {
            if (property.PropertyType == typeof(byte[]) && property.GetIndexParameters().Length == 0)
            {
                if (property.GetValue(value) is byte[] bytes)
                {
                    return bytes;
                }
            }
        }
        foreach (FieldInfo field in type.GetFields(
            BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance))
        {
            if (field.FieldType == typeof(byte[]) && field.GetValue(value) is byte[] bytes)
            {
                return bytes;
            }
        }
        return Array.Empty<byte>();
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
