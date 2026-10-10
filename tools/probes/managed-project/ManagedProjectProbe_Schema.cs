using System.Security.Cryptography;
using System.Collections;
using System.Collections.Immutable;
using System.Reflection;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Text;
using System.Text.RegularExpressions;
using AssetRipper.Assets.Bundles;
using AssetRipper.Assets.Collections;
using AssetRipper.Assets.IO.Writing;
using AssetRipper.Assets.Metadata;
using AssetRipper.Checksum;
using AssetRipper.Export.Configuration;
using AssetRipper.Export.UnityProjects;
using AssetRipper.Export.UnityProjects.Scripts;
using AssetRipper.Import.Logging;
using AssetRipper.Import.Structure.Assembly;
using AssetRipper.Import.Structure.Assembly.Managers;
using AssetRipper.Import.Structure.Assembly.TypeTrees;
using AssetRipper.IO.Endian;
using AssetRipper.IO.Files;
using AssetRipper.IO.Files.SerializedFiles;
using AssetRipper.IO.Files.SerializedFiles.Parser;
using AssetRipper.IO.Files.SerializedFiles.Parser.TypeTrees;
using AssetRipper.Primitives;
using AssetRipper.SerializationLogic;

internal static partial class ManagedProjectProbe
{
    internal static int ExportMonoStructure(string inputRootArgument, string outputRootArgument, string rowsArgument)
    {
        string inputRoot = Path.GetFullPath(inputRootArgument);
        string outputRoot = Path.GetFullPath(outputRootArgument);
        string rowsPath = Path.GetFullPath(rowsArgument);
        if (!Directory.Exists(inputRoot))
        {
            throw new DirectoryNotFoundException($"input root does not exist: {inputRoot}");
        }
        EnsureEmptyOrMissingDirectory(outputRoot, "output");
        if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
        {
            throw new IOException($"refusing to overwrite probe rows {rowsPath}");
        }

        Directory.CreateDirectory(outputRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);
        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(new FullConfiguration());
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        string[] monoBehaviours = gameData.GameBundle.FetchAssets()
        .Where(asset => asset.ClassID == 114 && asset.PathID == 42)
        .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
        .ToArray();
        if (monoBehaviours.Length != 1)
        {
            throw new InvalidDataException($"expected one MonoBehaviour at PathID 42, found {monoBehaviours.Length}");
        }
        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

        string projectAssets = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string? monoYamlPath = Directory.Exists(projectAssets)
        ? Directory.GetFiles(projectAssets, "*.asset", SearchOption.AllDirectories)
        .FirstOrDefault(path => File.ReadAllText(path).Contains("m_Name: Behaviour", StringComparison.Ordinal))
        : null;
        if (monoYamlPath is null)
        {
            throw new InvalidDataException("source export did not write the expected MonoBehaviour asset");
        }

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine("# Frozen AssetRipper ExportHandler result for an assembly-backed UnloadedStructure.");
        rows.WriteLine($"OBJECT|{monoBehaviours[0]}");
        foreach (string path in Directory.GetFiles(outputRoot, "*", SearchOption.AllDirectories)
        .OrderBy(path => Path.GetRelativePath(outputRoot, path), StringComparer.Ordinal))
        {
            string relative = Path.GetRelativePath(outputRoot, path).Replace('\\', '/');
            byte[] bytes = File.ReadAllBytes(path);
            rows.WriteLine($"FILE|{relative}|{bytes.Length}|{Convert.ToHexString(SHA256.HashData(bytes))}");
            if (string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase))
            {
                rows.WriteLine($"CONTENT|{relative}|{Convert.ToBase64String(bytes)}");
            }
        }
        Console.WriteLine($"source_object={monoBehaviours[0]}");
        Console.WriteLine($"mono_behaviour={Path.GetRelativePath(outputRoot, monoYamlPath).Replace('\\', '/')}");
        return 0;
    }

    internal static int ExportManagedSchema(
        string inputRootArgument,
        string outputRootArgument,
        string rowsArgument,
        string assemblyArgument,
        string unityVersionText)
    {
        string inputRoot = Path.GetFullPath(inputRootArgument);
        string outputRoot = Path.GetFullPath(outputRootArgument);
        string rowsPath = Path.GetFullPath(rowsArgument);
        string assemblyPath = Path.GetFullPath(assemblyArgument);
        EnsureEmptyOrMissingDirectory(inputRoot, "input");
        EnsureEmptyOrMissingDirectory(outputRoot, "output");
        if (!File.Exists(assemblyPath))
        {
            throw new FileNotFoundException("managed schema fixture assembly is missing", assemblyPath);
        }
        if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
        {
            throw new IOException($"refusing to overwrite probe rows {rowsPath}");
        }

        Directory.CreateDirectory(inputRoot);
        Directory.CreateDirectory(outputRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);
        string serializedPath = Path.Combine(inputRoot, "managed_schema.assets");
        string inputAssemblyPath = Path.Combine(inputRoot, "ComplexFixtureScripts.dll");
        byte[] serializedBytes = BuildManagedSchemaFile(unityVersionText);
        byte[] assemblyBytes = File.ReadAllBytes(assemblyPath);
        File.WriteAllBytes(serializedPath, serializedBytes);
        File.WriteAllBytes(inputAssemblyPath, assemblyBytes);

        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(new FullConfiguration());
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        var matches = gameData.GameBundle.FetchAssets()
            .Where(asset => (asset.ClassID == 114 && asset.PathID == 42)
                || (asset.ClassID == 115 && asset.PathID == 7)
                || (asset.ClassID == 49 && asset.PathID == 9))
            .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
            .ToArray();
        if (matches.Length != 3)
        {
            throw new InvalidDataException($"expected MonoBehaviour, MonoScript, and TextAsset objects; found {matches.Length}: {string.Join(";", matches)}");
        }
        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

        string assetsRoot = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string? behaviourPath = Directory.GetFiles(assetsRoot, "*.asset", SearchOption.AllDirectories)
            .FirstOrDefault(path => File.ReadAllText(path).Contains("m_Name: RichRoot", StringComparison.Ordinal));
        if (behaviourPath is null)
        {
            throw new InvalidDataException("source project export did not write the RichRoot MonoBehaviour");
        }
        string? targetMetaPath = Directory.GetFiles(assetsRoot, "*.meta", SearchOption.AllDirectories)
            .FirstOrDefault(path => File.ReadAllText(path).Contains("TextScriptImporter:", StringComparison.Ordinal));
        if (targetMetaPath is null)
        {
            throw new InvalidDataException("source project export did not write a TextScriptImporter sidecar");
        }
        string targetPath = targetMetaPath[..^5];
        if (!File.Exists(targetPath))
        {
            throw new FileNotFoundException("TextScriptImporter sidecar has no matching asset", targetPath);
        }
        string targetMeta = File.ReadAllText(targetMetaPath);
        string targetGuid = targetMeta.Split('\n')
            .Select(line => line.TrimEnd('\r'))
            .FirstOrDefault(line => line.StartsWith("guid: ", StringComparison.Ordinal))?[6..]
            ?? throw new InvalidDataException("TextScriptImporter sidecar has no GUID");
        string behaviourText = File.ReadAllText(behaviourPath);
        string targetPointerGuid = $"guid: {targetGuid}";
        int pointerGuidCount = behaviourText.Split(targetPointerGuid, StringSplitOptions.None).Length - 1;
        if (pointerGuidCount != 1)
        {
            throw new InvalidDataException(
                $"expected one TextAsset pointer GUID in MonoBehaviour YAML, found {pointerGuidCount}");
        }
        byte[] normalizedBehaviourBytes = Encoding.UTF8.GetBytes(
            behaviourText.Replace(targetPointerGuid, "guid: __TEXTASSET_GUID__", StringComparison.Ordinal));
        byte[] normalizedTargetMetaBytes = Encoding.UTF8.GetBytes(string.Join("\n",
            File.ReadAllLines(targetMetaPath)
                .Where(line => !line.StartsWith("guid: ", StringComparison.Ordinal)
                    && !line.StartsWith("timeCreated: ", StringComparison.Ordinal))) + "\n");

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine("# Frozen AssetRipper export for a no-TypeTree MonoBehaviour with nested and collection fields.");
        rows.WriteLine("NORMALIZE|MonoBehaviour TextAsset PPtr GUID replaced with __TEXTASSET_GUID__.");
        rows.WriteLine("NORMALIZE|Importer guid and timeCreated fields omitted.");
        rows.WriteLine($"UNITY_VERSION|{unityVersionText}");
        rows.WriteLine($"INPUT_HEX|managed_schema.assets|{Convert.ToHexString(serializedBytes)}");
        rows.WriteLine($"ASSEMBLY|ComplexFixtureScripts.dll|{assemblyBytes.Length}|{Convert.ToHexString(SHA256.HashData(assemblyBytes))}");
        foreach (string item in matches)
        {
            rows.WriteLine($"OBJECT|{item}");
        }
        var monoScript = gameData.GameBundle.FetchAssets()
            .OfType<AssetRipper.SourceGenerated.Classes.ClassID_115.IMonoScript>()
            .Single(script => script.PathID == 7);
        SerializableType behaviour = monoScript.GetBehaviourType(gameData.AssemblyManager)
            ?? throw new InvalidDataException("source MonoScript has no serializable behaviour type");
        var sourcePathCache = new AssetRipper.Processing.AnimationClips.PathChecksumCache(gameData);
        sourcePathCache.Add(monoScript);
        foreach (SerializableType.Field field in behaviour.Fields)
        {
            DumpScriptPath(field, string.Empty, 0, sourcePathCache, rows);
        }
        foreach (string path in new[] { "target", "target.m_FileID", "target.m_PathID" })
        {
            uint checksum = Crc32Algorithm.HashUTF8(path);
            sourcePathCache.TryGetPath(checksum, out string? cachedPath);
            rows.WriteLine($"SCRIPT_PATH_LOOKUP|{path}|{checksum}|{cachedPath ?? "<null>"}");
        }
        string behaviourRelative = Path.GetRelativePath(outputRoot, behaviourPath).Replace('\\', '/');
        rows.WriteLine($"YAML_HEX|{behaviourRelative}|{normalizedBehaviourBytes.Length}|{Convert.ToHexString(SHA256.HashData(normalizedBehaviourBytes))}|{Convert.ToHexString(normalizedBehaviourBytes)}");
        string targetRelative = Path.GetRelativePath(outputRoot, targetPath).Replace('\\', '/');
        byte[] targetBytes = File.ReadAllBytes(targetPath);
        rows.WriteLine($"TARGET_HEX|{targetRelative}|{targetBytes.Length}|{Convert.ToHexString(SHA256.HashData(targetBytes))}|{Convert.ToHexString(targetBytes)}");
        string targetMetaRelative = Path.GetRelativePath(outputRoot, targetMetaPath).Replace('\\', '/');
        rows.WriteLine($"TARGET_META_HEX|{targetMetaRelative}|{normalizedTargetMetaBytes.Length}|{Convert.ToHexString(SHA256.HashData(normalizedTargetMetaBytes))}|{Convert.ToHexString(normalizedTargetMetaBytes)}");
        string pluginDllPath = Path.Combine(assetsRoot, "Plugins", "ComplexFixtureScripts.dll");
        if (!File.Exists(pluginDllPath))
        {
            throw new FileNotFoundException("source project export did not save the managed assembly", pluginDllPath);
        }
        byte[] pluginDllBytes = File.ReadAllBytes(pluginDllPath);
        string pluginDllRelative = Path.GetRelativePath(outputRoot, pluginDllPath).Replace('\\', '/');
        rows.WriteLine($"PLUGIN_DLL|{pluginDllRelative}|{pluginDllBytes.Length}|{Convert.ToHexString(SHA256.HashData(pluginDllBytes))}");
        string pluginMetaPath = pluginDllPath + ".meta";
        if (!File.Exists(pluginMetaPath))
        {
            throw new FileNotFoundException("source project export did not write a PluginImporter sidecar", pluginMetaPath);
        }
        byte[] normalizedPluginMetaBytes = Encoding.UTF8.GetBytes(string.Join("\n",
            File.ReadAllLines(pluginMetaPath)
                .Where(line => !line.StartsWith("guid: ", StringComparison.Ordinal)
                    && !line.StartsWith("timeCreated: ", StringComparison.Ordinal))) + "\n");
        string pluginMetaRelative = Path.GetRelativePath(outputRoot, pluginMetaPath).Replace('\\', '/');
        rows.WriteLine($"PLUGIN_META_HEX|{pluginMetaRelative}|{normalizedPluginMetaBytes.Length}|{Convert.ToHexString(SHA256.HashData(normalizedPluginMetaBytes))}|{Convert.ToHexString(normalizedPluginMetaBytes)}");
        Console.WriteLine($"mono_behaviour={behaviourRelative}");
        Console.WriteLine($"target={targetRelative}");
        return 0;
    }

    private static void DumpScriptPath(
        SerializableType.Field field,
        string parentPath,
        int depth,
        AssetRipper.Processing.AnimationClips.PathChecksumCache cache,
        StreamWriter rows)
    {
        if (depth > 10)
        {
            return;
        }
        if (field.Type.IsPrimitive())
        {
            string path = parentPath + field.Name;
            uint crc = Crc32Algorithm.HashUTF8(path);
            if (!cache.TryGetPath(crc, out string? cachedPath) || cachedPath != path)
            {
                throw new InvalidDataException($"source PathChecksumCache omitted {path:?}");
            }
            rows.WriteLine($"SCRIPT_PATH|{path}|{crc}");
            return;
        }
        string nestedPath = parentPath + field.Name + ".";
        foreach (SerializableType.Field child in field.Type.Fields)
        {
            DumpScriptPath(child, nestedPath, depth + 1, cache, rows);
        }
    }

    internal static int ExportManagedReferenceSchema(
        string inputRootArgument,
        string outputRootArgument,
        string rowsArgument,
        string assemblyArgument,
        string unityVersionText)
    {
        string inputRoot = Path.GetFullPath(inputRootArgument);
        string outputRoot = Path.GetFullPath(outputRootArgument);
        string rowsPath = Path.GetFullPath(rowsArgument);
        string assemblyPath = Path.GetFullPath(assemblyArgument);
        EnsureEmptyOrMissingDirectory(inputRoot, "input");
        EnsureEmptyOrMissingDirectory(outputRoot, "output");
        if (!File.Exists(assemblyPath))
        {
            throw new FileNotFoundException("managed reference schema fixture assembly is missing", assemblyPath);
        }
        if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
        {
            throw new IOException($"refusing to overwrite probe rows {rowsPath}");
        }

        Directory.CreateDirectory(inputRoot);
        Directory.CreateDirectory(outputRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);
        string serializedPath = Path.Combine(inputRoot, "managed_reference_schema.assets");
        string targetSerializedPath = Path.Combine(inputRoot, "target.assets");
        string inputAssemblyPath = Path.Combine(inputRoot, "ManagedReferenceFixtureScripts.dll");
        if (unityVersionText is not "2019.4.40f1" and not "2021.2.0f1")
        {
            throw new ArgumentException($"the managed-reference fixture supports only its pinned 2019.4.40f1 and 2021.2.0f1 cases, not {unityVersionText}");
        }
        bool stableReferenceIds = unityVersionText == "2021.2.0f1";
        byte[] serializedBytes = BuildManagedReferenceSchemaFile(unityVersionText);
        byte[] targetSerializedBytes = BuildManagedReferenceTargetFile(unityVersionText);
        byte[] assemblyBytes = File.ReadAllBytes(assemblyPath);
        File.WriteAllBytes(serializedPath, serializedBytes);
        File.WriteAllBytes(targetSerializedPath, targetSerializedBytes);
        File.WriteAllBytes(inputAssemblyPath, assemblyBytes);

        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(new FullConfiguration());
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        var matches = gameData.GameBundle.FetchAssets()
            .Where(asset => (asset.ClassID == 114 && asset.PathID == 42)
                || (asset.ClassID == 115 && asset.PathID == 7)
                || (asset.ClassID == 49 && asset.PathID == 9))
            .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
            .ToArray();
        if (matches.Length != 3)
        {
            throw new InvalidDataException($"expected MonoBehaviour, MonoScript, and TextAsset objects; found {matches.Length}: {string.Join(";", matches)}");
        }
        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

        string assetsRoot = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string? behaviourPath = Directory.GetFiles(assetsRoot, "*.asset", SearchOption.AllDirectories)
            .FirstOrDefault(path => File.ReadAllText(path).Contains("m_Name: ManagedRoot", StringComparison.Ordinal));
        if (behaviourPath is null)
        {
            throw new InvalidDataException("source project export did not write the ManagedRoot MonoBehaviour");
        }
        string? targetMetaPath = Directory.GetFiles(assetsRoot, "*.meta", SearchOption.AllDirectories)
            .FirstOrDefault(path => File.ReadAllText(path).Contains("TextScriptImporter:", StringComparison.Ordinal));
        if (targetMetaPath is null)
        {
            throw new InvalidDataException("source project export did not write a TextScriptImporter sidecar");
        }
        string targetPath = targetMetaPath[..^5];
        string behaviourText = File.ReadAllText(behaviourPath);
        string pluginAssemblyPath = Path.Combine(assetsRoot, "Plugins", "ManagedReferenceFixtureScripts.dll");
        string pluginMetaPath = pluginAssemblyPath + ".meta";
        if (!File.Exists(pluginAssemblyPath) || !File.Exists(pluginMetaPath))
        {
            throw new InvalidDataException("source project export did not write the managed assembly and PluginImporter sidecar");
        }
        string targetGuid = GuidFromMeta(targetMetaPath);
        string scriptLine = behaviourText.Split('\n')
            .Select(line => line.TrimEnd('\r'))
            .FirstOrDefault(line => line.TrimStart().StartsWith("m_Script:", StringComparison.Ordinal))
            ?? throw new InvalidDataException("MonoBehaviour YAML has no m_Script pointer");
        string scriptGuid = GuidFromYamlLine(scriptLine);
        string normalizedBehaviour = behaviourText
            .Replace($"guid: {targetGuid}", "guid: __TEXTASSET_GUID__", StringComparison.Ordinal)
            .Replace($"guid: {scriptGuid}", "guid: __SCRIPT_GUID__", StringComparison.Ordinal);
        string[] requiredBehaviourFragments = stableReferenceIds
            ?
            [
                "  root:\n    rid: 101",
            "  marker: 77",
            "  references:\n    version: 2",
            "    RefIds:\n    - rid: 101",
            "        value: 13",
            "        next:\n          rid: 202",
            "        target: {fileID: 4900000, guid: __TEXTASSET_GUID__, type: 3}",
            "        weight: 0.5",
            "    - rid: 202",
            "        value: 27",
            "        next:\n          rid: 101",
            "        label: back",
            ]
            :
            [
                "  root:\n    id: 0",
            "  marker: 77",
            "  references:\n    version: 1",
            "    RefIds:\n    - rid: 0",
            "        value: 13",
            "        next:\n          id: 1",
            "        target: {fileID: 4900000, guid: __TEXTASSET_GUID__, type: 3}",
            "        weight: 0.5",
            "    - rid: 1",
            "        value: 27",
            "        next:\n          id: 0",
            "        label: back",
            ];
        foreach (string fragment in requiredBehaviourFragments)
        {
            if (!normalizedBehaviour.Contains(fragment, StringComparison.Ordinal))
            {
                throw new InvalidDataException($"source MonoBehaviour YAML is missing required SerializeReference evidence: {fragment}");
            }
        }
        if (normalizedBehaviour.Split("\n    - rid:", StringSplitOptions.None).Length - 1 != 2)
        {
            throw new InvalidDataException("source MonoBehaviour YAML must contain exactly two managed registry entries");
        }
        byte[] normalizedBehaviourBytes = Encoding.UTF8.GetBytes(normalizedBehaviour);
        byte[] normalizedTargetMetaBytes = Encoding.UTF8.GetBytes(string.Join("\n",
            File.ReadAllLines(targetMetaPath)
                .Where(line => !line.StartsWith("guid: ", StringComparison.Ordinal)
                    && !line.StartsWith("timeCreated: ", StringComparison.Ordinal))) + "\n");
        byte[] pluginAssemblyBytes = File.ReadAllBytes(pluginAssemblyPath);
        byte[] normalizedPluginMetaBytes = Encoding.UTF8.GetBytes(string.Join("\n",
            File.ReadAllLines(pluginMetaPath)
                .Where(line => !line.StartsWith("guid: ", StringComparison.Ordinal)
                    && !line.StartsWith("timeCreated: ", StringComparison.Ordinal))) + "\n");
        byte[] targetBytes = File.ReadAllBytes(targetPath);
        if (!StringComparer.Ordinal.Equals(Encoding.UTF8.GetString(targetBytes), "managed-reference-target"))
        {
            throw new InvalidDataException("source TextAsset output does not retain the external PPtr target content");
        }

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine("# Frozen AssetRipper export for an assembly-resolved no-TypeTree SerializeReference registry.");
        rows.WriteLine("NORMALIZE|TextAsset target GUID becomes __TEXTASSET_GUID__; MonoScript GUID becomes __SCRIPT_GUID__.");
        rows.WriteLine("NORMALIZE|Importer guid and timeCreated fields omitted.");
        rows.WriteLine($"UNITY_VERSION|{unityVersionText}");
        rows.WriteLine($"REGISTRY_FORMAT|{(stableReferenceIds ? 2 : 1)}|{(stableReferenceIds ? "counted stable IDs" : "terminated index IDs")}");
        rows.WriteLine($"INPUT_HEX|managed_reference_schema.assets|{Convert.ToHexString(serializedBytes)}");
        rows.WriteLine($"INPUT_HEX|target.assets|{Convert.ToHexString(targetSerializedBytes)}");
        rows.WriteLine($"ASSEMBLY|ManagedReferenceFixtureScripts.dll|{assemblyBytes.Length}|{Convert.ToHexString(SHA256.HashData(assemblyBytes))}");
        rows.WriteLine($"ASSEMBLY_OUTPUT|ManagedReferenceFixtureScripts.dll|{pluginAssemblyBytes.Length}|{Convert.ToHexString(SHA256.HashData(pluginAssemblyBytes))}");
        foreach (string className in new[] { "GraphHost", "NodeA", "NodeB" })
        {
            ScriptIdentifier scriptId = new("ManagedReferenceFixtureScripts", "Game", className);
            if (!gameData.AssemblyManager.TryGetSerializableType(scriptId, UnityVersion.Parse(unityVersionText), out SerializableType? schema, out string? reason))
            {
                throw new InvalidDataException($"source FieldSerializer could not build {className}: {reason}");
            }
            DumpSourceSchema(schema!, className, rows);
        }
        foreach (string item in matches)
        {
            rows.WriteLine($"OBJECT|{item}");
        }
        string behaviourRelative = Path.GetRelativePath(outputRoot, behaviourPath).Replace('\\', '/');
        rows.WriteLine($"YAML_HEX|{behaviourRelative}|{normalizedBehaviourBytes.Length}|{Convert.ToHexString(SHA256.HashData(normalizedBehaviourBytes))}|{Convert.ToHexString(normalizedBehaviourBytes)}");
        string targetRelative = Path.GetRelativePath(outputRoot, targetPath).Replace('\\', '/');
        rows.WriteLine($"TARGET_HEX|{targetRelative}|{targetBytes.Length}|{Convert.ToHexString(SHA256.HashData(targetBytes))}|{Convert.ToHexString(targetBytes)}");
        string targetMetaRelative = Path.GetRelativePath(outputRoot, targetMetaPath).Replace('\\', '/');
        rows.WriteLine($"TARGET_META_HEX|{targetMetaRelative}|{normalizedTargetMetaBytes.Length}|{Convert.ToHexString(SHA256.HashData(normalizedTargetMetaBytes))}|{Convert.ToHexString(normalizedTargetMetaBytes)}");
        string pluginMetaRelative = Path.GetRelativePath(outputRoot, pluginMetaPath).Replace('\\', '/');
        rows.WriteLine($"PLUGIN_META_HEX|{pluginMetaRelative}|{normalizedPluginMetaBytes.Length}|{Convert.ToHexString(SHA256.HashData(normalizedPluginMetaBytes))}|{Convert.ToHexString(normalizedPluginMetaBytes)}");
        Console.WriteLine($"mono_behaviour={behaviourRelative}");
        Console.WriteLine($"target={targetRelative}");
        return 0;
    }

    internal static void DumpSourceSchema(SerializableType serializableType, string identity, StreamWriter rows)
    {
        TypeTreeNodeStruct root = TypeTreeNodeStruct.FromSerializableType(serializableType);
        Dump(root, 0);

        void Dump(TypeTreeNodeStruct node, int level)
        {
            rows.WriteLine($"SCHEMA_NODE|{identity}|{level}|{node.TypeName}|{node.Name}|{node.Version}|{(uint)node.MetaFlag}");
            foreach (TypeTreeNodeStruct child in node.SubNodes)
            {
                Dump(child, level + 1);
            }
        }
    }

    internal static string GuidFromMeta(string path)
    {
        string line = File.ReadAllLines(path)
            .FirstOrDefault(line => line.StartsWith("guid: ", StringComparison.Ordinal))
            ?? throw new InvalidDataException($"metadata file has no GUID: {path}");
        return line[6..];
    }

    internal static string GuidFromYamlLine(string line)
    {
        const string marker = "guid: ";
        int start = line.IndexOf(marker, StringComparison.Ordinal);
        if (start < 0)
        {
            throw new InvalidDataException($"YAML pointer has no GUID: {line}");
        }
        start += marker.Length;
        string value = new(line[start..].TakeWhile(Uri.IsHexDigit).ToArray());
        if (value.Length != 32)
        {
            throw new InvalidDataException($"YAML pointer GUID has {value.Length} hex digits: {line}");
        }
        return value;
    }

    internal static byte[] BuildManagedSchemaFile(string unityVersionText)
    {
        List<byte> behaviour = [];
        WriteI32(behaviour, 0); // m_GameObject.m_FileID
        WriteI64(behaviour, 0); // m_GameObject.m_PathID
        behaviour.Add(1); // m_Enabled
        AlignFour(behaviour);
        WriteI32(behaviour, 0); // m_Script.m_FileID
        WriteI64(behaviour, 7); // m_Script.m_PathID
        WriteString(behaviour, "RichRoot"); // m_Name

        WriteI64(behaviour, 0x0102030405060708); // inherited sequence
        WriteI32(behaviour, 45); // health
        WriteI32(behaviour, 3); // samples.Length
        WriteI32(behaviour, 4);
        WriteI32(behaviour, 5);
        WriteI32(behaviour, 6);
        WriteI32(behaviour, 2); // weights.Count
        WriteI32(behaviour, BitConverter.SingleToInt32Bits(0.25f));
        WriteI32(behaviour, BitConverter.SingleToInt32Bits(0.75f));
        WriteI32(behaviour, 99); // stats.score
        WriteString(behaviour, "nest"); // stats.label
        WriteI32(behaviour, 0); // target.m_FileID
        WriteI64(behaviour, 9); // target.m_PathID -> local TextAsset
        behaviour.Add(1); // active
        AlignFour(behaviour);
        behaviour.Add(1); // phase.Active
        AlignFour(behaviour);

        List<byte> script = [];
        WriteString(script, "RichController"); // m_Name
        WriteI32(script, 0); // m_ExecutionOrder
        script.AddRange(new byte[16]); // m_PropertiesHash
        WriteString(script, "RichController"); // m_ClassName
        WriteString(script, "Game"); // m_Namespace
        WriteString(script, "ComplexFixtureScripts"); // m_AssemblyName

        List<byte> textAsset = [];
        WriteString(textAsset, "Target"); // m_Name
        WriteString(textAsset, "project-export-target"); // m_Script

        SerializedType behaviourType = new() { RawTypeID = 114 };
        SerializedType scriptType = new() { RawTypeID = 115 };
        SerializedType textAssetType = new() { RawTypeID = 49 };
        SerializedFile file = new SerializedFileBuilder
        {
            Generation = FormatVersion.StoresTypeDependencies,
            Version = UnityVersion.Parse(unityVersionText),
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = false,
            Types = { behaviourType, scriptType, textAssetType },
            Objects =
        {
            new ObjectInfo(behaviourType)
            {
                FileID = 42,
                TypeID = 114,
                SerializedTypeIndex = 0,
                ObjectData = behaviour.ToArray(),
            },
            new ObjectInfo(scriptType)
            {
                FileID = 7,
                TypeID = 115,
                SerializedTypeIndex = 1,
                ObjectData = script.ToArray(),
            },
            new ObjectInfo(textAssetType)
            {
                FileID = 9,
                TypeID = 49,
                SerializedTypeIndex = 2,
                ObjectData = textAsset.ToArray(),
            },
        },
        }.Build();
        file.FilePath = "managed_schema.assets";
        return file.ToByteArray();
    }

    internal static byte[] BuildManagedReferenceSchemaFile(string unityVersionText)
    {
        bool stableReferenceIds = unityVersionText == "2021.2.0f1";
        List<byte> behaviour = [];
        WriteI32(behaviour, 0); // m_GameObject.m_FileID
        WriteI64(behaviour, 0); // m_GameObject.m_PathID
        behaviour.Add(1); // m_Enabled
        AlignFour(behaviour);
        WriteI32(behaviour, 0); // m_Script.m_FileID
        WriteI64(behaviour, 7); // m_Script.m_PathID
        WriteString(behaviour, "ManagedRoot"); // m_Name
        if (stableReferenceIds)
        {
            WriteI64(behaviour, 101); // root.rid -> NodeA
        }
        else
        {
            WriteI32(behaviour, 0); // root.rid -> first legacy registry entry
        }
        WriteI32(behaviour, 77); // marker

        WriteI32(behaviour, stableReferenceIds ? 2 : 1); // counted or terminated registry format
        if (stableReferenceIds)
        {
            WriteI32(behaviour, 2); // references.Count
            WriteI64(behaviour, 101); // NodeA rid
        }
        WriteManagedTypeTriple(behaviour, "NodeA", "Game", "ManagedReferenceFixtureScripts");
        WriteI32(behaviour, 13); // NodeA.value
        if (stableReferenceIds)
        {
            WriteI64(behaviour, 202); // NodeA.next.rid -> NodeB
        }
        else
        {
            WriteI32(behaviour, 1); // NodeA.next.rid -> NodeB
        }
        WriteI32(behaviour, 1); // NodeA.target.m_FileID -> target.assets
        WriteI64(behaviour, 9); // NodeA.target.m_PathID -> TextAsset
        WriteI32(behaviour, BitConverter.SingleToInt32Bits(0.5f)); // NodeA.weight

        if (stableReferenceIds)
        {
            WriteI64(behaviour, 202); // NodeB rid
        }
        WriteManagedTypeTriple(behaviour, "NodeB", "Game", "ManagedReferenceFixtureScripts");
        WriteI32(behaviour, 27); // NodeB.value
        if (stableReferenceIds)
        {
            WriteI64(behaviour, 101); // NodeB.next.rid -> NodeA, forming a cycle
        }
        else
        {
            WriteI32(behaviour, 0); // NodeB.next.rid -> NodeA, forming a cycle
        }
        WriteI32(behaviour, 0); // NodeB.target.m_FileID
        WriteI64(behaviour, 0); // NodeB.target.m_PathID
        WriteString(behaviour, "back"); // NodeB.label
        if (!stableReferenceIds)
        {
            WriteManagedTypeTriple(behaviour, "Terminus", "UnityEngine.DMAT", "FAKE_ASM");
        }
        AlignFour(behaviour); // registry array alignment

        List<byte> script = [];
        WriteString(script, "GraphHost"); // m_Name
        WriteI32(script, 0); // m_ExecutionOrder
        script.AddRange(new byte[16]); // m_PropertiesHash
        WriteString(script, "GraphHost"); // m_ClassName
        WriteString(script, "Game"); // m_Namespace
        WriteString(script, "ManagedReferenceFixtureScripts"); // m_AssemblyName

        SerializedType behaviourType = new() { RawTypeID = 114 };
        SerializedType scriptType = new() { RawTypeID = 115 };
        SerializedFile file = new SerializedFileBuilder
        {
            Generation = FormatVersion.StoresTypeDependencies,
            Version = UnityVersion.Parse(unityVersionText),
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = false,
            Types = { behaviourType, scriptType },
            Objects =
        {
            new ObjectInfo(behaviourType)
            {
                FileID = 42,
                TypeID = 114,
                SerializedTypeIndex = 0,
                ObjectData = behaviour.ToArray(),
            },
            new ObjectInfo(scriptType)
            {
                FileID = 7,
                TypeID = 115,
                SerializedTypeIndex = 1,
                ObjectData = script.ToArray(),
            },
        },
            Dependencies =
        {
            new FileIdentifier
            {
                AssetPath = "target.assets",
                PathNameOrigin = "target.assets",
                PathName = "target.assets",
                Type = AssetType.Serialized,
            },
        },
        }.Build();
        file.FilePath = "managed_reference_schema.assets";
        return file.ToByteArray();
    }

    internal static byte[] BuildManagedReferenceTargetFile(string unityVersionText)
    {
        List<byte> textAsset = [];
        WriteString(textAsset, "Target");
        WriteString(textAsset, "managed-reference-target");
        SerializedType textAssetType = new() { RawTypeID = 49 };
        SerializedFile file = new SerializedFileBuilder
        {
            Generation = FormatVersion.StoresTypeDependencies,
            Version = UnityVersion.Parse(unityVersionText),
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = false,
            Types = { textAssetType },
            Objects =
        {
            new ObjectInfo(textAssetType)
            {
                FileID = 9,
                TypeID = 49,
                ObjectData = textAsset.ToArray(),
            },
        },
        }.Build();
        file.FilePath = "target.assets";
        return file.ToByteArray();
    }

    internal static void WriteManagedTypeTriple(List<byte> payload, string className, string namespaceName, string assemblyName)
    {
        WriteString(payload, className);
        WriteString(payload, namespaceName);
        WriteString(payload, assemblyName);
    }

    internal static void AlignFour(List<byte> bytes)
    {
        while ((bytes.Count & 3) != 0)
        {
            bytes.Add(0);
        }
    }
}
