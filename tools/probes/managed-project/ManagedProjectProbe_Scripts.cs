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
    internal static int ExportEmptyScript(string inputRootArgument, string outputRootArgument, string rowsArgument)
    {
        string inputRoot = Path.GetFullPath(inputRootArgument);
        string outputRoot = Path.GetFullPath(outputRootArgument);
        string rowsPath = Path.GetFullPath(rowsArgument);
        EnsureEmptyOrMissingDirectory(inputRoot, "input");
        EnsureEmptyOrMissingDirectory(outputRoot, "output");
        if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
        {
            throw new IOException($"refusing to overwrite probe rows {rowsPath}");
        }

        Directory.CreateDirectory(inputRoot);
        Directory.CreateDirectory(outputRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);
        string inputPath = Path.Combine(inputRoot, "generic_script.assets");
        byte[] inputBytes = BuildGenericScriptFile();
        File.WriteAllBytes(inputPath, inputBytes);

        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(new FullConfiguration());
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        var monoScripts = gameData.GameBundle.FetchAssets()
        .Where(asset => asset.ClassID == 115 && asset.PathID == 7)
        .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
        .ToArray();
        if (monoScripts.Length != 1 || !monoScripts[0].StartsWith("MonoScript|7|", StringComparison.Ordinal))
        {
            throw new InvalidDataException($"expected one source MonoScript at PathID 7: {string.Join(";", monoScripts)}");
        }
        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

        string assetsRoot = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string[] relativePaths =
        [
        "Scripts/GameScripts/Game/Widget`1.cs",
"Scripts/GameScripts/Game/Widget`1.cs.meta",
"Scripts/GameScripts/GameScripts.asmdef",
];
        string[] absolutePaths = relativePaths.Select(path => Path.Combine(assetsRoot, path)).ToArray();
        foreach (string path in absolutePaths)
        {
            if (!File.Exists(path))
            {
                throw new FileNotFoundException($"expected source script output {path}");
            }
        }

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine("# Frozen AssetRipper generic MonoScript output with no managed assembly input.");
        rows.WriteLine($"OBJECT|{monoScripts[0]}");
        WriteInputRow(rows, inputPath, inputBytes);
        for (int index = 0; index < relativePaths.Length; index++)
        {
            byte[] bytes = File.ReadAllBytes(absolutePaths[index]);
            if (relativePaths[index].EndsWith(".meta", StringComparison.Ordinal))
            {
                bytes = NormalizeMonoImporterMeta(bytes);
            }
            rows.WriteLine($"FILE|Assets/{relativePaths[index]}|{bytes.Length}|{Convert.ToHexString(SHA256.HashData(bytes))}");
            rows.WriteLine($"CONTENT_HEX|Assets/{relativePaths[index]}|{Convert.ToHexString(bytes)}");
        }
        Console.WriteLine($"source_object={monoScripts[0]}");
        Console.WriteLine($"script=Assets/{relativePaths[0]}");
        Console.WriteLine($"sidecar=Assets/{relativePaths[1]}");
        Console.WriteLine($"assembly_definition=Assets/{relativePaths[2]}");
        return 0;
    }

    internal static byte[] NormalizeMonoImporterMeta(byte[] bytes)
    {
        string text = Encoding.UTF8.GetString(bytes);
        string[] lines = text.Split(["\r\n", "\n"], StringSplitOptions.None);
        string normalized = string.Join(
            "\n",
            lines.Where(line => !line.StartsWith("timeCreated: ", StringComparison.Ordinal)));
        return Encoding.UTF8.GetBytes(normalized);
    }

    internal static byte[] BuildGenericScriptFile()
    {
        List<byte> payload = [];
        WriteString(payload, "Widget"); // m_Name
        WriteI32(payload, 17); // m_ExecutionOrder
        payload.AddRange(new byte[16]); // Hash128 m_PropertiesHash
        WriteString(payload, "Widget`1"); // m_ClassName
        WriteString(payload, "Game"); // m_Namespace
        WriteString(payload, "GameScripts"); // m_AssemblyName

        SerializedType monoScriptType = new() { RawTypeID = 115 };
        SerializedFile file = new SerializedFileBuilder
        {
            Generation = FormatVersion.StoresTypeDependencies,
            Version = UnityVersion.Parse("2019.4.40f1"),
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = false,
            Types = { monoScriptType },
            Objects =
        {
            new ObjectInfo(monoScriptType)
            {
                FileID = 7,
                TypeID = 115,
                ObjectData = payload.ToArray(),
            },
        },
        }.Build();
        file.FilePath = "generic_script.assets";
        return file.ToByteArray();
    }

    internal static int ExportReferenceScript(string inputRootArgument, string outputRootArgument, string rowsArgument)
    {
        string inputRoot = Path.GetFullPath(inputRootArgument);
        string outputRoot = Path.GetFullPath(outputRootArgument);
        string rowsPath = Path.GetFullPath(rowsArgument);
        EnsureEmptyOrMissingDirectory(inputRoot, "input");
        EnsureEmptyOrMissingDirectory(outputRoot, "output");
        if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
        {
            throw new IOException($"refusing to overwrite probe rows {rowsPath}");
        }

        Directory.CreateDirectory(inputRoot);
        Directory.CreateDirectory(outputRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);
        string scriptInputPath = Path.Combine(inputRoot, "reference_script.assets");
        string behaviourInputPath = Path.Combine(inputRoot, "reference_behaviour.assets");
        byte[] scriptInputBytes = BuildReferenceScriptFile();
        byte[] behaviourInputBytes = BuildReferenceBehaviourFile();
        File.WriteAllBytes(scriptInputPath, scriptInputBytes);
        File.WriteAllBytes(behaviourInputPath, behaviourInputBytes);

        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(new FullConfiguration());
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        var scripts = gameData.GameBundle.FetchAssets()
            .Where(asset => asset.ClassID == 115 && asset.PathID == 7)
            .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
            .ToArray();
        if (scripts.Length != 1 || !scripts[0].StartsWith("MonoScript|7|", StringComparison.Ordinal))
        {
            throw new InvalidDataException($"expected one source reference MonoScript at PathID 7: {string.Join(";", scripts)}");
        }
        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

        string assetsRoot = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string scriptRelative = "Scripts/UnityEngine/UnityEngine/Object.cs";
        string sidecarRelative = scriptRelative + ".meta";
        string definitionRelative = "Scripts/UnityEngine/UnityEngine.asmdef";
        string scriptPath = Path.Combine(assetsRoot, scriptRelative);
        string sidecarPath = Path.Combine(assetsRoot, sidecarRelative);
        string definitionPath = Path.Combine(assetsRoot, definitionRelative);
        foreach (string path in new[] { scriptPath, sidecarPath, definitionPath })
        {
            if (!File.Exists(path))
            {
                throw new FileNotFoundException($"expected source reference-script output {path}");
            }
        }
        string? behaviourPath = Directory.Exists(assetsRoot)
            ? Directory.GetFiles(assetsRoot, "*.asset", SearchOption.AllDirectories)
                .FirstOrDefault(path => File.ReadAllText(path).Contains("m_Name: ReferenceRoot", StringComparison.Ordinal))
            : null;
        if (behaviourPath is null)
        {
            throw new InvalidDataException("source export did not write the expected referencing MonoBehaviour asset");
        }

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine("# Frozen AssetRipper no-manager reference MonoScript pointer and project-export probe.");
        rows.WriteLine($"OBJECT|{scripts[0]}");
        WriteInputRow(rows, scriptInputPath, scriptInputBytes);
        WriteInputRow(rows, behaviourInputPath, behaviourInputBytes);
        foreach ((string relative, string path) in new[]
        {
        ($"Assets/{scriptRelative}", scriptPath),
        ($"Assets/{sidecarRelative}", sidecarPath),
        ($"Assets/{definitionRelative}", definitionPath),
        ($"Assets/{Path.GetRelativePath(assetsRoot, behaviourPath).Replace('\\', '/')}", behaviourPath),
    })
        {
            byte[] bytes = File.ReadAllBytes(path);
            if (relative.EndsWith(".meta", StringComparison.Ordinal))
            {
                bytes = NormalizeMonoImporterMeta(bytes);
            }
            rows.WriteLine($"FILE|{relative}|{bytes.Length}|{Convert.ToHexString(SHA256.HashData(bytes))}");
            rows.WriteLine($"CONTENT_HEX|{relative}|{Convert.ToHexString(bytes)}");
        }
        Console.WriteLine($"source_object={scripts[0]}");
        Console.WriteLine($"script=Assets/{scriptRelative}");
        Console.WriteLine($"sidecar=Assets/{sidecarRelative}");
        Console.WriteLine($"assembly_definition=Assets/{definitionRelative}");
        Console.WriteLine($"behaviour=Assets/{Path.GetRelativePath(assetsRoot, behaviourPath).Replace('\\', '/')}");
        return 0;
    }

    internal static int ExportManagerReferenceScript(
        string inputRootArgument,
        string outputRootArgument,
        string rowsArgument,
        string mscorlibV2Argument,
        string assemblyName,
        string unityVersion,
        ScriptExportMode? scriptExportMode)
    {
        string inputRoot = Path.GetFullPath(inputRootArgument);
        string outputRoot = Path.GetFullPath(outputRootArgument);
        string rowsPath = Path.GetFullPath(rowsArgument);
        string mscorlibV2Path = Path.GetFullPath(mscorlibV2Argument);
        EnsureEmptyOrMissingDirectory(inputRoot, "input");
        EnsureEmptyOrMissingDirectory(outputRoot, "output");
        if (!File.Exists(mscorlibV2Path))
        {
            throw new FileNotFoundException("mscorlib v2 fixture is missing", mscorlibV2Path);
        }
        if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
        {
            throw new IOException($"refusing to overwrite probe rows {rowsPath}");
        }

        Directory.CreateDirectory(inputRoot);
        Directory.CreateDirectory(outputRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);
        string scriptInputPath = Path.Combine(inputRoot, "reference_script.assets");
        string behaviourInputPath = Path.Combine(inputRoot, "reference_behaviour.assets");
        string mscorlibInputPath = Path.Combine(inputRoot, "mscorlib.dll");
        byte[] scriptInputBytes = BuildReferenceScriptFile(assemblyName, unityVersion);
        byte[] behaviourInputBytes = BuildReferenceBehaviourFile(unityVersion);
        byte[] mscorlibInputBytes = File.ReadAllBytes(mscorlibV2Path);
        File.WriteAllBytes(scriptInputPath, scriptInputBytes);
        File.WriteAllBytes(behaviourInputPath, behaviourInputBytes);
        File.WriteAllBytes(mscorlibInputPath, mscorlibInputBytes);

        Logger.Add(new ConsoleLogger());
        FullConfiguration configuration = new();
        if (scriptExportMode is not null)
        {
            configuration.ExportSettings.ScriptExportMode = scriptExportMode.Value;
        }
        ExportHandler handler = new(configuration);
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        if (!gameData.AssemblyManager.IsSet || !gameData.AssemblyManager.HasMscorlib2)
        {
            throw new InvalidDataException(
                $"expected initialized mscorlib2 manager, got backend={gameData.AssemblyManager.ScriptingBackend}, " +
                $"isSet={gameData.AssemblyManager.IsSet}, hasMscorlib2={gameData.AssemblyManager.HasMscorlib2}");
        }
        var scripts = gameData.GameBundle.FetchAssets()
            .Where(asset => asset.ClassID == 115 && asset.PathID == 7)
            .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
            .ToArray();
        if (scripts.Length != 1 || !scripts[0].StartsWith("MonoScript|7|", StringComparison.Ordinal))
        {
            throw new InvalidDataException($"expected one source {assemblyName} MonoScript at PathID 7: {string.Join(';', scripts)}");
        }
        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

        string assetsRoot = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string? behaviourPath = Directory.Exists(assetsRoot)
            ? Directory.GetFiles(assetsRoot, "*.asset", SearchOption.AllDirectories)
                .FirstOrDefault(path => File.ReadAllText(path).Contains("m_Name: ReferenceRoot", StringComparison.Ordinal))
            : null;
        if (behaviourPath is null)
        {
            throw new InvalidDataException("source manager-backed export did not write the referencing MonoBehaviour asset");
        }

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine($"MANAGER|{gameData.AssemblyManager.ScriptingBackend}|{gameData.AssemblyManager.IsSet}|{gameData.AssemblyManager.HasMscorlib2}");
        if (scriptExportMode is not null)
        {
            rows.WriteLine($"MODE|{configuration.ExportSettings.ScriptExportMode}");
        }
        rows.WriteLine($"OBJECT|{scripts[0]}");
        WriteInputRow(rows, scriptInputPath, scriptInputBytes);
        WriteInputRow(rows, behaviourInputPath, behaviourInputBytes);
        WriteInputRow(rows, mscorlibInputPath, mscorlibInputBytes);
        string behaviourRelative = $"Assets/{Path.GetRelativePath(assetsRoot, behaviourPath).Replace('\\', '/')}";
        byte[] behaviourBytes = File.ReadAllBytes(behaviourPath);
        rows.WriteLine($"FILE|{behaviourRelative}|{behaviourBytes.Length}|{Convert.ToHexString(SHA256.HashData(behaviourBytes))}");
        rows.WriteLine($"CONTENT_HEX|{behaviourRelative}|{Convert.ToHexString(behaviourBytes)}");
        Console.WriteLine($"manager={gameData.AssemblyManager.ScriptingBackend}|{gameData.AssemblyManager.IsSet}|{gameData.AssemblyManager.HasMscorlib2}");
        Console.WriteLine($"source_object={scripts[0]}");
        Console.WriteLine($"behaviour={behaviourRelative}");
        return 0;
    }

    internal static int ExportTextAsset(string inputRootArgument, string outputRootArgument, string rowsArgument, string modeArgument)
    {
        string inputRoot = Path.GetFullPath(inputRootArgument);
        string outputRoot = Path.GetFullPath(outputRootArgument);
        string rowsPath = Path.GetFullPath(rowsArgument);
        EnsureEmptyOrMissingDirectory(outputRoot, "output");
        if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
        {
            throw new IOException($"refusing to overwrite probe rows {rowsPath}");
        }
        if (!Directory.Exists(inputRoot))
        {
            throw new DirectoryNotFoundException($"input root does not exist: {inputRoot}");
        }

        TextExportMode mode = Enum.Parse<TextExportMode>(modeArgument, ignoreCase: false);
        Directory.CreateDirectory(outputRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);
        FullConfiguration configuration = new();
        configuration.ExportSettings.TextExportMode = mode;
        ExportHandler handler = new(configuration);
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        var textAssets = gameData.GameBundle.FetchAssets()
            .Where(asset => asset.ClassID == 49)
            .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
            .Order(StringComparer.Ordinal)
            .ToArray();
        if (textAssets.Length != 1)
        {
            throw new InvalidDataException($"expected one TextAsset, found {textAssets.Length}: {string.Join(';', textAssets)}");
        }
        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

        string projectAssets = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string[] files = Directory.GetFiles(projectAssets, "*", SearchOption.AllDirectories)
            .OrderBy(path => Path.GetRelativePath(outputRoot, path), StringComparer.Ordinal)
            .ToArray();
        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine($"MODE|{mode}");
        rows.WriteLine($"OBJECT|{textAssets[0]}");
        foreach (string path in files)
        {
            string relative = Path.GetRelativePath(outputRoot, path).Replace('\\', '/');
            byte[] bytes = File.ReadAllBytes(path);
            if (relative.EndsWith(".meta", StringComparison.Ordinal))
            {
                string normalized = string.Join('\n', File.ReadAllLines(path)
                    .Where(line => !line.StartsWith("guid:", StringComparison.Ordinal)
                        && !line.StartsWith("timeCreated:", StringComparison.Ordinal))) + "\n";
                rows.WriteLine($"META|{relative}|{normalized.Replace("\n", "\\n", StringComparison.Ordinal)}");
            }
            else
            {
                rows.WriteLine($"ASSET|{relative}|{Convert.ToBase64String(bytes)}");
            }
        }
        Console.WriteLine($"mode={mode}");
        Console.WriteLine($"text_asset={textAssets[0]}");
        Console.WriteLine($"project_assets={files.Length}");
        return 0;
    }

    internal static int ExportSavedScript(string inputRootArgument, string outputRootArgument, string rowsArgument, string assemblyArgument)
    {
        string inputRoot = Path.GetFullPath(inputRootArgument);
        string outputRoot = Path.GetFullPath(outputRootArgument);
        string rowsPath = Path.GetFullPath(rowsArgument);
        string assemblyPath = Path.GetFullPath(assemblyArgument);
        EnsureEmptyOrMissingDirectory(inputRoot, "input");
        EnsureEmptyOrMissingDirectory(outputRoot, "output");
        if (!File.Exists(assemblyPath))
        {
            throw new FileNotFoundException("managed assembly fixture is missing", assemblyPath);
        }
        if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
        {
            throw new IOException($"refusing to overwrite probe rows {rowsPath}");
        }

        Directory.CreateDirectory(inputRoot);
        Directory.CreateDirectory(outputRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);
        string scriptPath = Path.Combine(inputRoot, "saved_script.assets");
        string behaviourPath = Path.Combine(inputRoot, "saved_behaviour.assets");
        string dllPath = Path.Combine(inputRoot, "FixtureScripts.dll");
        byte[] scriptBytes = BuildSavedScriptFile();
        byte[] behaviourBytes = BuildSavedBehaviourFile();
        byte[] assemblyBytes = File.ReadAllBytes(assemblyPath);
        File.WriteAllBytes(scriptPath, scriptBytes);
        File.WriteAllBytes(behaviourPath, behaviourBytes);
        File.WriteAllBytes(dllPath, assemblyBytes);

        Logger.Add(new ConsoleLogger());
        FullConfiguration configuration = new();
        configuration.ExportSettings.ScriptExportMode = ScriptExportMode.DllExportWithoutRenaming;
        ExportHandler handler = new(configuration);
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        string[] scripts = gameData.GameBundle.FetchAssets()
            .Where(asset => asset.ClassID == 115 && asset.PathID == 7)
            .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
            .ToArray();
        if (scripts.Length != 1 || !scripts[0].StartsWith("MonoScript|7|", StringComparison.Ordinal))
        {
            throw new InvalidDataException($"expected one saved MonoScript at PathID 7: {string.Join(';', scripts)}");
        }
        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

        string assetsRoot = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string savedDllPath = Path.Combine(assetsRoot, "Plugins", "FixtureScripts.dll");
        string savedMetaPath = savedDllPath + ".meta";
        if (!File.Exists(savedDllPath) || !File.Exists(savedMetaPath))
        {
            string[] actual = Directory.Exists(assetsRoot)
                ? Directory.GetFiles(assetsRoot, "*", SearchOption.AllDirectories)
                    .Select(path => Path.GetRelativePath(assetsRoot, path).Replace('\\', '/'))
                    .Order(StringComparer.Ordinal)
                    .ToArray()
                : [];
            throw new FileNotFoundException($"expected saved assembly and PluginImporter sidecar; found {string.Join(';', actual)}");
        }
        string? behaviourOutputPath = Directory.Exists(assetsRoot)
            ? Directory.GetFiles(assetsRoot, "*.asset", SearchOption.AllDirectories)
                .FirstOrDefault(path => File.ReadAllText(path).Contains("m_Name: SavedScriptRoot", StringComparison.Ordinal))
            : null;
        if (behaviourOutputPath is null)
        {
            throw new InvalidDataException("source export did not write the assembly-backed MonoBehaviour asset");
        }

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine("# Frozen AssetRipper manager-backed ScriptExportCollection DLL save output.");
        rows.WriteLine("MODE|DllExportWithoutRenaming");
        rows.WriteLine($"OBJECT|{scripts[0]}");
        WriteInputRow(rows, scriptPath, scriptBytes);
        WriteInputRow(rows, behaviourPath, behaviourBytes);
        rows.WriteLine($"INPUT|FixtureScripts.dll|{assemblyBytes.Length}|{Convert.ToHexString(SHA256.HashData(assemblyBytes))}");
        string[] outputs =
        [
            savedDllPath,
        savedMetaPath,
        behaviourOutputPath,
    ];
        foreach (string path in outputs)
        {
            string relative = $"Assets/{Path.GetRelativePath(assetsRoot, path).Replace('\\', '/')}";
            byte[] bytes = File.ReadAllBytes(path);
            if (relative.EndsWith(".meta", StringComparison.Ordinal))
            {
                bytes = NormalizeMonoImporterMeta(bytes);
            }
            rows.WriteLine($"FILE|{relative}|{bytes.Length}|{Convert.ToHexString(SHA256.HashData(bytes))}");
            if (!relative.EndsWith(".dll", StringComparison.OrdinalIgnoreCase))
            {
                rows.WriteLine($"CONTENT_HEX|{relative}|{Convert.ToHexString(bytes)}");
            }
        }
        foreach (string path in Directory.GetFiles(Path.Combine(assetsRoot, "Plugins"), "*.dll")
            .Where(path => !string.Equals(Path.GetFileName(path), "FixtureScripts.dll", StringComparison.Ordinal))
            .OrderBy(path => path, StringComparer.Ordinal))
        {
            byte[] bytes = File.ReadAllBytes(path);
            rows.WriteLine($"SOURCE_EXTRA_PLUGIN|Assets/Plugins/{Path.GetFileName(path)}|{bytes.Length}|{Convert.ToHexString(SHA256.HashData(bytes))}");
        }
        Console.WriteLine($"source_object={scripts[0]}");
        Console.WriteLine("mode=DllExportWithoutRenaming");
        Console.WriteLine("assembly=Assets/Plugins/FixtureScripts.dll");
        Console.WriteLine($"behaviour=Assets/{Path.GetRelativePath(assetsRoot, behaviourOutputPath).Replace('\\', '/')}");
        return 0;
    }

    internal static byte[] BuildSavedScriptFile()
    {
        List<byte> payload = [];
        WriteString(payload, "HeroController"); // m_Name
        WriteI32(payload, 0); // m_ExecutionOrder
        payload.AddRange(new byte[16]); // Hash128 m_PropertiesHash
        WriteString(payload, "HeroController"); // m_ClassName
        WriteString(payload, "Game"); // m_Namespace
        WriteString(payload, "FixtureScripts"); // m_AssemblyName

        SerializedType monoScriptType = new() { RawTypeID = 115 };
        SerializedFile file = new SerializedFileBuilder
        {
            Generation = FormatVersion.StoresTypeDependencies,
            Version = UnityVersion.Parse("2019.4.40f1"),
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = false,
            Types = { monoScriptType },
            Objects =
        {
            new ObjectInfo(monoScriptType)
            {
                FileID = 7,
                TypeID = 115,
                ObjectData = payload.ToArray(),
            },
        },
        }.Build();
        file.FilePath = "saved_script.assets";
        return file.ToByteArray();
    }

    internal static byte[] BuildSavedBehaviourFile()
    {
        List<byte> payload = [];
        WriteI32(payload, 0); // m_GameObject.m_FileID
        WriteI64(payload, 0); // m_GameObject.m_PathID
        payload.Add(1); // m_Enabled
        while (payload.Count % 4 != 0)
        {
            payload.Add(0);
        }
        WriteI32(payload, 1); // m_Script.m_FileID -> saved_script.assets
        WriteI64(payload, 7); // m_Script.m_PathID -> saved MonoScript
        WriteString(payload, "SavedScriptRoot"); // m_Name
        WriteI32(payload, 789); // m_Health

        SerializedType monoBehaviourType = new() { RawTypeID = 114 };
        AssignTree(monoBehaviourType, MonoBehaviourTargetTree());
        SerializedFile file = new SerializedFileBuilder
        {
            Generation = FormatVersion.StoresTypeDependencies,
            Version = UnityVersion.Parse("2019.4.40f1"),
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = true,
            Types = { monoBehaviourType },
            Dependencies =
        {
            new FileIdentifier
            {
                AssetPath = "saved_script.assets",
                PathNameOrigin = "saved_script.assets",
                PathName = "saved_script.assets",
                Type = AssetType.Serialized,
            },
        },
            Objects =
        {
            new ObjectInfo(monoBehaviourType)
            {
                FileID = 8,
                TypeID = 114,
                ObjectData = payload.ToArray(),
            },
        },
        }.Build();
        file.FilePath = "saved_behaviour.assets";
        return file.ToByteArray();
    }

    internal static byte[] BuildReferenceScriptFile(string assemblyName = "UnityEngine", string versionText = "2019.4.40f1")
    {
        List<byte> scriptPayload = [];
        WriteString(scriptPayload, "Object"); // m_Name
        WriteI32(scriptPayload, 0); // m_ExecutionOrder
        scriptPayload.AddRange(new byte[16]); // Hash128 m_PropertiesHash
        WriteString(scriptPayload, "Object"); // m_ClassName
        WriteString(scriptPayload, "UnityEngine"); // m_Namespace
        WriteString(scriptPayload, assemblyName); // m_AssemblyName

        SerializedType monoScriptType = new() { RawTypeID = 115 };
        SerializedFile file = new SerializedFileBuilder
        {
            Generation = FormatVersion.StoresTypeDependencies,
            Version = UnityVersion.Parse(versionText),
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = false,
            Types = { monoScriptType },
            Objects =
        {
            new ObjectInfo(monoScriptType)
            {
                FileID = 7,
                TypeID = 115,
                ObjectData = scriptPayload.ToArray(),
            },
        },
        }.Build();
        file.FilePath = "reference_script.assets";
        return file.ToByteArray();
    }

    internal static byte[] BuildReferenceBehaviourFile(string versionText = "2019.4.40f1")
    {
        List<byte> behaviourPayload = [];
        WriteI32(behaviourPayload, 0); // m_GameObject.m_FileID
        WriteI64(behaviourPayload, 0); // m_GameObject.m_PathID
        behaviourPayload.Add(1); // m_Enabled
        while (behaviourPayload.Count % 4 != 0)
        {
            behaviourPayload.Add(0);
        }
        WriteI32(behaviourPayload, 1); // m_Script.m_FileID -> reference_script.assets
        WriteI64(behaviourPayload, 7); // m_Script.m_PathID -> referenced MonoScript
        WriteString(behaviourPayload, "ReferenceRoot"); // m_Name
        WriteI32(behaviourPayload, 789); // m_Health

        SerializedType monoBehaviourType = new() { RawTypeID = 114 };
        AssignTree(monoBehaviourType, MonoBehaviourTargetTree());
        SerializedFile file = new SerializedFileBuilder
        {
            Generation = FormatVersion.StoresTypeDependencies,
            Version = UnityVersion.Parse(versionText),
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = true,
            Types = { monoBehaviourType },
            Dependencies =
        {
            new FileIdentifier
            {
                AssetPath = "reference_script.assets",
                PathNameOrigin = "reference_script.assets",
                PathName = "reference_script.assets",
                Type = AssetType.Serialized,
            },
        },
            Objects =
        {
            new ObjectInfo(monoBehaviourType)
            {
                FileID = 8,
                TypeID = 114,
                ObjectData = behaviourPayload.ToArray(),
            },
        },
        }.Build();
        file.FilePath = "reference_behaviour.assets";
        return file.ToByteArray();
    }

    internal static int DumpReferenceAssemblyMap(string outputPathArgument, string? mscorlibV2Path)
    {
        string outputPath = Path.GetFullPath(outputPathArgument);
        if (File.Exists(outputPath) || Directory.Exists(outputPath))
        {
            throw new IOException($"refusing to overwrite reference assembly map {outputPath}");
        }
        Directory.CreateDirectory(Path.GetDirectoryName(outputPath)!);

        FieldInfo dataField = typeof(ReferenceAssemblies).GetField(
            "assemblyDataFile",
            BindingFlags.NonPublic | BindingFlags.Static)
            ?? throw new MissingFieldException(typeof(ReferenceAssemblies).FullName, "assemblyDataFile");
        object dataFile = dataField.GetValue(null)
            ?? throw new InvalidDataException("AssetRipper reference assembly data did not initialize");
        PropertyInfo assembliesProperty = dataFile.GetType().GetProperty("Assemblies")
            ?? throw new MissingMemberException(dataFile.GetType().FullName, "Assemblies");
        IEnumerable assemblies = (IEnumerable)(assembliesProperty.GetValue(dataFile)
            ?? throw new InvalidDataException("AssetRipper reference assembly versions were empty"));
        var manager = new BaseManager(_ => { });
        if (mscorlibV2Path is not null)
        {
            using FileStream mscorlibStream = File.OpenRead(mscorlibV2Path);
            AsmResolver.DotNet.AssemblyDefinition mscorlib =
                AsmResolver.DotNet.AssemblyDefinition.FromStream(mscorlibStream, createRuntimeContext: false);
            if (mscorlib.Name?.ToString() != "mscorlib" || mscorlib.Version.Major != 2)
            {
                throw new InvalidDataException(
                    $"expected an assembly named mscorlib with major version 2; got {mscorlib.Name} {mscorlib.Version}");
            }
            manager.Add(mscorlib);
        }
        List<(string Version, int MapId)> versionRows = [];
        Dictionary<string, int> mapIds = new(StringComparer.Ordinal);
        List<(int Id, string[] Entries)> maps = [];

        foreach (object pair in assemblies)
        {
            Type pairType = pair.GetType();
            UnityVersion version = (UnityVersion)(pairType.GetProperty("Key")?.GetValue(pair)
                ?? throw new MissingMemberException(pairType.FullName, "Key"));
            string versionText = version.ToString();
            string[] entries = ReferenceAssemblies.GetReferenceAssemblies(manager, version)
                .OrderBy(mapping => mapping.Key, StringComparer.Ordinal)
                .Select(mapping => $"{mapping.Key}|{mapping.Value}")
                .ToArray();
            string canonicalMap = string.Join("\n", entries);
            if (!mapIds.TryGetValue(canonicalMap, out int mapId))
            {
                mapId = maps.Count;
                mapIds.Add(canonicalMap, mapId);
                maps.Add((mapId, entries));
            }
            versionRows.Add((versionText, mapId));
        }

        using StreamWriter writer = new(outputPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        writer.WriteLine(mscorlibV2Path is null
            ? "# AssetRipper ReferenceAssemblies.GetReferenceAssemblies with HasMscorlib2=false."
            : "# AssetRipper ReferenceAssemblies.GetReferenceAssemblies with HasMscorlib2=true.");
        foreach ((string version, int mapId) in versionRows)
        {
            writer.WriteLine($"VERSION|{version}|{mapId}");
        }
        foreach ((int mapId, string[] entries) in maps)
        {
            foreach (string entry in entries)
            {
                writer.WriteLine($"ENTRY|{mapId}|{entry}");
            }
        }
        Console.WriteLine($"versions={versionRows.Count}");
        Console.WriteLine($"unique_maps={maps.Count}");
        Console.WriteLine($"output={outputPath}");
        return 0;
    }

    internal static void EnsureEmptyOrMissingDirectory(string path, string label)
    {
        if (File.Exists(path))
        {
            throw new IOException($"{label} root is a file: {path}");
        }
        if (Directory.Exists(path) && Directory.EnumerateFileSystemEntries(path).Any())
        {
            throw new IOException($"{label} root is not empty: {path}");
        }
    }

    internal static void WriteInputRow(StreamWriter rows, string path, byte[] bytes)
    {
        rows.WriteLine($"INPUT|{Path.GetFileName(path)}|{bytes.Length}|{Convert.ToHexString(SHA256.HashData(bytes))}");
        rows.WriteLine($"INPUT_HEX|{Path.GetFileName(path)}|{Convert.ToHexString(bytes)}");
    }
}
