using System.Reflection;
using System.Text;
using AssetRipper.Assets;
using AssetRipper.Assets.Collections;
using AssetRipper.Assets.IO.Writing;
using AssetRipper.IO.Endian;
using AssetRipper.IO.Files;
using AssetRipper.IO.Files.SerializedFiles;
using AssetRipper.IO.Files.SerializedFiles.Parser;

// Reference-runtime probe for the animation processing workstream.
//
// Modes:
//   fieldhashes <out>   compact dump of the generated FieldHashes tables
//   props <TypeName>    list public members of a shipped type
//   types <fragment>    list shipped type names containing a fragment
//   convert <out>       build a real AnimationClip graph, run the shipped
//                       AnimationClipConverter, and dump the complete result
//   workspace <out> <serialized-file>
//                       write the same generated non-default graph as a no-TypeTree
//                       SerializedFile before conversion
public static class Program
{
    private static Assembly Sg = null!;
    private static Assembly AssetsAsm = null!;
    private static Assembly ProcessingAsm = null!;

    public static int Main(string[] args)
    {
        Sg = typeof(AssetRipper.SourceGenerated.Classes.ClassID_74.AnimationClip).Assembly;
        AssetsAsm = typeof(AssetRipper.Assets.Metadata.AssetInfo).Assembly;
        ProcessingAsm = typeof(AssetRipper.Processing.AnimationClips.AnimationClipConverter).Assembly;
        string mode = args.Length > 0 ? args[0] : "fieldhashes";
        switch (mode)
        {
            case "fieldhashes":
                return DumpFieldHashes(args[1]);
            case "props":
                return DumpProps(args[1]);
            case "types":
                return DumpTypes(args.Length > 1 ? args[1] : "AnimationClip");
            case "convert":
                try
                {
                    return RunConvert(args[1]);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine(ex.ToString());
                    return 3;
                }
            case "workspace":
                try
                {
                    return RunConvert(args[1], args[2]);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine(ex.ToString());
                    return 3;
                }
            default:
                Console.Error.WriteLine($"unknown mode {mode}");
                return 2;
        }
    }

    private static Type Resolve(string name)
    {
        foreach (Assembly asm in new[] { Sg, AssetsAsm, ProcessingAsm })
        {
            Type? type = asm.GetType(name);
            if (type is not null)
            {
                return type;
            }
        }
        foreach (Assembly asm in new[] { Sg, AssetsAsm, ProcessingAsm })
        {
            Type? type = asm.GetTypes().FirstOrDefault(t => t.Name == name);
            if (type is not null)
            {
                return type;
            }
        }
        throw new InvalidOperationException($"type {name} not found");
    }

    private static int DumpTypes(string fragment)
    {
        foreach (Type type in Sg.GetTypes().Where(t => (t.FullName ?? t.Name).Contains(fragment, StringComparison.Ordinal)).OrderBy(t => t.FullName))
        {
            Console.WriteLine(type.FullName);
        }
        return 0;
    }

    private static int DumpProps(string name)
    {
        Type type = Resolve(name);
        Console.WriteLine($"# {type.FullName} (base {type.BaseType?.FullName})");
        foreach (PropertyInfo prop in type.GetProperties(BindingFlags.Public | BindingFlags.Instance).OrderBy(p => p.Name))
        {
            Console.WriteLine($"P|{prop.Name}|{prop.PropertyType.FullName ?? prop.PropertyType.Name}|{(prop.CanWrite ? "rw" : "r")}");
        }
        foreach (MethodInfo method in type.GetMethods(BindingFlags.Public | BindingFlags.Instance).Where(m => m.Name.StartsWith("Has_")).OrderBy(m => m.Name))
        {
            Console.WriteLine($"H|{method.Name}");
        }
        return 0;
    }

    private static int DumpFieldHashes(string output)
    {
        var pool = new List<string>();
        var poolIndex = new Dictionary<string, int>();
        var classes = new List<(int ClassId, List<(uint Hash, int Index)> Rows)>();
        int entries = 0;
        foreach (Type type in Sg.GetTypes())
        {
            string? ns = type.Namespace;
            if (ns is null || !ns.StartsWith("AssetRipper.SourceGenerated.Classes.ClassID_"))
            {
                continue;
            }
            string suffix = ns["AssetRipper.SourceGenerated.Classes.ClassID_".Length..];
            if (!int.TryParse(suffix, out int classId))
            {
                continue;
            }
            FieldInfo? field = type.GetField("s_FieldPathDictionary", BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.DeclaredOnly);
            if (field?.GetValue(null) is not System.Collections.IDictionary dict)
            {
                continue;
            }
            var rows = new List<(uint Hash, int Index)>();
            foreach (System.Collections.DictionaryEntry entry in dict)
            {
                string path = (string)entry.Value!;
                if (!poolIndex.TryGetValue(path, out int index))
                {
                    index = pool.Count;
                    poolIndex.Add(path, index);
                    pool.Add(path);
                }
                rows.Add(((uint)entry.Key!, index));
            }
            rows.Sort((a, b) => a.Hash.CompareTo(b.Hash));
            entries += rows.Count;
            classes.Add((classId, rows));
        }
        classes.Sort((a, b) => a.ClassId.CompareTo(b.ClassId));
        var builder = new StringBuilder();
        builder.Append("# FieldHashes table dumped from the shipped AssetRipper.SourceGenerated.dll.\n");
        builder.Append("# Pool lines: P|<index>|<path>\n");
        builder.Append("# Class lines: C|<classId>|<hash>:<poolIndex>,... (hash ascending)\n");
        for (int i = 0; i < pool.Count; i++)
        {
            builder.Append("P|").Append(i).Append('|').Append(pool[i]).Append('\n');
        }
        foreach ((int classId, List<(uint Hash, int Index)> rows) in classes)
        {
            builder.Append("C|").Append(classId).Append('|');
            for (int i = 0; i < rows.Count; i++)
            {
                if (i > 0)
                {
                    builder.Append(',');
                }
                builder.Append(rows[i].Hash).Append(':').Append(rows[i].Index);
            }
            builder.Append('\n');
        }
        File.WriteAllText(output, builder.ToString());
        Console.WriteLine($"wrote {output} classes={classes.Count} entries={entries} pool={pool.Count}");
        return 0;
    }

    // ---------------------------------------------------------------------------------
    // Reflection helpers
    // ---------------------------------------------------------------------------------

    private static object? Get(object target, string name) =>
        target.GetType().GetProperty(name)?.GetValue(target);

    private static bool Gate(object target, string name)
    {
        MethodInfo? method = target.GetType().GetMethod(name, BindingFlags.Public | BindingFlags.Instance);
        return method is not null && (bool)method.Invoke(target, null)!;
    }

    private static object? HasValue(object target, string name)
    {
        MethodInfo? method = target.GetType().GetMethod("Has_" + name, BindingFlags.Public | BindingFlags.Instance);
        if (method is not null && !(bool)method.Invoke(target, null)!)
        {
            return null;
        }
        return Get(target, name);
    }

    private static void Set(object target, string name, object? value)
    {
        PropertyInfo? prop = target.GetType().GetProperty(name);
        if (prop is not null && prop.CanWrite)
        {
            prop.SetValue(target, ConvertValue(prop.PropertyType, value));
            return;
        }
        for (Type? bt = target.GetType(); bt is not null; bt = bt.BaseType)
        {
            FieldInfo? field = bt.GetField(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly);
            if (field is not null)
            {
                field.SetValue(target, ConvertValue(field.FieldType, value));
                return;
            }
        }
        if (prop is not null)
        {
            prop.GetSetMethod(true)?.Invoke(target, new[] { ConvertValue(prop.PropertyType, value) });
            return;
        }
        throw new InvalidOperationException($"no settable {name} on {target.GetType().FullName}");
    }

    private static object? ConvertValue(Type type, object? value)
    {
        if (value is null)
        {
            return null;
        }
        if (type.IsInstanceOfType(value))
        {
            return value;
        }
        if (type.IsEnum)
        {
            return Enum.ToObject(type, value);
        }
        if (type == typeof(AssetRipper.Primitives.Utf8String))
        {
            return new AssetRipper.Primitives.Utf8String((string)value);
        }
        return Convert.ChangeType(value, type);
    }

    private static void SetUtf8(object target, string name, string value) =>
        Set(target, name, value);

    private static object AddNew(object list) =>
        list.GetType().GetMethod("AddNew")!.Invoke(list, null)!;

    private static void AddItem(object list, object value) =>
        list.GetType().GetMethod("Add")!.Invoke(list, new[] { value });

    private static void Clear(object list) =>
        list.GetType().GetMethod("Clear")!.Invoke(list, null);

    private static int Count(object list) =>
        (int)list.GetType().GetProperty("Count")!.GetValue(list)!;

    private static object At(object list, int index)
    {
        PropertyInfo? indexer = list.GetType().GetProperty("Item");
        return indexer!.GetValue(list, new object[] { index })!;
    }

    private static void SetAsset(object pptr, object? asset)
    {
        MethodInfo? method = pptr.GetType().GetMethod("SetAsset", new[] { typeof(AssetRipper.Assets.Collections.AssetCollection), typeof(AssetRipper.Assets.IUnityObjectBase) });
        if (method is not null)
        {
            method.Invoke(pptr, new[] { asset is null ? null : GetCollection(asset), asset });
            return;
        }
        long pathId = asset is null ? 0 : Convert.ToInt64(asset.GetType().GetProperty("PathID")!.GetValue(asset)!);
        TrySetInt(pptr, "FileID_", 0);
        TrySetInt(pptr, "m_FileID_", 0);
        if (TrySetPathId(pptr, pathId))
        {
            return;
        }
        throw new InvalidOperationException($"no PathID slot on {pptr.GetType().FullName}");
    }

    private static void TrySetInt(object target, string name, int value)
    {
        try
        {
            Set(target, name, value);
        }
        catch (InvalidOperationException)
        {
        }
    }

    private static bool TrySetPathId(object pptr, long pathId)
    {
        foreach (string slot in new[] { "PathID", "PathID__Int32", "PathID__Int64", "m_PathID_", "m_PathID" })
        {
            try
            {
                Set(pptr, slot, slot == "PathID__Int32" ? (int)pathId : pathId);
                foreach (string read in new[] { "PathID", "PathID__Int32", "PathID__Int64" })
                {
                    object? value = Get(pptr, read);
                    if (value is not null && Convert.ToInt64(value) == pathId)
                    {
                        return true;
                    }
                }
            }
            catch (Exception)
            {
            }
        }
        return false;
    }

    private static object GetCollection(object asset) =>
        asset.GetType().GetProperty("Collection")!.GetValue(asset)!;

    private static object NewAsset(Type factoryType, object collection, long pathId, int classId, object version)
    {
        object asset = factoryType.GetMethod("Create", new[] { typeof(AssetRipper.Assets.Metadata.AssetInfo), typeof(AssetRipper.Primitives.UnityVersion) })!
            .Invoke(null, new object[] { new AssetRipper.Assets.Metadata.AssetInfo((AssetRipper.Assets.Collections.AssetCollection)collection, pathId, classId), version })!;
        collection.GetType().GetMethod("AddAsset", BindingFlags.NonPublic | BindingFlags.Instance)!
            .Invoke(collection, new object[] { asset });
        return asset;
    }

    private static uint Bits(float value) => BitConverter.SingleToUInt32Bits(value);

    private static string Hex(float value) => Bits(value).ToString("X8");

    private static string Hex(uint value) => value.ToString("X8");

    private static void AppendVec(StringBuilder builder, object vec, string[] names)
    {
        foreach (string name in names)
        {
            builder.Append('|').Append(Hex(Convert.ToSingle(Get(vec, name))));
        }
    }

    private static uint Crc32(string text)
    {
        Type checksum = Type.GetType("AssetRipper.Checksum.Crc32Algorithm, AssetRipper.Checksum")!;
        return (uint)checksum.GetMethod("HashUTF8", BindingFlags.Public | BindingFlags.Static, null, new[] { typeof(string) }, null)!.Invoke(null, new object[] { text })!;
    }

    private static byte[] StreamedFrame(float time, (int Index, float Cx, float Cy, float Cz, float Limit)[] keys)
    {
        using var stream = new MemoryStream();
        using var writer = new BinaryWriter(stream);
        writer.Write(time);
        writer.Write(keys.Length);
        foreach ((int index, float cx, float cy, float cz, float limit) in keys)
        {
            writer.Write(index);
            writer.Write(cx);
            writer.Write(cy);
            writer.Write(cz);
            writer.Write(limit);
        }
        return stream.ToArray();
    }

    // ---------------------------------------------------------------------------------
    // Conversion differential
    // ---------------------------------------------------------------------------------

    private static int RunConvert(string output, string? serializedWorkspace = null)
    {
        var bundle = new AssetRipper.Assets.Bundles.GameBundle();
        var collection = new AssetRipper.Assets.Collections.ProcessedAssetCollection(bundle);
        object version = AssetRipper.Primitives.UnityVersion.Parse("2022.3.0f1");
        collection.SetLayout(AssetRipper.Primitives.UnityVersion.Parse("2022.3.0f1"));

        // ---- materialized scene graph -------------------------------------------------
        object rootGo = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_1.GameObject), collection, 1010, 1, version);
        object armGo = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_1.GameObject), collection, 1011, 1, version);
        object legGo = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_1.GameObject), collection, 1012, 1, version);
        object rootTr = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_4.Transform), collection, 1020, 4, version);
        object armTr = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_4.Transform), collection, 1021, 4, version);
        object legTr = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_4.Transform), collection, 1022, 4, version);
        object skin = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_137.SkinnedMeshRenderer), collection, 1030, 137, version);
        object mesh = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_43.Mesh), collection, 1040, 43, version);
        object renderer = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_23.MeshRenderer), collection, 1050, 23, version);
        object material = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_21.Material), collection, 1060, 21, version);
        object controller = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_91.AnimatorController), collection, 1080, 91, version);
        object animator = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_95.Animator), collection, 1090, 95, version);

        SetUtf8(rootGo, "Name_R", "Root");
        SetUtf8(armGo, "Name_R", "Arm");
        SetUtf8(legGo, "Name_R", "Leg");

        Set(rootTr, "GameObject_C4P", rootGo);
        Set(armTr, "GameObject_C4P", armGo);
        Set(legTr, "GameObject_C4P", legGo);
        Set(armTr, "Father_C4P", rootTr);
        Set(legTr, "Father_C4P", rootTr);
        AddComponent(rootGo, rootTr, 4);
        AddComponent(armGo, armTr, 4);
        AddComponent(legGo, legTr, 4);
        object rootChildren = Get(rootTr, "Children_C4")!;
        SetAsset(AddNew(rootChildren), armTr);
        SetAsset(AddNew(rootChildren), legTr);

        // Arm carries the skinned mesh (whose blend-shape lookup misses), Leg the
        // mesh renderer (whose material-property lookup misses). Both walks therefore
        // reach the component and fall through to the ReverseAscii fallback.
        object armComponents = Get(armGo, "Components")!;
        object armPair = AddNew(armComponents);
        Set(armPair, "ClassID", 137);
        SetAsset(Get(armPair, "Component")!, skin);
        Set(skin, "GameObjectP", armGo);
        Set(skin, "MeshP", mesh);

        object legComponents = Get(legGo, "Components")!;
        object legPair = AddNew(legComponents);
        Set(legPair, "ClassID", 23);
        SetAsset(Get(legPair, "Component")!, renderer);
        Set(renderer, "GameObjectP", legGo);
        SetAsset(AddNew(Get(renderer, "Materials")!), material);

        // ---- the clip -----------------------------------------------------------------
        object clip = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_74.AnimationClip), collection, 1000, 74, version);
        Set(clip, "SampleRate_C74", 30.0f);

        string[] paths =
        {
            "Root", "Root", "Root", "Arm", "Root", "Leg", "Root", "Root",
            "Arm", "Leg", "Leg", "Leg", "Root", "Root", "Root", "Root",
            "Root", "Root", "Root", "Root", "Leg", "Leg", "Root",
        };
        (int ClassId, uint Attribute, byte CustomType, byte IsPPtr)[] bindingSpecs =
        {
            (4, 1, 4, 0),                          // 0 translation
            (4, 2, 4, 0),                          // 1 rotation
            (4, 3, 4, 0),                          // 2 scaling
            (4, 4, 2, 0),                          // 3 euler (rotation order 2)
            (1, Crc32("m_IsActive"), 0, 0),        // 4 GameObject default hit
            (23, Crc32("m_Enabled"), 0, 0),        // 5 engine default hit
            (23, 0xDEADBEEFu, 0, 0),               // 6 engine default miss
            (114, Crc32("myField"), 0, 0),         // 7 script default, checksum miss
            (137, 0xC34B2C0Bu, 20, 0),             // 8 blend shape (scene walk misses)
            (23, 0x8686C589u, 22, 0),              // 9 renderer material (scene walk misses)
            (23, 3, 21, 0),                        // 10 renderer array
            (212, 0, 23, 0),                       // 11 sprite renderer
            (114, 0x12345678u, 40, 0),             // 12 user defined
            (181, 4, 31, 0),                       // 13 position constraint
            (95, 42, 8, 0),                        // 14 animator muscle
            (198, 0x924284C0u, 27, 0),             // 15 particle system
            (20, 0x9B5A57ECu, 0, 1),               // 16 PPtr engine curve
            (224, 1, 4, 0),                        // 17 rect-transform translation
            (4, 1, 4, 0),                          // 18 dense translation
            (23, Crc32("m_ReceiveShadows"), 0, 0), // 19 spacer
            (23, Crc32("m_LightmapIndex"), 0, 0),  // 20 spacer
            (23, Crc32("m_CastShadows"), 0, 0),    // 21 constant engine default
            (114, 0x9299AC34u, 24, 0),             // 22 constant MonoBehaviour hit
        };

        object bindingConstant = Get(clip, "ClipBindingConstant_C74")!;
        object genericBindings = Get(bindingConstant, "GenericBindings")!;
        Clear(genericBindings);
        for (int i = 0; i < bindingSpecs.Length; i++)
        {
            object binding = AddNew(genericBindings);
            Set(binding, "Path", Crc32(paths[i]));
            Set(binding, "Attribute", bindingSpecs[i].Attribute);
            if (Gate(binding, "Has_ClassID_UInt16"))
            {
                Set(binding, "ClassID_UInt16", (ushort)bindingSpecs[i].ClassId);
            }
            else
            {
                Set(binding, "ClassID_Int32", bindingSpecs[i].ClassId);
            }
            Set(binding, "CustomType", bindingSpecs[i].CustomType);
            Set(binding, "IsPPtrCurve", bindingSpecs[i].IsPPtr);
        }
        object mapping = Get(bindingConstant, "PptrCurveMapping")!;
        Clear(mapping);
        SetAsset(AddNew(mapping), rootGo);
        SetAsset(AddNew(mapping), null);

        // ---- streamed data ------------------------------------------------------------
        // Binding-cache positions: 0-2 translation, 3-6 rotation, 7-9 scaling, 10-12 euler,
        // 13 GameObject, 14 engine hit, 15 engine miss, 16 script, 17 blend shape,
        // 18 material, 19 renderer, 20 sprite, 21 user defined, 22 constraint, 23 muscle,
        // 24 particle system, 25 PPtr, 26 rect-transform translation, 27-29 dense
        // translation, 30/31 spacers, 32 constant engine, 33 constant MonoBehaviour.
        var frame0 = new (int, float, float, float, float)[] { (25, 0f, 0f, 0f, 0f) };
        var frame1 = new List<(int, float, float, float, float)>
        {
            (0, 0.1f, 0.2f, 0.3f, 1.0f),
            (1, 0f, 0f, 0f, 2.0f),
            (2, 0f, 0f, 0f, 3.0f),
            (3, 0.1f, 0.2f, 0.3f, 0.5f),
        };
        for (int i = 4; i <= 24; i++)
        {
            frame1.Add((i, 0f, 0f, 0f, 10.0f + i));
        }
        for (int i = 26; i <= 28; i++)
        {
            frame1.Add((i, 0f, 0f, 0f, 40.0f + i));
        }
        var frame2 = new List<(int, float, float, float, float)>
        {
            // Consecutive duplicate: the collapse keeps the last key.
            (13, 0f, 0f, 0f, 99.0f),
            (13, 0f, 0f, 0f, 8.0f),
            (0, 0f, 0f, 0f, 1.5f),
            (1, 0f, 0f, 0f, 2.0f),
            (2, 0f, 0f, 0f, 4.0f),
        };
        for (int i = 3; i <= 24; i++)
        {
            frame2.Add((i, 0f, 0f, 0f, 20.0f + i));
        }
        for (int i = 26; i <= 28; i++)
        {
            frame2.Add((i, 0f, 0f, 0f, 50.0f + i));
        }
        var streamed = new List<byte>();
        streamed.AddRange(StreamedFrame(float.MinValue, frame0));
        streamed.AddRange(StreamedFrame(1.0f / 30.0f, frame1.ToArray()));
        streamed.AddRange(StreamedFrame(2.0f / 30.0f, frame2.ToArray()));
        streamed.AddRange(StreamedFrame(float.PositiveInfinity, Array.Empty<(int, float, float, float, float)>()));

        object muscle = Get(clip, "MuscleClip_C74")!;
        object clipData = Get(muscle, "Clip")!;
        object clipValue = Get(clipData, "Data")!;
        object streamedClip = Get(clipValue, "StreamedClip")!;
        object streamedData = Get(streamedClip, "Data")!;
        Clear(streamedData);
        byte[] streamedBytes = streamed.ToArray();
        for (int i = 0; i < streamedBytes.Length; i += 4)
        {
            AddItem(streamedData, BitConverter.ToUInt32(streamedBytes, i));
        }
        Set(streamedClip, "CurveCount_UInt32", 29u);

        object dense = Get(clipValue, "DenseClip")!;
        Set(dense, "FrameCount", 2);
        Set(dense, "CurveCount", 3u);
        Set(dense, "SampleRate", 30.0f);
        Set(dense, "BeginTime", 0.5f);
        object denseSamples = Get(dense, "SampleArray")!;
        Clear(denseSamples);
        for (int i = 0; i < 6; i++)
        {
            AddItem(denseSamples, 100.0f + i);
        }

        object constant = Get(clipValue, "ConstantClip")!;
        object constantData = Get(constant, "Data")!;
        Clear(constantData);
        AddItem(constantData, 777.0f);
        AddItem(constantData, 888.0f);

        Set(muscle, "StopTime", 2.0f);
        Set(muscle, "StartTime", 0.0f);
        Set(muscle, "LoopTime", true);
        Set(muscle, "Level", 0.25f);

        // The clip must be reachable from an animator so FindRoots sees the scene.
        SetAsset(AddNew(Get(controller, "AnimationClips")!), clip);
        AddComponent(rootGo, animator, 95);
        Set(animator, "GameObjectP", rootGo);
        Set(animator, "Controller_PPtr_RuntimeAnimatorController_5P", controller);
        if (Get(animator, "HasTransformHierarchy") is not null)
        {
            Set(animator, "HasTransformHierarchy", true);
        }

        // Events must survive conversion untouched.
        object events = Get(clip, "Events_C74")!;
        object animationEvent = AddNew(events);
        SetUtf8(animationEvent, "FunctionName", "OnThing");
        SetUtf8(animationEvent, "Data", "payload");
        Set(animationEvent, "FloatParameter", 1.5f);
        Set(animationEvent, "IntParameter", 7);
        Set(animationEvent, "Time", 0.75f);

        if (serializedWorkspace is not null)
        {
            WriteSerializedWorkspace(
                serializedWorkspace,
                collection,
                version,
                [rootGo, armGo, legGo, rootTr, armTr, legTr, skin, mesh, renderer, material, controller, animator, clip]);
        }

        var checksumType = Type.GetType("AssetRipper.Processing.AnimationClips.PathChecksumCache, AssetRipper.Processing")!;
        var gameDataType = Type.GetType("AssetRipper.Processing.GameData, AssetRipper.Processing")!;
        object gameData = Activator.CreateInstance(gameDataType, bundle, version, null!, null!)!;
        object checksumCache = Activator.CreateInstance(checksumType, gameData)!;
        typeof(AssetRipper.Processing.AnimationClips.AnimationClipConverter)
            .GetMethod("Process", BindingFlags.Public | BindingFlags.Static)!
            .Invoke(null, new[] { clip, checksumCache });

        DumpClip(clip, output);
        return 0;
    }

    private static void WriteSerializedWorkspace(
        string output,
        object collection,
        object version,
        IReadOnlyList<object> assets)
    {
        var builder = new SerializedFileBuilder
        {
            Generation = FormatVersion.Unknown_10,
            Version = (AssetRipper.Primitives.UnityVersion)version,
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = false,
        };

        Dictionary<int, SerializedType> typesByClassId = new();
        foreach (object asset in assets.OrderBy(asset => Convert.ToInt64(Get(asset, "PathID"))))
        {
            int classId = Convert.ToInt32(Get(asset, "ClassID"));
            long pathId = Convert.ToInt64(Get(asset, "PathID"));
            if (!typesByClassId.TryGetValue(classId, out SerializedType? type))
            {
                type = new SerializedType { RawTypeID = classId };
                typesByClassId.Add(classId, type);
                builder.Types.Add(type);
            }

            using MemoryStream payload = new();
            using (AssetWriter writer = new(payload, (AssetCollection)collection))
            {
                ((IUnityObjectBase)asset).WriteRelease(writer);
            }

            builder.Objects.Add(new ObjectInfo(type)
            {
                FileID = pathId,
                TypeID = classId,
                ObjectData = payload.ToArray(),
            });
        }

        AssetRipper.IO.Files.SerializedFiles.SerializedFile file = builder.Build();
        file.FilePath = "animation_workspace.assets";
        string fullPath = Path.GetFullPath(output);
        Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
        File.WriteAllBytes(fullPath, file.ToByteArray());
        Console.WriteLine($"wrote source-generated workspace {fullPath} objects={assets.Count} bytes={new FileInfo(fullPath).Length}");
    }

    private static void AddComponent(object gameObject, object component, int classId)
    {
        object pair = AddNew(Get(gameObject, "Components")!);
        Set(pair, "ClassID", classId);
        SetAsset(Get(pair, "Component")!, component);
    }

    private static void DumpClip(object clip, string output)
    {
        var builder = new StringBuilder();
        builder.Append("# AnimationClipConverter.Process output over a real generated clip.\n");
        builder.Append("# Probe tools/probes/anim-convert (convert mode), shipped AssetRipper.Processing.\n");
        builder.Append("# All floats are bit patterns; envelope rotation order -1 means the field is absent.\n");

        builder.Append("INFO|sample_rate|").Append(Hex(Convert.ToSingle(Get(clip, "SampleRate_C74")))).Append('\n');
        object info = Get(clip, "MuscleClipInfo_C74")!;
        builder.Append("MINFO");
        builder.Append('|').Append(Hex(Convert.ToSingle(Get(info, "StartTime"))));
        builder.Append('|').Append(Hex(Convert.ToSingle(Get(info, "StopTime"))));
        builder.Append('|').Append(Hex(Convert.ToSingle(Get(info, "OrientationOffsetY"))));
        builder.Append('|').Append(Hex(Convert.ToSingle(Get(info, "Level"))));
        builder.Append('|').Append(Hex(Convert.ToSingle(Get(info, "CycleOffset"))));
        builder.Append('|').Append(Hex(Convert.ToSingle(Get(info, "AdditiveReferencePoseTime"))));
        foreach (string flag in new[]
        {
            "LoopTime", "LoopBlend", "LoopBlendOrientation", "LoopBlendPositionY",
            "LoopBlendPositionXZ", "KeepOriginalOrientation", "KeepOriginalPositionY",
            "KeepOriginalPositionXZ", "HeightFromFeet", "HasAdditiveReferencePose", "Mirror",
        })
        {
            object? value = HasValue(info, flag);
            builder.Append('|').Append(value is bool b && b ? "1" : "0");
        }
        builder.Append('\n');

        object events = Get(clip, "Events_C74")!;
        for (int i = 0; i < Count(events); i++)
        {
            object e = At(events, i);
            builder.Append("EVENT|").Append(i)
                .Append('|').Append(Get(e, "FunctionName"))
                .Append('|').Append(Get(e, "Data"))
                .Append('|').Append(Hex(Convert.ToSingle(Get(e, "FloatParameter"))))
                .Append('|').Append(Get(e, "IntParameter"))
                .Append('|').Append(Hex(Convert.ToSingle(Get(e, "Time"))))
                .Append('\n');
        }

        DumpVector3Curves(builder, clip, "POS", "PositionCurves_C74");
        DumpQuaternionCurves(builder, clip, "ROT", "RotationCurves_C74");
        DumpVector3Curves(builder, clip, "SCL", "ScaleCurves_C74");
        DumpVector3Curves(builder, clip, "EUL", "EulerCurves_C74");
        DumpFloatCurves(builder, clip);
        DumpPPtrCurves(builder, clip);

        File.WriteAllText(output, builder.ToString());
        Console.WriteLine($"wrote {output}");
    }

    private static void DumpVector3Curves(StringBuilder builder, object clip, string tag, string prop)
    {
        object list = Get(clip, prop)!;
        for (int i = 0; i < Count(list); i++)
        {
            object curve = At(list, i);
            object envelope = Get(curve, "Curve")!;
            builder.Append(tag).Append('|').Append(i)
                .Append('|').Append(Get(curve, "Path"))
                .Append('|').Append(Convert.ToInt32(Get(envelope, "PreInfinity")))
                .Append('|').Append(Convert.ToInt32(Get(envelope, "PostInfinity")))
                .Append('|').Append(Gate(envelope, "Has_RotationOrder") ? Convert.ToInt32(Get(envelope, "RotationOrder")) : -1)
                .Append('\n');
            object keys = Get(envelope, "Curve")!;
            for (int k = 0; k < Count(keys); k++)
            {
                object key = At(keys, k);
                builder.Append(tag).Append("K|").Append(i)
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "Time"))));
                AppendVec(builder, Get(key, "Value")!, new[] { "X", "Y", "Z" });
                AppendVec(builder, Get(key, "InSlope")!, new[] { "X", "Y", "Z" });
                AppendVec(builder, Get(key, "OutSlope")!, new[] { "X", "Y", "Z" });
                AppendVec(builder, HasValue(key, "InWeight") ?? Get(key, "InSlope")!, new[] { "X", "Y", "Z" });
                AppendVec(builder, HasValue(key, "OutWeight") ?? Get(key, "OutSlope")!, new[] { "X", "Y", "Z" });
                builder.Append('|').Append(Convert.ToInt32(Get(key, "TangentMode")));
                object? weighted = HasValue(key, "WeightedMode");
                builder.Append('|').Append(weighted is null ? -1 : Convert.ToInt32(weighted));
                builder.Append('\n');
            }
        }
    }

    private static void DumpQuaternionCurves(StringBuilder builder, object clip, string tag, string prop)
    {
        object list = Get(clip, prop)!;
        for (int i = 0; i < Count(list); i++)
        {
            object curve = At(list, i);
            object envelope = Get(curve, "Curve")!;
            builder.Append(tag).Append('|').Append(i)
                .Append('|').Append(Get(curve, "Path"))
                .Append('|').Append(Convert.ToInt32(Get(envelope, "PreInfinity")))
                .Append('|').Append(Convert.ToInt32(Get(envelope, "PostInfinity")))
                .Append('|').Append(Gate(envelope, "Has_RotationOrder") ? Convert.ToInt32(Get(envelope, "RotationOrder")) : -1)
                .Append('\n');
            object keys = Get(envelope, "Curve")!;
            for (int k = 0; k < Count(keys); k++)
            {
                object key = At(keys, k);
                builder.Append(tag).Append("K|").Append(i)
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "Time"))));
                AppendVec(builder, Get(key, "Value")!, new[] { "X", "Y", "Z", "W" });
                AppendVec(builder, Get(key, "InSlope")!, new[] { "X", "Y", "Z", "W" });
                AppendVec(builder, Get(key, "OutSlope")!, new[] { "X", "Y", "Z", "W" });
                AppendVec(builder, HasValue(key, "InWeight") ?? Get(key, "InSlope")!, new[] { "X", "Y", "Z", "W" });
                AppendVec(builder, HasValue(key, "OutWeight") ?? Get(key, "OutSlope")!, new[] { "X", "Y", "Z", "W" });
                builder.Append('|').Append(Convert.ToInt32(Get(key, "TangentMode")));
                object? weighted = HasValue(key, "WeightedMode");
                builder.Append('|').Append(weighted is null ? -1 : Convert.ToInt32(weighted));
                builder.Append('\n');
            }
        }
    }

    private static void DumpFloatCurves(StringBuilder builder, object clip)
    {
        object list = Get(clip, "FloatCurves_C74")!;
        for (int i = 0; i < Count(list); i++)
        {
            object curve = At(list, i);
            object envelope = Get(curve, "Curve")!;
            object? script = Get(curve, "Script");
            builder.Append("FLOAT|").Append(i)
                .Append('|').Append(Get(curve, "Path"))
                .Append('|').Append(Get(curve, "Attribute"))
                .Append('|').Append(Convert.ToInt32(Get(curve, "ClassID")))
                .Append('|').Append(script is null ? -1 : Convert.ToInt64(Get(script, "PathID__Int64")))
                .Append('|').Append(Convert.ToInt32(Get(envelope, "PreInfinity")))
                .Append('|').Append(Convert.ToInt32(Get(envelope, "PostInfinity")))
                .Append('|').Append(Gate(envelope, "Has_RotationOrder") ? Convert.ToInt32(Get(envelope, "RotationOrder")) : -1)
                .Append('\n');
            object keys = Get(envelope, "Curve")!;
            for (int k = 0; k < Count(keys); k++)
            {
                object key = At(keys, k);
                builder.Append("FK|").Append(i)
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "Time"))))
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "Value"))))
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "InSlope"))))
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "OutSlope"))));
                object? inWeight = HasValue(key, "InWeight");
                object? outWeight = HasValue(key, "OutWeight");
                builder.Append('|').Append(inWeight is null ? "-1" : Hex(Convert.ToSingle(inWeight)));
                builder.Append('|').Append(outWeight is null ? "-1" : Hex(Convert.ToSingle(outWeight)));
                builder.Append('|').Append(Convert.ToInt32(Get(key, "TangentMode")));
                object? weighted = HasValue(key, "WeightedMode");
                builder.Append('|').Append(weighted is null ? -1 : Convert.ToInt32(weighted));
                builder.Append('\n');
            }
        }
    }

    private static void DumpPPtrCurves(StringBuilder builder, object clip)
    {
        object list = Get(clip, "PPtrCurves_C74")!;
        for (int i = 0; i < Count(list); i++)
        {
            object curve = At(list, i);
            builder.Append("PPTR|").Append(i)
                .Append('|').Append(Get(curve, "Path"))
                .Append('|').Append(Get(curve, "Attribute"))
                .Append('|').Append(Convert.ToInt32(Get(curve, "ClassID")))
                .Append('|').Append(Convert.ToInt32(Get(curve, "Flags")))
                .Append('\n');
            object keys = Get(curve, "Curve")!;
            for (int k = 0; k < Count(keys); k++)
            {
                object key = At(keys, k);
                object value = Get(key, "Value")!;
                long pathId = Convert.ToInt64(Get(value, "PathID__Int64"));
                builder.Append("PK|").Append(i)
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "Time"))))
                    .Append('|').Append(Convert.ToInt32(Get(value, "FileID_")))
                    .Append('|').Append(pathId)
                    .Append('\n');
            }
        }
    }
}
