using System.Security.Cryptography;
using System.Collections;
using System.Collections.Immutable;
using System.Reflection;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Text;
using System.Text.RegularExpressions;
using AssetRipper.Assets;
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
    internal static int ExportNestedReferenceBundle(string inputRootArgument, string outputRootArgument, string rowsArgument)
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

        byte[] managedFile = BuildManagedRootFile();
        byte[] targetFile = BuildTargetBehaviourFile();
        byte[] nestedBundle = BuildUnityFs(("target.assets", targetFile));
        byte[] outerBundle = BuildUnityFs(
        ("managed.assets", managedFile),
        ("nested.bundle", nestedBundle));
        string bundlePath = Path.Combine(inputRoot, "workspace.bundle");
        File.WriteAllBytes(bundlePath, outerBundle);

        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(new FullConfiguration());
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        string[] sourceObjects = gameData.GameBundle.FetchAssets()
        .Where(asset => asset.ClassID == 114 && asset.PathID is 42 or 900)
        .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
        .Order(StringComparer.Ordinal)
        .ToArray();
        if (sourceObjects.Length != 2
        || !sourceObjects.Any(line => line.StartsWith("MonoBehaviour|42|", StringComparison.Ordinal))
        || !sourceObjects.Any(line => line.StartsWith("MonoBehaviour|900|", StringComparison.Ordinal)))
        {
            throw new InvalidDataException($"AssetRipper did not resolve both nested-bundle objects: {string.Join(";", sourceObjects)}");
        }

        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);
        string projectAssets = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string[] assetFiles = Directory.Exists(projectAssets)
        ? Directory.GetFiles(projectAssets, "*", SearchOption.AllDirectories)
        .Where(path => new[] { ".asset", ".meta" }.Contains(Path.GetExtension(path), StringComparer.OrdinalIgnoreCase))
        .OrderBy(path => Path.GetRelativePath(outputRoot, path), StringComparer.Ordinal)
        .ToArray()
        : [];
        string? rootYamlPath = assetFiles.FirstOrDefault(path =>
        string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
        && File.ReadAllText(path).Contains("m_Name: ManagedRoot", StringComparison.Ordinal));
        string? targetYamlPath = assetFiles.FirstOrDefault(path =>
        string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
        && File.ReadAllText(path).Contains("m_Name: ReferencedRoot", StringComparison.Ordinal));
        if (rootYamlPath is null || targetYamlPath is null)
        {
            throw new InvalidDataException($"nested-bundle export omitted the root or target object: {string.Join(",", assetFiles.Select(path => Path.GetRelativePath(outputRoot, path)))}");
        }
        string targetMetaPath = targetYamlPath + ".meta";
        string targetGuid = File.ReadLines(targetMetaPath)
        .FirstOrDefault(line => line.StartsWith("guid: ", StringComparison.Ordinal))?[6..]
        ?? throw new InvalidDataException("nested-bundle target metadata has no GUID");

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine("# Frozen source AssetRipper export of a cross-file pointer through a nested UnityFS bundle.");
        rows.WriteLine("VERSION|2019.4.40f1|format=21|platform=StandaloneWin64Player");
        rows.WriteLine($"OBJECT|{string.Join(";", sourceObjects)}");
        WriteInputRow(rows, bundlePath, outerBundle);
        rows.WriteLine($"ENTRY|workspace.bundle|managed.assets|{managedFile.Length}|{Convert.ToHexString(SHA256.HashData(managedFile))}");
        rows.WriteLine($"ENTRY|workspace.bundle|nested.bundle|{nestedBundle.Length}|{Convert.ToHexString(SHA256.HashData(nestedBundle))}");
        rows.WriteLine($"ENTRY|nested.bundle|target.assets|{targetFile.Length}|{Convert.ToHexString(SHA256.HashData(targetFile))}");
        foreach (string file in assetFiles)
        {
            string relative = Path.GetRelativePath(outputRoot, file).Replace('\\', '/');
            byte[] bytes = File.ReadAllBytes(file);
            byte[] stableBytes = bytes;
            if (string.Equals(Path.GetExtension(file), ".asset", StringComparison.OrdinalIgnoreCase))
            {
                stableBytes = Encoding.UTF8.GetBytes(Encoding.UTF8.GetString(bytes).Replace(targetGuid, "<TARGET_GUID>", StringComparison.Ordinal));
            }
            else if (string.Equals(Path.GetExtension(file), ".meta", StringComparison.OrdinalIgnoreCase))
            {
                string normalized = string.Join(
                "\n",
                Encoding.UTF8.GetString(bytes)
                .Split('\n')
                .Where(line => !line.StartsWith("guid: ", StringComparison.Ordinal)
                && !line.StartsWith("timeCreated: ", StringComparison.Ordinal)))
                .TrimEnd('\r', '\n') + "\n";
                stableBytes = Encoding.UTF8.GetBytes(normalized);
            }
            rows.WriteLine($"FILE|{relative}|{stableBytes.Length}|{Convert.ToHexString(SHA256.HashData(stableBytes))}");
            if (new[] { ".asset", ".meta" }.Contains(Path.GetExtension(file), StringComparer.OrdinalIgnoreCase))
            {
                rows.WriteLine($"CONTENT|{relative}|{Convert.ToBase64String(stableBytes)}");
            }
        }
        Console.WriteLine($"source_objects={string.Join(';', sourceObjects)}");
        Console.WriteLine($"bundle_bytes={outerBundle.Length}");
        Console.WriteLine($"project_asset_files={assetFiles.Length}");
        return 0;
    }

    internal static int ExportEngineResourcePointer(
        string inputRootArgument,
        string outputRootArgument,
        string rowsArgument,
        string engineFileName)
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

        byte[] managedFile = BuildManagedRootFile(engineFileName);
        byte[] engineFile = BuildTargetBehaviourFile("EngineResourceTarget", 789, engineFileName);
        string managedPath = Path.Combine(inputRoot, "managed.assets");
        string enginePath = Path.Combine(inputRoot, engineFileName);
        File.WriteAllBytes(managedPath, managedFile);
        File.WriteAllBytes(enginePath, engineFile);

        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(new FullConfiguration());
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        string[] sourceObjects = gameData.GameBundle.FetchAssets()
            .Where(asset => asset.ClassID == 114 && asset.PathID is 42 or 900)
            .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
            .Order(StringComparer.Ordinal)
            .ToArray();
        if (sourceObjects.Length != 2
            || !sourceObjects.Any(line => line.StartsWith("MonoBehaviour|42|", StringComparison.Ordinal))
            || !sourceObjects.Any(line => line.StartsWith("MonoBehaviour|900|", StringComparison.Ordinal)))
        {
            throw new InvalidDataException($"AssetRipper did not load both engine-pointer objects: {string.Join(";", sourceObjects)}");
        }

        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);
        string projectAssets = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string[] assetFiles = Directory.Exists(projectAssets)
            ? Directory.GetFiles(projectAssets, "*", SearchOption.AllDirectories)
                .Where(path => new[] { ".asset", ".meta" }.Contains(Path.GetExtension(path), StringComparer.OrdinalIgnoreCase))
                .OrderBy(path => Path.GetRelativePath(outputRoot, path), StringComparer.Ordinal)
                .ToArray()
            : [];
        string? rootYamlPath = assetFiles.FirstOrDefault(path =>
            string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
            && File.ReadAllText(path).Contains("m_Name: ManagedRoot", StringComparison.Ordinal));
        if (rootYamlPath is null
            || assetFiles.Any(path => string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
                && File.ReadAllText(path).Contains("m_Name: EngineResourceTarget", StringComparison.Ordinal)))
        {
            throw new InvalidDataException($"AssetRipper did not redirect the explicit engine collection: {string.Join(",", assetFiles.Select(path => Path.GetRelativePath(outputRoot, path)))}");
        }
        string rootYaml = File.ReadAllText(rootYamlPath);
        string expectedGuid = engineFileName is "unity default resources" or "unity_default_resources"
            ? "0000000000000000e000000000000000"
            : "0000000000000000f000000000000000";
        if (!rootYaml.Contains($"guid: {expectedGuid}, type: 0", StringComparison.Ordinal)
            || !rootYaml.Contains("fileID: 900", StringComparison.Ordinal))
        {
            throw new InvalidDataException("AssetRipper did not emit the default-resource EGUID and internal PathID pointer");
        }

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine($"# Frozen source project export for an explicit {engineFileName} PPtr target.");
        rows.WriteLine("VERSION|2019.4.40f1|format=21|platform=StandaloneWin64Player");
        rows.WriteLine($"OBJECT|{string.Join(";", sourceObjects)}");
        WriteInputRow(rows, managedPath, managedFile);
        WriteInputRow(rows, enginePath, engineFile);
        foreach (string file in assetFiles)
        {
            string relative = Path.GetRelativePath(outputRoot, file).Replace('\\', '/');
            byte[] bytes = File.ReadAllBytes(file);
            byte[] stableBytes = bytes;
            if (string.Equals(Path.GetExtension(file), ".meta", StringComparison.OrdinalIgnoreCase))
            {
                string normalized = string.Join(
                    "\n",
                    Encoding.UTF8.GetString(bytes)
                        .Split('\n')
                        .Where(line => !line.StartsWith("guid: ", StringComparison.Ordinal)
                            && !line.StartsWith("timeCreated: ", StringComparison.Ordinal)))
                    .TrimEnd('\r', '\n') + "\n";
                stableBytes = Encoding.UTF8.GetBytes(normalized);
            }
            rows.WriteLine($"FILE|{relative}|{stableBytes.Length}|{Convert.ToHexString(SHA256.HashData(stableBytes))}");
            if (new[] { ".asset", ".meta" }.Contains(Path.GetExtension(file), StringComparer.OrdinalIgnoreCase))
            {
                rows.WriteLine($"CONTENT|{relative}|{Convert.ToBase64String(stableBytes)}");
            }
        }
        Console.WriteLine($"source_objects={string.Join(';', sourceObjects)}");
        Console.WriteLine($"project_asset_files={assetFiles.Length}");
        Console.WriteLine($"engine_pointer={expectedGuid}:900:0");
        return 0;
    }

    internal static int ExportInjectedEngineResourcePointer(
        string inputRootArgument,
        string outputRootArgument,
        string rowsArgument,
        string engineFileName,
        long targetPathId)
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
        if (Path.GetFileName(engineFileName) != engineFileName || string.IsNullOrWhiteSpace(engineFileName))
        {
            throw new ArgumentException("engine file name must be one safe relative component", nameof(engineFileName));
        }

        Directory.CreateDirectory(inputRoot);
        Directory.CreateDirectory(outputRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);

        byte[] managedFile = BuildManagedRootFile(engineFileName, assetPointerPathId: targetPathId);
        string managedPath = Path.Combine(inputRoot, "managed.assets");
        File.WriteAllBytes(managedPath, managedFile);

        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(new FullConfiguration());
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        AssetCollection? injectedCollection = gameData.GameBundle.ResolveCollection(engineFileName);
        if (injectedCollection is null)
        {
            throw new InvalidDataException("frozen source did not inject the missing engine-resource collection");
        }
        IUnityObjectBase target = injectedCollection.OfType<IUnityObjectBase>().SingleOrDefault(asset => asset.PathID == targetPathId)
            ?? throw new InvalidDataException($"injected collection has no asset at PathID {targetPathId}");
        string targetName = target is INamed named ? named.Name.ToString() : "<unnamed>";

        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);
        string projectAssets = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string[] assetFiles = Directory.Exists(projectAssets)
            ? Directory.GetFiles(projectAssets, "*", SearchOption.AllDirectories)
                .Where(path => new[] { ".asset", ".meta" }.Contains(Path.GetExtension(path), StringComparer.OrdinalIgnoreCase))
                .OrderBy(path => Path.GetRelativePath(outputRoot, path), StringComparer.Ordinal)
                .ToArray()
            : [];
        string? rootYamlPath = assetFiles.FirstOrDefault(path =>
            string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
            && File.ReadAllText(path).Contains("m_Name: ManagedRoot", StringComparison.Ordinal));
        if (rootYamlPath is null)
        {
            throw new InvalidDataException("injected engine-resource export omitted the managed root asset");
        }
        string rootYaml = File.ReadAllText(rootYamlPath);
        const string expectedGuid = "0000000000000000f000000000000000";
        if (!rootYaml.Contains($"guid: {expectedGuid}, type: 0", StringComparison.Ordinal)
            || !rootYaml.Contains($"fileID: {targetPathId}", StringComparison.Ordinal))
        {
            throw new InvalidDataException("export did not preserve the injected builtin-extra internal pointer");
        }

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine("# Frozen source project export after injecting an absent builtin-extra collection from EngineAssetsTpk.");
        rows.WriteLine("VERSION|2019.4.40f1|format=21|platform=StandaloneWin64Player");
        rows.WriteLine($"INJECTED_COLLECTION|{injectedCollection.Name}|{injectedCollection.GetType().Name}");
        rows.WriteLine($"TARGET|{target.ClassName}|{target.PathID}|{targetName}");
        rows.WriteLine($"INPUT|managed.assets|{managedFile.Length}|{Convert.ToHexString(SHA256.HashData(managedFile))}");
        rows.WriteLine($"INPUT_HEX|managed.assets|{Convert.ToHexString(managedFile)}");
        foreach (string file in assetFiles)
        {
            string relative = Path.GetRelativePath(outputRoot, file).Replace('\\', '/');
            byte[] bytes = File.ReadAllBytes(file);
            byte[] stableBytes = bytes;
            if (string.Equals(Path.GetExtension(file), ".meta", StringComparison.OrdinalIgnoreCase))
            {
                string normalized = string.Join(
                    "\n",
                    Encoding.UTF8.GetString(bytes)
                        .Split('\n')
                        .Where(line => !line.StartsWith("guid: ", StringComparison.Ordinal)
                            && !line.StartsWith("timeCreated: ", StringComparison.Ordinal)))
                    .TrimEnd('\r', '\n') + "\n";
                stableBytes = Encoding.UTF8.GetBytes(normalized);
            }
            rows.WriteLine($"FILE|{relative}|{stableBytes.Length}|{Convert.ToHexString(SHA256.HashData(stableBytes))}");
            rows.WriteLine($"CONTENT|{relative}|{Convert.ToBase64String(stableBytes)}");
        }
        Console.WriteLine($"injected_collection={injectedCollection.Name}");
        Console.WriteLine($"target={target.ClassName}|{target.PathID}|{targetName}");
        Console.WriteLine($"engine_pointer={expectedGuid}:{targetPathId}:0");
        Console.WriteLine($"project_asset_files={assetFiles.Length}");
        return 0;
    }

    internal static int ExportSiblingBundleReferenceOrder(string inputRootArgument, string outputRootArgument, string rowsArgument)
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

        byte[] managedFile = BuildManagedRootFile();
        byte[] firstTargetFile = BuildTargetBehaviourFile("FirstTarget", 789);
        byte[] secondTargetFile = BuildTargetBehaviourFile("SecondTarget", 456);
        byte[] firstBundle = BuildUnityFs(("target.assets", firstTargetFile));
        byte[] secondBundle = BuildUnityFs(("target.assets", secondTargetFile));
        byte[] outerBundle = BuildUnityFs(
            ("managed.assets", managedFile),
            ("first.bundle", firstBundle),
            ("second.bundle", secondBundle));
        string bundlePath = Path.Combine(inputRoot, "workspace.bundle");
        File.WriteAllBytes(bundlePath, outerBundle);

        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(new FullConfiguration());
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        string[] sourceObjects = gameData.GameBundle.FetchAssets()
            .Where(asset => asset.ClassID == 114 && asset.PathID is 42 or 900)
            .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
            .Order(StringComparer.Ordinal)
            .ToArray();
        if (sourceObjects.Length != 3
            || !sourceObjects.Any(line => line.Contains("ManagedRoot", StringComparison.Ordinal))
            || !sourceObjects.Any(line => line.Contains("FirstTarget", StringComparison.Ordinal))
            || !sourceObjects.Any(line => line.Contains("SecondTarget", StringComparison.Ordinal)))
        {
            throw new InvalidDataException($"AssetRipper did not load all duplicate-name sibling collections: {string.Join(";", sourceObjects)}");
        }

        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);
        string projectAssets = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string[] assetFiles = Directory.Exists(projectAssets)
            ? Directory.GetFiles(projectAssets, "*", SearchOption.AllDirectories)
                .Where(path => new[] { ".asset", ".meta" }.Contains(Path.GetExtension(path), StringComparer.OrdinalIgnoreCase))
                .OrderBy(path => Path.GetRelativePath(outputRoot, path), StringComparer.Ordinal)
                .ToArray()
            : [];
        string? rootYamlPath = assetFiles.FirstOrDefault(path =>
            string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
            && File.ReadAllText(path).Contains("m_Name: ManagedRoot", StringComparison.Ordinal));
        string? firstTargetYamlPath = assetFiles.FirstOrDefault(path =>
            string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
            && File.ReadAllText(path).Contains("m_Name: FirstTarget", StringComparison.Ordinal));
        string? secondTargetYamlPath = assetFiles.FirstOrDefault(path =>
            string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
            && File.ReadAllText(path).Contains("m_Name: SecondTarget", StringComparison.Ordinal));
        if (rootYamlPath is null || firstTargetYamlPath is null || secondTargetYamlPath is null)
        {
            throw new InvalidDataException($"sibling-bundle export omitted an object: {string.Join(",", assetFiles.Select(path => Path.GetRelativePath(outputRoot, path)))}");
        }
        string rootYaml = File.ReadAllText(rootYamlPath);
        string firstTargetMetaPath = firstTargetYamlPath + ".meta";
        string secondTargetMetaPath = secondTargetYamlPath + ".meta";
        string firstTargetGuid = File.ReadLines(firstTargetMetaPath)
            .FirstOrDefault(line => line.StartsWith("guid: ", StringComparison.Ordinal))?[6..]
            ?? throw new InvalidDataException("first sibling target metadata has no GUID");
        string secondTargetGuid = File.ReadLines(secondTargetMetaPath)
            .FirstOrDefault(line => line.StartsWith("guid: ", StringComparison.Ordinal))?[6..]
            ?? throw new InvalidDataException("second sibling target metadata has no GUID");
        if (!rootYaml.Contains(firstTargetGuid, StringComparison.Ordinal)
            || rootYaml.Contains(secondTargetGuid, StringComparison.Ordinal))
        {
            throw new InvalidDataException("the cross-file PPtr did not select the first same-named collection in source bundle-entry order");
        }

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine("# Frozen source AssetRipper export of duplicate collection names in sibling UnityFS bundles.");
        rows.WriteLine("VERSION|2019.4.40f1|format=21|platform=StandaloneWin64Player");
        rows.WriteLine($"OBJECT|{string.Join(";", sourceObjects)}");
        rows.WriteLine($"SELECTED|target.assets|FirstTarget|{firstTargetGuid}");
        WriteInputRow(rows, bundlePath, outerBundle);
        rows.WriteLine($"ENTRY|workspace.bundle|managed.assets|{managedFile.Length}|{Convert.ToHexString(SHA256.HashData(managedFile))}");
        rows.WriteLine($"ENTRY|workspace.bundle|first.bundle|{firstBundle.Length}|{Convert.ToHexString(SHA256.HashData(firstBundle))}");
        rows.WriteLine($"ENTRY|workspace.bundle|second.bundle|{secondBundle.Length}|{Convert.ToHexString(SHA256.HashData(secondBundle))}");
        rows.WriteLine($"ENTRY|first.bundle|target.assets|{firstTargetFile.Length}|{Convert.ToHexString(SHA256.HashData(firstTargetFile))}");
        rows.WriteLine($"ENTRY|second.bundle|target.assets|{secondTargetFile.Length}|{Convert.ToHexString(SHA256.HashData(secondTargetFile))}");
        foreach (string file in assetFiles)
        {
            string relative = Path.GetRelativePath(outputRoot, file).Replace('\\', '/');
            byte[] bytes = File.ReadAllBytes(file);
            byte[] stableBytes = bytes;
            if (string.Equals(Path.GetExtension(file), ".asset", StringComparison.OrdinalIgnoreCase))
            {
                stableBytes = Encoding.UTF8.GetBytes(Encoding.UTF8.GetString(bytes).Replace(firstTargetGuid, "<TARGET_GUID>", StringComparison.Ordinal));
            }
            else if (string.Equals(Path.GetExtension(file), ".meta", StringComparison.OrdinalIgnoreCase))
            {
                string normalized = string.Join(
                    "\n",
                    Encoding.UTF8.GetString(bytes)
                        .Split('\n')
                        .Where(line => !line.StartsWith("guid: ", StringComparison.Ordinal)
                            && !line.StartsWith("timeCreated: ", StringComparison.Ordinal)))
                    .TrimEnd('\r', '\n') + "\n";
                stableBytes = Encoding.UTF8.GetBytes(normalized);
            }
            rows.WriteLine($"FILE|{relative}|{stableBytes.Length}|{Convert.ToHexString(SHA256.HashData(stableBytes))}");
            if (new[] { ".asset", ".meta" }.Contains(Path.GetExtension(file), StringComparer.OrdinalIgnoreCase))
            {
                rows.WriteLine($"CONTENT|{relative}|{Convert.ToBase64String(stableBytes)}");
            }
        }
        string pathIdMapRelative = "AuxiliaryFiles/path_id_map.json";
        string pathIdMapPath = Path.Combine(outputRoot, "AuxiliaryFiles", "path_id_map.json");
        if (!File.Exists(pathIdMapPath))
        {
            throw new InvalidDataException("sibling-bundle export omitted AuxiliaryFiles/path_id_map.json");
        }
        byte[] pathIdMapBytes = File.ReadAllBytes(pathIdMapPath);
        rows.WriteLine($"FILE|{pathIdMapRelative}|{pathIdMapBytes.Length}|{Convert.ToHexString(SHA256.HashData(pathIdMapBytes))}");
        rows.WriteLine($"SOURCE_OUTPUT|{pathIdMapRelative}|{Convert.ToHexString(pathIdMapBytes)}");
        Console.WriteLine($"source_objects={string.Join(';', sourceObjects)}");
        Console.WriteLine($"selected_target=FirstTarget|{firstTargetGuid}");
        Console.WriteLine($"bundle_bytes={outerBundle.Length}");
        Console.WriteLine($"project_asset_files={assetFiles.Length}");
        return 0;
    }

    internal static int ExportNestedSiblingBundleReferenceOrder(string inputRootArgument, string outputRootArgument, string rowsArgument)
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

        byte[] managedFile = BuildManagedRootFile();
        byte[] firstTargetFile = BuildTargetBehaviourFile("FirstTarget", 789);
        byte[] secondTargetFile = BuildTargetBehaviourFile("SecondTarget", 456);
        byte[] sourceBundle = BuildUnityFs(("managed.assets", managedFile));
        byte[] firstBundle = BuildUnityFs(("target.assets", firstTargetFile));
        byte[] secondBundle = BuildUnityFs(("target.assets", secondTargetFile));
        byte[] outerBundle = BuildUnityFs(
            ("source.bundle", sourceBundle),
            ("first.bundle", firstBundle),
            ("second.bundle", secondBundle));
        string bundlePath = Path.Combine(inputRoot, "workspace.bundle");
        File.WriteAllBytes(bundlePath, outerBundle);

        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(new FullConfiguration());
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        string[] sourceObjects = gameData.GameBundle.FetchAssets()
            .Where(asset => asset.ClassID == 114 && asset.PathID is 42 or 900)
            .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
            .Order(StringComparer.Ordinal)
            .ToArray();
        if (sourceObjects.Length != 3
            || !sourceObjects.Any(line => line.Contains("ManagedRoot", StringComparison.Ordinal))
            || !sourceObjects.Any(line => line.Contains("FirstTarget", StringComparison.Ordinal))
            || !sourceObjects.Any(line => line.Contains("SecondTarget", StringComparison.Ordinal)))
        {
            throw new InvalidDataException($"AssetRipper did not load all nested and sibling collections: {string.Join(";", sourceObjects)}");
        }

        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);
        string projectAssets = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string[] assetFiles = Directory.Exists(projectAssets)
            ? Directory.GetFiles(projectAssets, "*", SearchOption.AllDirectories)
                .Where(path => new[] { ".asset", ".meta" }.Contains(Path.GetExtension(path), StringComparer.OrdinalIgnoreCase))
                .OrderBy(path => Path.GetRelativePath(outputRoot, path), StringComparer.Ordinal)
                .ToArray()
            : [];
        string? rootYamlPath = assetFiles.FirstOrDefault(path =>
            string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
            && File.ReadAllText(path).Contains("m_Name: ManagedRoot", StringComparison.Ordinal));
        string? firstTargetYamlPath = assetFiles.FirstOrDefault(path =>
            string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
            && File.ReadAllText(path).Contains("m_Name: FirstTarget", StringComparison.Ordinal));
        string? secondTargetYamlPath = assetFiles.FirstOrDefault(path =>
            string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
            && File.ReadAllText(path).Contains("m_Name: SecondTarget", StringComparison.Ordinal));
        if (rootYamlPath is null || firstTargetYamlPath is null || secondTargetYamlPath is null)
        {
            throw new InvalidDataException($"nested-source export omitted an object: {string.Join(",", assetFiles.Select(path => Path.GetRelativePath(outputRoot, path)))}");
        }

        string rootYaml = File.ReadAllText(rootYamlPath);
        string firstTargetGuid = File.ReadLines(firstTargetYamlPath + ".meta")
            .FirstOrDefault(line => line.StartsWith("guid: ", StringComparison.Ordinal))?[6..]
            ?? throw new InvalidDataException("first sibling target metadata has no GUID");
        string secondTargetGuid = File.ReadLines(secondTargetYamlPath + ".meta")
            .FirstOrDefault(line => line.StartsWith("guid: ", StringComparison.Ordinal))?[6..]
            ?? throw new InvalidDataException("second sibling target metadata has no GUID");
        if (!rootYaml.Contains(firstTargetGuid, StringComparison.Ordinal)
            || rootYaml.Contains(secondTargetGuid, StringComparison.Ordinal))
        {
            throw new InvalidDataException("the nested source collection did not resolve the first sibling target in outer entry order");
        }

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine("# Frozen source AssetRipper export when a nested collection resolves duplicate names in sibling bundles.");
        rows.WriteLine("VERSION|2019.4.40f1|format=21|platform=StandaloneWin64Player");
        rows.WriteLine($"OBJECT|{string.Join(";", sourceObjects)}");
        rows.WriteLine("SELECTED|source.bundle/managed.assets|target.assets|FirstTarget|<TARGET_GUID>");
        WriteInputRow(rows, bundlePath, outerBundle);
        rows.WriteLine($"ENTRY|workspace.bundle|source.bundle|{sourceBundle.Length}|{Convert.ToHexString(SHA256.HashData(sourceBundle))}");
        rows.WriteLine($"ENTRY|workspace.bundle|first.bundle|{firstBundle.Length}|{Convert.ToHexString(SHA256.HashData(firstBundle))}");
        rows.WriteLine($"ENTRY|workspace.bundle|second.bundle|{secondBundle.Length}|{Convert.ToHexString(SHA256.HashData(secondBundle))}");
        rows.WriteLine($"ENTRY|source.bundle|managed.assets|{managedFile.Length}|{Convert.ToHexString(SHA256.HashData(managedFile))}");
        rows.WriteLine($"ENTRY|first.bundle|target.assets|{firstTargetFile.Length}|{Convert.ToHexString(SHA256.HashData(firstTargetFile))}");
        rows.WriteLine($"ENTRY|second.bundle|target.assets|{secondTargetFile.Length}|{Convert.ToHexString(SHA256.HashData(secondTargetFile))}");
        string normalizedRootYaml = rootYaml.Replace(firstTargetGuid, "<TARGET_GUID>", StringComparison.Ordinal);
        rows.WriteLine($"ROOT_YAML|{Convert.ToBase64String(Encoding.UTF8.GetBytes(normalizedRootYaml))}");
        Console.WriteLine($"source_objects={string.Join(';', sourceObjects)}");
        Console.WriteLine($"selected_target=FirstTarget|{firstTargetGuid}");
        Console.WriteLine($"bundle_bytes={outerBundle.Length}");
        Console.WriteLine($"project_asset_files={assetFiles.Length}");
        return 0;
    }

    internal static int ExportSameBundleDuplicateReferenceOrder(string inputRootArgument, string outputRootArgument, string rowsArgument)
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

        byte[] managedFile = BuildManagedRootFile();
        byte[] firstTargetFile = BuildTargetBehaviourFile("FirstTarget", 789);
        byte[] secondTargetFile = BuildTargetBehaviourFile("SecondTarget", 456);
        byte[] bundle = BuildUnityFs(
            ("managed.assets", managedFile),
            ("target.assets", firstTargetFile),
            ("target.assets", secondTargetFile));
        string bundlePath = Path.Combine(inputRoot, "workspace.bundle");
        File.WriteAllBytes(bundlePath, bundle);

        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(new FullConfiguration());
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        string[] sourceObjects = gameData.GameBundle.FetchAssets()
            .Where(asset => asset.ClassID == 114 && asset.PathID is 42 or 900)
            .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
            .Order(StringComparer.Ordinal)
            .ToArray();
        if (sourceObjects.Length != 3
            || !sourceObjects.Any(line => line.Contains("ManagedRoot", StringComparison.Ordinal))
            || !sourceObjects.Any(line => line.Contains("FirstTarget", StringComparison.Ordinal))
            || !sourceObjects.Any(line => line.Contains("SecondTarget", StringComparison.Ordinal)))
        {
            throw new InvalidDataException($"AssetRipper did not load all same-bundle collections: {string.Join(";", sourceObjects)}");
        }

        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);
        string projectAssets = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string[] assetFiles = Directory.Exists(projectAssets)
            ? Directory.GetFiles(projectAssets, "*", SearchOption.AllDirectories)
                .Where(path => new[] { ".asset", ".meta" }.Contains(Path.GetExtension(path), StringComparer.OrdinalIgnoreCase))
                .OrderBy(path => Path.GetRelativePath(outputRoot, path), StringComparer.Ordinal)
                .ToArray()
            : [];
        string? rootYamlPath = assetFiles.FirstOrDefault(path =>
            string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
            && File.ReadAllText(path).Contains("m_Name: ManagedRoot", StringComparison.Ordinal));
        string? firstTargetYamlPath = assetFiles.FirstOrDefault(path =>
            string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
            && File.ReadAllText(path).Contains("m_Name: FirstTarget", StringComparison.Ordinal));
        string? secondTargetYamlPath = assetFiles.FirstOrDefault(path =>
            string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
            && File.ReadAllText(path).Contains("m_Name: SecondTarget", StringComparison.Ordinal));
        if (rootYamlPath is null || firstTargetYamlPath is null || secondTargetYamlPath is null)
        {
            throw new InvalidDataException("same-bundle project export omitted one of the three source objects");
        }

        string rootYaml = File.ReadAllText(rootYamlPath);
        string firstTargetGuid = File.ReadLines(firstTargetYamlPath + ".meta")
            .FirstOrDefault(line => line.StartsWith("guid: ", StringComparison.Ordinal))?[6..]
            ?? throw new InvalidDataException("first same-bundle target metadata has no GUID");
        string secondTargetGuid = File.ReadLines(secondTargetYamlPath + ".meta")
            .FirstOrDefault(line => line.StartsWith("guid: ", StringComparison.Ordinal))?[6..]
            ?? throw new InvalidDataException("second same-bundle target metadata has no GUID");
        if (!rootYaml.Contains(firstTargetGuid, StringComparison.Ordinal)
            || rootYaml.Contains(secondTargetGuid, StringComparison.Ordinal))
        {
            throw new InvalidDataException("the duplicate-name PPtr did not select the first direct collection");
        }

        string pathIdMapPath = Path.Combine(outputRoot, "AuxiliaryFiles", "path_id_map.json");
        if (!File.Exists(pathIdMapPath))
        {
            throw new InvalidDataException("same-bundle export omitted AuxiliaryFiles/path_id_map.json");
        }
        byte[] pathIdMap = File.ReadAllBytes(pathIdMapPath);
        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine("# Frozen source AssetRipper export for duplicate SerializedFile names in one UnityFS bundle.");
        rows.WriteLine("VERSION|2019.4.40f1|format=21|platform=StandaloneWin64Player");
        rows.WriteLine($"OBJECT|{string.Join(";", sourceObjects)}");
        rows.WriteLine("SELECTED|workspace.bundle/managed.assets|target.assets|FirstTarget|<TARGET_GUID>");
        WriteInputRow(rows, bundlePath, bundle);
        rows.WriteLine($"ENTRY|workspace.bundle|managed.assets|{managedFile.Length}|{Convert.ToHexString(SHA256.HashData(managedFile))}");
        rows.WriteLine($"ENTRY|workspace.bundle|target.assets|{firstTargetFile.Length}|{Convert.ToHexString(SHA256.HashData(firstTargetFile))}");
        rows.WriteLine($"ENTRY|workspace.bundle|target.assets|{secondTargetFile.Length}|{Convert.ToHexString(SHA256.HashData(secondTargetFile))}");
        string normalizedRootYaml = rootYaml.Replace(firstTargetGuid, "<TARGET_GUID>", StringComparison.Ordinal);
        rows.WriteLine($"ROOT_YAML|{Convert.ToBase64String(Encoding.UTF8.GetBytes(normalizedRootYaml))}");
        rows.WriteLine($"SOURCE_OUTPUT|AuxiliaryFiles/path_id_map.json|{Convert.ToHexString(pathIdMap)}");
        foreach (string path in new[] { rootYamlPath, firstTargetYamlPath, secondTargetYamlPath })
        {
            string relative = Path.GetRelativePath(projectAssets, path).Replace('\\', '/');
            string stableContent = string.Equals(path, rootYamlPath, StringComparison.Ordinal)
                ? normalizedRootYaml
                : File.ReadAllText(path);
            rows.WriteLine($"SOURCE_YAML|{relative}|{Convert.ToBase64String(Encoding.UTF8.GetBytes(stableContent))}");
        }
        Console.WriteLine($"source_objects={string.Join(';', sourceObjects)}");
        Console.WriteLine($"selected_target=FirstTarget|{firstTargetGuid}");
        Console.WriteLine($"bundle_bytes={bundle.Length}");
        Console.WriteLine($"project_asset_files={assetFiles.Length}");
        return 0;
    }

    internal static int ExportNestedMissingDependency(string inputRootArgument, string outputRootArgument, string rowsArgument)
    {
        return ExportNestedArrayDependencyCore(inputRootArgument, outputRootArgument, rowsArgument, false, false);
    }

    internal static int ExportNestedMissingDependencyArray(string inputRootArgument, string outputRootArgument, string rowsArgument)
    {
        return ExportNestedArrayDependencyCore(inputRootArgument, outputRootArgument, rowsArgument, true, false);
    }

    internal static int ExportNestedResolvedDependencyArray(string inputRootArgument, string outputRootArgument, string rowsArgument)
    {
        return ExportNestedArrayDependencyCore(inputRootArgument, outputRootArgument, rowsArgument, true, true);
    }

    internal static int ExportNestedDistinctResolvedDependencyArray(string inputRootArgument, string outputRootArgument, string rowsArgument)
    {
        return ExportNestedArrayDependencyCore(inputRootArgument, outputRootArgument, rowsArgument, true, true, true);
    }

    internal static int ExportNestedSeparateResolvedDependencyArray(string inputRootArgument, string outputRootArgument, string rowsArgument)
    {
        return ExportNestedArrayDependencyCore(inputRootArgument, outputRootArgument, rowsArgument, true, true, true, true);
    }

    private static int ExportNestedArrayDependencyCore(
        string inputRootArgument,
        string outputRootArgument,
        string rowsArgument,
        bool arrayAssetPointer,
        bool resolvedArrayPointer,
        bool distinctResolvedArrayTargets = false,
        bool separateResolvedArrayFiles = false)
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

        if (separateResolvedArrayFiles && (!resolvedArrayPointer || !distinctResolvedArrayTargets))
        {
            throw new ArgumentException("Separate target files require distinct resolved array targets");
        }
        string externalFileName = !resolvedArrayPointer
            ? "missing.assets"
            : separateResolvedArrayFiles
                ? "first-target.assets"
                : "target.assets";
        string secondExternalFileName = "second-target.assets";
        byte[] managedFile = BuildManagedRootFile(
            externalFileName,
            arrayAssetPointer,
            resolvedArrayPointer && !distinctResolvedArrayTargets,
            separateResolvedArrayFiles ? secondExternalFileName : null);
        byte[]? targetFile = resolvedArrayPointer
            ? separateResolvedArrayFiles
                ? BuildTargetBehaviourFileWithObjects(externalFileName, (900, "ReferencedRoot", 789))
                : distinctResolvedArrayTargets
                ? BuildTargetBehaviourFileWithObjects(
                    "target.assets",
                    (900, "ReferencedRoot", 789),
                    (901, "AlternateReferencedRoot", 456))
                : BuildTargetBehaviourFile()
            : null;
        byte[]? secondTargetFile = separateResolvedArrayFiles
            ? BuildTargetBehaviourFileWithObjects(secondExternalFileName, (901, "AlternateReferencedRoot", 456))
            : null;
        byte[] sourceBundle = separateResolvedArrayFiles
            ? BuildUnityFs(("managed.assets", managedFile))
            : targetFile is null
                ? BuildUnityFs(("managed.assets", managedFile))
                : BuildUnityFs(("managed.assets", managedFile), ("target.assets", targetFile));
        byte[]? firstBundle = separateResolvedArrayFiles
            ? BuildUnityFs((externalFileName, targetFile!))
            : null;
        byte[]? secondBundle = separateResolvedArrayFiles
            ? BuildUnityFs((secondExternalFileName, secondTargetFile!))
            : null;
        byte[] outerBundle = separateResolvedArrayFiles
            ? BuildUnityFs(
                ("source.bundle", sourceBundle),
                ("first.bundle", firstBundle!),
                ("second.bundle", secondBundle!))
            : BuildUnityFs(("source.bundle", sourceBundle));
        string bundlePath = Path.Combine(inputRoot, "workspace.bundle");
        File.WriteAllBytes(bundlePath, outerBundle);

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine(separateResolvedArrayFiles
            ? "# Frozen source AssetRipper export for resolved array PPtrs routed to separate nested files."
            : distinctResolvedArrayTargets
                ? "# Frozen source AssetRipper export for distinct resolved PPtrs in an array inside a nested collection."
            : resolvedArrayPointer
                ? "# Frozen source AssetRipper export for resolved PPtrs in an array inside a nested collection."
            : arrayAssetPointer
                ? "# Frozen source AssetRipper export for missing PPtrs in an array inside a nested collection."
                : "# Frozen source AssetRipper export for a missing external requested by a nested collection.");
        rows.WriteLine("VERSION|2019.4.40f1|format=21|platform=StandaloneWin64Player");
        rows.WriteLine(separateResolvedArrayFiles
            ? "DISTINCT_FILE_RESOLVED|source.bundle/managed.assets|first-target.assets|first.bundle/first-target.assets|900|second-target.assets|second.bundle/second-target.assets|901"
            : distinctResolvedArrayTargets
                ? "DISTINCT_RESOLVED|source.bundle/managed.assets|target.assets|source.bundle/target.assets|900|901"
            : resolvedArrayPointer
                ? "RESOLVED|source.bundle/managed.assets|target.assets|source.bundle/target.assets"
            : "MISSING|source.bundle/managed.assets|missing.assets");
        WriteInputRow(rows, bundlePath, outerBundle);
        rows.WriteLine($"ENTRY|workspace.bundle|source.bundle|{sourceBundle.Length}|{Convert.ToHexString(SHA256.HashData(sourceBundle))}");
        if (separateResolvedArrayFiles)
        {
            rows.WriteLine($"ENTRY|workspace.bundle|first.bundle|{firstBundle!.Length}|{Convert.ToHexString(SHA256.HashData(firstBundle))}");
            rows.WriteLine($"ENTRY|workspace.bundle|second.bundle|{secondBundle!.Length}|{Convert.ToHexString(SHA256.HashData(secondBundle))}");
        }
        rows.WriteLine($"ENTRY|source.bundle|managed.assets|{managedFile.Length}|{Convert.ToHexString(SHA256.HashData(managedFile))}");
        if (separateResolvedArrayFiles)
        {
            rows.WriteLine($"ENTRY|first.bundle|first-target.assets|{targetFile!.Length}|{Convert.ToHexString(SHA256.HashData(targetFile))}");
            rows.WriteLine($"ENTRY|second.bundle|second-target.assets|{secondTargetFile!.Length}|{Convert.ToHexString(SHA256.HashData(secondTargetFile))}");
        }
        else if (targetFile is not null)
        {
            rows.WriteLine($"ENTRY|source.bundle|target.assets|{targetFile.Length}|{Convert.ToHexString(SHA256.HashData(targetFile))}");
        }

        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(new FullConfiguration());
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        string[] sourceObjects = gameData.GameBundle.FetchAssets()
            .Where(asset => asset.ClassID == 114 && asset.PathID is 42 or 900 or 901)
            .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
            .Order(StringComparer.Ordinal)
            .ToArray();
        int expectedObjectCount = distinctResolvedArrayTargets ? 3 : resolvedArrayPointer ? 2 : 1;
        if (sourceObjects.Length != expectedObjectCount
            || !sourceObjects.Any(line => line.Contains("ManagedRoot", StringComparison.Ordinal))
            || (resolvedArrayPointer && !sourceObjects.Any(line => line.Contains("ReferencedRoot", StringComparison.Ordinal)))
            || (distinctResolvedArrayTargets && !sourceObjects.Any(line => line.Contains("AlternateReferencedRoot", StringComparison.Ordinal))))
        {
            throw new InvalidDataException($"AssetRipper loaded an unexpected object set for the nested array dependency: {string.Join(";", sourceObjects)}");
        }

        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);
        string projectAssets = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string? rootYamlPath = Directory.Exists(projectAssets)
            ? Directory.GetFiles(projectAssets, "*.asset", SearchOption.AllDirectories)
                .FirstOrDefault(path => File.ReadAllText(path).Contains("m_Name: ManagedRoot", StringComparison.Ordinal))
            : null;
        if (rootYamlPath is null)
        {
            throw new InvalidDataException("AssetRipper export omitted ManagedRoot for the missing external dependency");
        }

        string rootYaml = File.ReadAllText(rootYamlPath);
        string normalizedRootYaml = rootYaml.Replace("\r\n", "\n", StringComparison.Ordinal).Replace('\r', '\n');
        string? normalizedTargetYaml = null;
        string? normalizedSecondTargetYaml = null;
        string? targetGuid = null;
        string? secondTargetGuid = null;
        if (resolvedArrayPointer)
        {
            string? targetYamlPath = Directory.GetFiles(projectAssets, "*.asset", SearchOption.AllDirectories)
                .FirstOrDefault(path => File.ReadAllText(path).Contains("m_Name: ReferencedRoot", StringComparison.Ordinal));
            if (targetYamlPath is null)
            {
                throw new InvalidDataException("AssetRipper export omitted the resolved array target");
            }
            string targetMetaPath = targetYamlPath + ".meta";
            targetGuid = File.ReadLines(targetMetaPath)
                .FirstOrDefault(line => line.StartsWith("guid: ", StringComparison.Ordinal))?[6..]
                ?? throw new InvalidDataException("resolved array target metadata has no GUID");
            normalizedTargetYaml = File.ReadAllText(targetYamlPath)
                .Replace("\r\n", "\n", StringComparison.Ordinal)
                .Replace('\r', '\n');
            if (distinctResolvedArrayTargets)
            {
                string? secondTargetYamlPath = Directory.GetFiles(projectAssets, "*.asset", SearchOption.AllDirectories)
                    .FirstOrDefault(path => File.ReadAllText(path).Contains("m_Name: AlternateReferencedRoot", StringComparison.Ordinal));
                if (secondTargetYamlPath is null)
                {
                    throw new InvalidDataException("AssetRipper export omitted the second distinct resolved array target");
                }
                string secondTargetMetaPath = secondTargetYamlPath + ".meta";
                secondTargetGuid = File.ReadLines(secondTargetMetaPath)
                    .FirstOrDefault(line => line.StartsWith("guid: ", StringComparison.Ordinal))?[6..]
                    ?? throw new InvalidDataException("second resolved array target metadata has no GUID");
                normalizedSecondTargetYaml = File.ReadAllText(secondTargetYamlPath)
                    .Replace("\r\n", "\n", StringComparison.Ordinal)
                    .Replace('\r', '\n');
                normalizedRootYaml = normalizedRootYaml
                    .Replace(targetGuid, "<TARGET_900_GUID>", StringComparison.Ordinal)
                    .Replace(secondTargetGuid, "<TARGET_901_GUID>", StringComparison.Ordinal);
            }
            else
            {
                normalizedRootYaml = normalizedRootYaml.Replace(targetGuid, "<TARGET_GUID>", StringComparison.Ordinal);
            }
        }
        rows.WriteLine($"OBJECT|{string.Join(";", sourceObjects)}");
        if (arrayAssetPointer)
        {
            string[] pointerLines = normalizedRootYaml.Split('\n')
                .Where(line => line.TrimStart().StartsWith("- {fileID:", StringComparison.Ordinal))
                .Select(line => line.Trim())
                .ToArray();
            if (pointerLines.Length != 2)
            {
                throw new InvalidDataException($"AssetRipper emitted {pointerLines.Length} missing PPtrs for the two-element managed array");
            }
            rows.WriteLine($"POINTERS|{string.Join("|", pointerLines)}");
            Console.WriteLine($"source_array_pointers={string.Join("|", pointerLines)}");
        }
        else
        {
            string pointerLine = normalizedRootYaml.Split('\n')
                .FirstOrDefault(line => line.Contains("m_Asset:", StringComparison.Ordinal))
                ?? throw new InvalidDataException("AssetRipper root YAML omitted the managed m_Asset field");
            rows.WriteLine($"POINTER|{pointerLine.Trim()}");
            Console.WriteLine($"source_missing_pointer={pointerLine.Trim()}");
        }
        rows.WriteLine($"ROOT_YAML|{Convert.ToBase64String(Encoding.UTF8.GetBytes(normalizedRootYaml))}");
        if (normalizedTargetYaml is not null)
        {
            rows.WriteLine($"TARGET_YAML|{Convert.ToBase64String(Encoding.UTF8.GetBytes(normalizedTargetYaml))}");
            if (distinctResolvedArrayTargets)
            {
                rows.WriteLine($"TARGET_YAML_901|{Convert.ToBase64String(Encoding.UTF8.GetBytes(normalizedSecondTargetYaml!))}");
                rows.WriteLine($"SOURCE_TARGET_GUIDS|{targetGuid}|{secondTargetGuid}");
                rows.WriteLine("TARGET_GUID_NORMALIZATION|AssetExportCollection.GUID uses UnityGuid.NewGuid; target PathIDs 900 and 901 use their own target meta GUID placeholders.");
            }
            else
            {
                rows.WriteLine($"TARGET_GUID|{targetGuid}");
            }
        }
        Console.WriteLine($"source_objects={string.Join(';', sourceObjects)}");
        Console.WriteLine($"bundle_bytes={outerBundle.Length}");
        return 0;
    }

    internal static byte[] BuildUnityFs(params (string Path, byte[] Data)[] entries)
    {
        if (entries.Length == 0)
        {
            throw new ArgumentException("UnityFS fixture must contain at least one entry", nameof(entries));
        }

        List<byte> data = new();
        List<byte> directory = new();
        WriteUInt32BigEndian(directory, checked((uint)entries.Length));
        foreach ((string path, byte[] bytes) in entries)
        {
            WriteInt64BigEndian(directory, data.Count);
            WriteInt64BigEndian(directory, bytes.Length);
            WriteInt32BigEndian(directory, 4); // NodeFlags.SerializedFile; recursive dispatch uses the entry signature.
            WriteNullTerminatedUtf8(directory, path);
            data.AddRange(bytes);
        }

        List<byte> metadata = new(new byte[16]); // BlocksInfo.UncompressedDataHash.
        WriteUInt32BigEndian(metadata, 1);
        WriteUInt32BigEndian(metadata, checked((uint)data.Count));
        WriteUInt32BigEndian(metadata, checked((uint)data.Count));
        WriteUInt16BigEndian(metadata, 0); // CompressionType.None.
        metadata.AddRange(directory);

        List<byte> bundle = new();
        WriteNullTerminatedUtf8(bundle, "UnityFS");
        WriteInt32BigEndian(bundle, 6);
        WriteNullTerminatedUtf8(bundle, "2019.4.40f1");
        WriteNullTerminatedUtf8(bundle, "2019.4.40f1");
        int sizeOffset = bundle.Count;
        WriteInt64BigEndian(bundle, 0);
        WriteInt32BigEndian(bundle, metadata.Count);
        WriteInt32BigEndian(bundle, metadata.Count);
        WriteInt32BigEndian(bundle, 0x40); // Directory and block records are combined.
        bundle.AddRange(metadata);
        bundle.AddRange(data);
        PatchInt64BigEndian(bundle, sizeOffset, bundle.Count);
        return bundle.ToArray();
    }

    internal static void WriteNullTerminatedUtf8(List<byte> destination, string value)
    {
        destination.AddRange(Encoding.UTF8.GetBytes(value));
        destination.Add(0);
    }

    internal static void WriteUInt16BigEndian(List<byte> destination, ushort value)
    {
        destination.Add((byte)(value >> 8));
        destination.Add((byte)value);
    }

    internal static void WriteUInt32BigEndian(List<byte> destination, uint value)
    {
        destination.Add((byte)(value >> 24));
        destination.Add((byte)(value >> 16));
        destination.Add((byte)(value >> 8));
        destination.Add((byte)value);
    }

    internal static void WriteInt32BigEndian(List<byte> destination, int value) =>
        WriteUInt32BigEndian(destination, unchecked((uint)value));

    internal static void WriteInt64BigEndian(List<byte> destination, long value)
    {
        ulong bits = unchecked((ulong)value);
        for (int shift = 56; shift >= 0; shift -= 8)
        {
            destination.Add((byte)(bits >> shift));
        }
    }

    internal static void PatchInt64BigEndian(List<byte> destination, int offset, long value)
    {
        ulong bits = unchecked((ulong)value);
        for (int index = 0; index < 8; index++)
        {
            destination[offset + index] = (byte)(bits >> (56 - index * 8));
        }
    }
}
