// What exact text does the reference's `DefaultJsonWalker` emit?
//
// `DefaultJsonWalker` is a public class and every callback is `public virtual`, so it can be driven
// directly with a `StringWriter` and no asset model at all. That makes the walker's *shape rules* -
// indentation, where newlines go, how empty containers collapse, how dictionary pairs render, and
// how a PPtr prints - capturable from the runtime instead of asserted from the port.
//
// This matters because the port's expectations for this module were twice written from the port
// itself and were twice wrong (see DIFF-JSON-ESCAPE-ENCODER-001 and DIFF-JSON-FLOAT-FORMAT-001).
//
// Run (from the RustAssetRipper repository root, with a COMPLETE reference build present):
//   dotnet run --project tools/probes/json-walker/JsonWalkerProbe.csproj -c Release -- <refroot>
//
// A complete build is `AssetRipper/Source/0Bins/AssetRipper.GUI.Free/Release`. The older mirror
// under `%LOCALAPPDATA%\Temp\opencode\ar-ref` is **incomplete**: it lacks AssetRipper.IO.Endian and
// AssetRipper.SourceGenerated, so `GetTypes()` silently returns a subset that omits the walker
// entirely and the probe reports "no-walker-type" while the file is byte-identical to the real one.
using System.Reflection;
using System.Runtime.Loader;
using System.Text;

public static class Program
{
    public static int Main(string[] args)
    {
        string root = args.Length > 0
            ? args[0]
            : Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "Temp", "opencode", "ar-ref", "Source", "0Bins", "AssetRipper.GUI.Free", "Release");
        string dll = Path.Combine(root, "AssetRipper.Export.PrimaryContent.dll");
        if (!File.Exists(dll))
        {
            Console.WriteLine($"PROBE|missing|{dll}");
            return 1;
        }

        // `DefaultJsonWalker` derives from `AssetWalker` in `AssetRipper.Assets.dll` and its
        // signatures mention `Utf8String`, so those assemblies must resolve or the type silently
        // fails to load. A resolver over the reference root keeps the probe working from any copy.
        AssemblyLoadContext.Default.Resolving += (_, name) =>
        {
            string candidate = Path.Combine(root, $"{name.Name}.dll");
            return File.Exists(candidate) ? Assembly.LoadFrom(candidate) : null;
        };

        Assembly assembly = Assembly.LoadFrom(dll);
        Type walkerType = assembly.GetType("AssetRipper.Export.PrimaryContent.DefaultJsonWalker");
        if (walkerType is null)
        {
            Console.WriteLine("PROBE|no-walker-type");
            foreach (Type t in SafeTypes(assembly).Where(t => t.Name.Contains("Walker")))
            {
                Console.WriteLine($"CANDIDATE|{t.FullName}|public={t.IsPublic}");
            }
            Console.WriteLine($"PROBE|type-count|{SafeTypes(assembly).Count()}");
            return 0;
        }
        Console.WriteLine($"PROBE|type|{walkerType.FullName}");
        foreach (MethodInfo method in walkerType
            .GetMethods(BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly)
            .OrderBy(m => m.Name, StringComparer.Ordinal))
        {
            string parameters = string.Join(
                ", ",
                method.GetParameters().Select(p => $"{Short(p.ParameterType)} {p.Name}"));
            string generics = method.IsGenericMethodDefinition
                ? $"<{string.Join(",", method.GetGenericArguments().Select(a => a.Name))}>"
                : "";
            Console.WriteLine($"SIG|{Short(method.ReturnType)} {method.Name}{generics}({parameters})");
        }

        // Each scenario is one callback sequence; the recorded text is what the walker emits.
        Dump(walkerType, "empty-asset", w => { });
        Dump(walkerType, "one-field-string", w =>
        {
            AssetVoid(w, "EnterAsset");
            Void(w, "EnterField", null, "m_Name");
            Primitive(w, typeof(string), "Cube");
            AssetVoid(w, "ExitAsset");
        });
        Dump(walkerType, "two-fields-divided", w =>
        {
            AssetVoid(w, "EnterAsset");
            Void(w, "EnterField", null, "m_A");
            Primitive(w, typeof(int), 1);
            AssetVoid(w, "DivideAsset");
            Void(w, "EnterField", null, "m_B");
            Primitive(w, typeof(bool), true);
            AssetVoid(w, "ExitAsset");
        });
        Dump(walkerType, "empty-list", w => { Generic(w, "EnterList", new[] { typeof(int) }, new int[0]); });
        Dump(walkerType, "list-of-three", w =>
        {
            var list = new[] { 1, 2, 3 };
            if ((bool)Generic(w, "EnterList", new[] { typeof(int) }, list)!)
            {
                Primitive(w, typeof(int), 1);
                Generic(w, "DivideList", new[] { typeof(int) }, list);
                Primitive(w, typeof(int), 2);
                Generic(w, "DivideList", new[] { typeof(int) }, list);
                Primitive(w, typeof(int), 3);
                Generic(w, "ExitList", new[] { typeof(int) }, list);
            }
        });
        Dump(walkerType, "empty-string-map", w =>
            Generic(w, "EnterDictionary", new[] { typeof(string), typeof(int) }, new Dictionary<string, int>()));
        Dump(walkerType, "empty-int-map", w =>
            Generic(w, "EnterDictionary", new[] { typeof(int), typeof(int) }, new Dictionary<int, int>()));
        Dump(walkerType, "string-map-one", w =>
        {
            var map = new Dictionary<string, int> { ["key"] = 5 };
            var pair = map.First();
            if ((bool)Generic(w, "EnterDictionary", new[] { typeof(string), typeof(int) }, map)!)
            {
                Generic(w, "EnterDictionaryPair", new[] { typeof(string), typeof(int) }, pair);
                Primitive(w, typeof(string), "key");
                Generic(w, "DivideDictionaryPair", new[] { typeof(string), typeof(int) }, pair);
                Primitive(w, typeof(int), 5);
                Generic(w, "ExitDictionaryPair", new[] { typeof(string), typeof(int) }, pair);
                Generic(w, "ExitDictionary", new[] { typeof(string), typeof(int) }, map);
            }
        });
        Dump(walkerType, "int-map-one", w =>
        {
            var map = new Dictionary<int, int> { [7] = 5 };
            var pair = map.First();
            if ((bool)Generic(w, "EnterDictionary", new[] { typeof(int), typeof(int) }, map)!)
            {
                if ((bool)Generic(w, "EnterDictionaryPair", new[] { typeof(int), typeof(int) }, pair)!)
                {
                    Primitive(w, typeof(int), 7);
                    Generic(w, "DivideDictionaryPair", new[] { typeof(int), typeof(int) }, pair);
                    Primitive(w, typeof(int), 5);
                    Generic(w, "ExitDictionaryPair", new[] { typeof(int), typeof(int) }, pair);
                }
                Generic(w, "ExitDictionary", new[] { typeof(int), typeof(int) }, map);
            }
        });
        Dump(walkerType, "nested-list-in-field", w =>
        {
            var list = new[] { 1, 2 };
            AssetVoid(w, "EnterAsset");
            Void(w, "EnterField", null, "m_Values");
            if ((bool)Generic(w, "EnterList", new[] { typeof(int) }, list)!)
            {
                Primitive(w, typeof(int), 1);
                Generic(w, "DivideList", new[] { typeof(int) }, list);
                Primitive(w, typeof(int), 2);
                Generic(w, "ExitList", new[] { typeof(int) }, list);
            }
            AssetVoid(w, "ExitAsset");
        });
        Dump(walkerType, "primitive-byte-array", w =>
            Primitive(w, typeof(byte[]), new byte[] { 1, 2, 3, 4 }));
        Dump(walkerType, "primitive-float", w => Primitive(w, typeof(float), 1.5f));
        Dump(walkerType, "primitive-char", w => Primitive(w, typeof(char), 'A'));

        // `VisitPPtr` takes a `PPtr<TAsset>`, so the shape of that type has to be known before the
        // callback can be driven at all. Dump it once so the construction is recorded rather than
        // guessed.
        DumpPptrType(root);
        DumpPptr(walkerType, root, "pptr-zero", 0, 0L);
        DumpPptr(walkerType, root, "pptr-ordinary", 1, 33L);
        DumpPptr(walkerType, root, "pptr-negative-file", -1, 5L);
        DumpPptr(walkerType, root, "pptr-negative-path", 2, -7L);
        DumpPptr(walkerType, root, "pptr-max-path", 0, long.MaxValue);
        return 0;
    }

    /// Drive `VisitPPtr<TAsset>` with a constructed `PPtr<TAsset>`.
    ///
    /// `PPtr<T>` constrains `T`, so the type argument cannot be `object`; the interface
    /// `IUnityObjectBase` satisfies the constraint and the callback only reads `FileID` and
    /// `PathID`, so the asset type does not affect the text.
    private static void DumpPptr(Type walkerType, string root, string name, int fileId, long pathId)
    {
        Assembly assets = Assembly.LoadFrom(Path.Combine(root, "AssetRipper.Assets.dll"));
        Type[] types = SafeTypes(assets).ToArray();
        Type pptrOpen = types.First(t => t.Name == "PPtr`1");
        Type argument = types.First(t => t.Name == "IUnityObjectBase");
        Type pptrType = pptrOpen.MakeGenericType(argument);
        object pptr = Activator.CreateInstance(pptrType, fileId, pathId)!;
        var writer = new StringWriter();
        object walker = Activator.CreateInstance(walkerType, writer)!;
        MethodInfo visit = walkerType.GetMethods(BindingFlags.Public | BindingFlags.Instance)
            .First(m => m.Name == "VisitPPtr")
            .MakeGenericMethod(argument);
        try
        {
            visit.Invoke(walker, new[] { pptr });
        }
        catch (Exception ex)
        {
            Console.WriteLine($"PPTR|{name}|{fileId}|{pathId}|<threw {ex.GetType().Name}>");
            return;
        }
        Console.WriteLine($"PPTR|{name}|{fileId}|{pathId}|{Visible(writer.ToString())}");
    }

    /// Record how to build a `PPtr<TAsset>` and a `PPtr`-derived `m_Collection` node.
    private static void DumpPptrType(string root)
    {
        Assembly assets = Assembly.LoadFrom(Path.Combine(root, "AssetRipper.Assets.dll"));
        Type? pptr = SafeTypes(assets).FirstOrDefault(t => t.Name == "PPtr`1");
        if (pptr is null)
        {
            Console.WriteLine("PPTRTYPE|missing");
            return;
        }
        Console.WriteLine($"PPTRTYPE|{pptr.FullName}|struct={pptr.IsValueType}");
        foreach (ConstructorInfo ctor in pptr.GetConstructors())
        {
            string parameters = string.Join(
                ", ",
                ctor.GetParameters().Select(p => $"{Short(p.ParameterType)} {p.Name}"));
            Console.WriteLine($"PPTRCTOR|{parameters}");
        }
        foreach (PropertyInfo property in pptr.GetProperties())
        {
            Console.WriteLine($"PPTRPROP|{Short(property.PropertyType)} {property.Name}|set={property.CanWrite}");
        }
        foreach (FieldInfo field in pptr.GetFields())
        {
            Console.WriteLine($"PPTRFIELD|{Short(field.FieldType)} {field.Name}");
        }
    }

    private static void Dump(Type walkerType, string name, Action<object> body)
    {
        var writer = new StringWriter();
        object walker = Activator.CreateInstance(walkerType, writer);
        try
        {
            body(walker);
        }
        catch (Exception ex)
        {
            Console.WriteLine($"TEXT|{name}|<threw {ex.GetType().Name}: {ex.Message.Split('\n')[0]}>");
            return;
        }
        Console.WriteLine($"TEXT|{name}|{Visible(writer.ToString())}");
    }

    /// Render with escapes so each recorded text is a single readable line.
    private static string Visible(string text) => text
        .Replace("\\", "\\\\")
        .Replace("\r", "\\r")
        .Replace("\n", "\\n")
        .Replace("\t", "\\t");

    private static void Void(object walker, string name, params object[] args) =>
        Invoke(walker, name, null, args);

    /// The `EnterAsset` / `DivideAsset` / `ExitAsset` family, which all take one asset argument.
    ///
    /// A separate helper because `Void(w, "EnterAsset", null)` binds `null` to the `params` array
    /// itself rather than to its single element, so the walker would be called with no argument at
    /// all. `EnterAsset` ignores the value, so `null` is a valid asset here.
    private static void AssetVoid(object walker, string name) =>
        Invoke(walker, name, null, new object[] { null });

    /// `VisitPrimitive<T>` with the element type supplied explicitly.
    private static void Primitive(object walker, Type type, object value) =>
        Invoke(walker, "VisitPrimitive", new[] { type }, new[] { value });

    /// Any `Enter*`, `Divide*`, `Exit*` or pair method whose generic arity is in `typeArguments`.
    private static object Generic(object walker, string name, Type[] typeArguments, params object[] args) =>
        Invoke(walker, name, typeArguments, args);

    private static object Invoke(object walker, string name, Type[] typeArguments, object[] args)
    {
        args ??= Array.Empty<object>();
        MethodInfo method = walker.GetType()
            .GetMethods(BindingFlags.Public | BindingFlags.Instance)
            .Where(m => m.Name == name && m.GetParameters().Length == args.Length)
            .Where(m => m.IsGenericMethodDefinition == (typeArguments is not null))
            .FirstOrDefault()
            ?? throw new MissingMethodException($"{name}/{args.Length} generic={typeArguments is not null}");
        if (method.IsGenericMethodDefinition)
        {
            method = method.MakeGenericMethod(typeArguments!);
        }
        return method.Invoke(walker, args);
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

    private static string Short(Type type)
    {
        if (!type.IsGenericType)
        {
            return type.Name;
        }
        string name = type.Name[..type.Name.IndexOf('`')];
        return $"{name}<{string.Join(",", type.GetGenericArguments().Select(Short))}>";
    }
}
