// Source-runtime integration probe for serialized managed-reference registries.
// It authors only synthetic SerializedFiles under the caller's scratch directory,
// then imports and exports those files through the frozen AssetRipper product path.
using System.Security.Cryptography;
using System.Collections;
using System.Collections.Immutable;
using System.Reflection;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Text;
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

if (args.Length > 0 && args[0] == "compare-managed-assembly-pe")
{
    if (args.Length != 3)
    {
        Console.Error.WriteLine("usage: managed-project compare-managed-assembly-pe <source-saved-dll> <rust-saved-dll>");
        return 2;
    }
    return CompareManagedAssemblyPe(args[1], args[2]);
}

if (args.Length > 0 && args[0] == "export-mono-structure")
{
    if (args.Length != 4)
    {
        Console.Error.WriteLine("usage: managed-project export-mono-structure <input-root> <output-root> <rows-file>");
        return 2;
    }
    return ExportMonoStructure(args[1], args[2], args[3]);
}

if (args.Length > 0 && args[0] == "export-managed-schema")
{
    if (args.Length != 5)
    {
        Console.Error.WriteLine("usage: managed-project export-managed-schema <input-root> <output-root> <rows-file> <assembly-file>");
        return 2;
    }
    return ExportManagedSchema(args[1], args[2], args[3], args[4]);
}

if (args.Length > 0 && args[0] == "export-managed-reference-schema")
{
    if (args.Length < 5 || args.Length > 6)
    {
        Console.Error.WriteLine("usage: managed-project export-managed-reference-schema <input-root> <output-root> <rows-file> <assembly-file> [unity-version]");
        return 2;
    }
    return ExportManagedReferenceSchema(
        args[1],
        args[2],
        args[3],
        args[4],
        args.Length == 6 ? args[5] : "2021.2.0f1");
}

if (args.Length > 0 && args[0] == "export-nested-reference-bundle")
{
    if (args.Length != 4)
    {
        Console.Error.WriteLine("usage: managed-project export-nested-reference-bundle <input-root> <output-root> <rows-file>");
        return 2;
    }
    return ExportNestedReferenceBundle(args[1], args[2], args[3]);
}

if (args.Length > 0 && args[0] == "export-sibling-bundle-reference-order")
{
    if (args.Length != 4)
    {
        Console.Error.WriteLine("usage: managed-project export-sibling-bundle-reference-order <input-root> <output-root> <rows-file>");
        return 2;
    }
    return ExportSiblingBundleReferenceOrder(args[1], args[2], args[3]);
}

if (args.Length > 0 && args[0] == "export-empty-script")
{
    if (args.Length != 4)
    {
        Console.Error.WriteLine("usage: managed-project export-empty-script <input-root> <output-root> <rows-file>");
        return 2;
    }
    return ExportEmptyScript(args[1], args[2], args[3]);
}

if (args.Length > 0 && args[0] == "export-reference-script")
{
    if (args.Length != 4)
    {
        Console.Error.WriteLine("usage: managed-project export-reference-script <input-root> <output-root> <rows-file>");
        return 2;
    }
    return ExportReferenceScript(args[1], args[2], args[3]);
}

if (args.Length > 0 && args[0] == "export-manager-reference-script")
{
    if (args.Length < 5 || args.Length > 8)
    {
        Console.Error.WriteLine("usage: managed-project export-manager-reference-script <input-root> <output-root> <rows-file> <mscorlib-v2-file> [assembly-name] [unity-version] [script-export-mode]");
        return 2;
    }
    return ExportManagerReferenceScript(
        args[1],
        args[2],
        args[3],
        args[4],
        args.Length > 5 ? args[5] : "Mono.Cecil",
        args.Length > 6 ? args[6] : "2019.1.0f1",
        args.Length > 7 ? Enum.Parse<ScriptExportMode>(args[7], ignoreCase: false) : null);
}

if (args.Length > 0 && args[0] == "export-textasset")
{
    if (args.Length != 5)
    {
        Console.Error.WriteLine("usage: managed-project export-textasset <input-root> <output-root> <rows-file> <Bytes|Txt|Parse>");
        return 2;
    }
    return ExportTextAsset(args[1], args[2], args[3], args[4]);
}

if (args.Length > 0 && args[0] == "export-saved-script")
{
    if (args.Length != 5)
    {
        Console.Error.WriteLine("usage: managed-project export-saved-script <input-root> <output-root> <rows-file> <assembly-file>");
        return 2;
    }
    return ExportSavedScript(args[1], args[2], args[3], args[4]);
}

if (args.Length > 0 && args[0] == "reference-assembly-map")
{
    if (args.Length != 2)
    {
        Console.Error.WriteLine("usage: managed-project reference-assembly-map <new-output-file>");
        return 2;
    }
    return DumpReferenceAssemblyMap(args[1], null);
}

if (args.Length > 0 && args[0] == "reference-assembly-map-mscorlib2")
{
    if (args.Length != 3)
    {
        Console.Error.WriteLine("usage: managed-project reference-assembly-map-mscorlib2 <new-output-file> <mscorlib-v2-file>");
        return 2;
    }
    return DumpReferenceAssemblyMap(args[1], args[2]);
}

if (args.Length != 3)
{
    Console.Error.WriteLine("usage: managed-project <empty-input-root> <empty-output-root> <new-rows-file>");
    return 2;
}

string inputRoot = Path.GetFullPath(args[0]);
string outputRoot = Path.GetFullPath(args[1]);
string rowsPath = Path.GetFullPath(args[2]);
EnsureEmptyOrMissingDirectory(inputRoot, "input");
EnsureEmptyOrMissingDirectory(outputRoot, "output");
if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
{
    throw new IOException($"refusing to overwrite probe rows {rowsPath}");
}

Directory.CreateDirectory(inputRoot);
Directory.CreateDirectory(outputRoot);
Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);
string rootPath = Path.Combine(inputRoot, "managed.assets");
string targetPath = Path.Combine(inputRoot, "target.assets");
byte[] rootBytes = BuildManagedRootFile();
byte[] targetBytes = BuildTargetBehaviourFile();
File.WriteAllBytes(rootPath, rootBytes);
File.WriteAllBytes(targetPath, targetBytes);

Logger.Add(new ConsoleLogger());
ExportHandler handler = new(new FullConfiguration());
var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
string[] sourceObjects = gameData.GameBundle.FetchAssets()
    .Where(asset => asset.ClassID == 114 && asset.PathID is 42 or 900)
    .Select(asset => $"{asset.ClassName}|{asset.PathID}|{asset}")
    .Order(StringComparer.Ordinal)
    .ToArray();
if (sourceObjects.Length != 2 || !sourceObjects.Any(line => line.StartsWith("MonoBehaviour|42|", StringComparison.Ordinal))
    || !sourceObjects.Any(line => line.StartsWith("MonoBehaviour|900|", StringComparison.Ordinal)))
{
    throw new InvalidDataException($"AssetRipper loaded an unexpected source object set: {string.Join(";", sourceObjects)}");
}

handler.Export(gameData, outputRoot, LocalFileSystem.Instance);
string projectAssets = Path.Combine(outputRoot, "ExportedProject", "Assets");
string[] assetFiles = Directory.Exists(projectAssets)
    ? Directory.GetFiles(projectAssets, "*", SearchOption.AllDirectories)
        .Where(path => new[] { ".asset", ".txt", ".meta" }.Contains(Path.GetExtension(path), StringComparer.OrdinalIgnoreCase))
        .OrderBy(path => Path.GetRelativePath(outputRoot, path), StringComparer.Ordinal)
        .ToArray()
    : [];
string? monoYamlPath = assetFiles.FirstOrDefault(path =>
    string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
    && File.ReadAllText(path).Contains("m_Name: ManagedRoot", StringComparison.Ordinal));
string? targetYamlPath = assetFiles.FirstOrDefault(path =>
    string.Equals(Path.GetExtension(path), ".asset", StringComparison.OrdinalIgnoreCase)
    && File.ReadAllText(path).Contains("m_Name: ReferencedRoot", StringComparison.Ordinal));
if (monoYamlPath is null || targetYamlPath is null)
{
    throw new InvalidDataException($"expected root and referenced MonoBehaviour outputs; found {string.Join(",", assetFiles.Select(path => Path.GetRelativePath(outputRoot, path)))}");
}

using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
rows.WriteLine("# Frozen AssetRipper managed-reference workspace/project-export probe.");
rows.WriteLine("VERSION|2019.4.40f1|format=21|platform=StandaloneWin64Player");
rows.WriteLine($"OBJECT|{string.Join(";", sourceObjects)}");
WriteInputRow(rows, rootPath, rootBytes);
WriteInputRow(rows, targetPath, targetBytes);
foreach (string file in assetFiles)
{
    string relative = Path.GetRelativePath(outputRoot, file).Replace('\\', '/');
    byte[] bytes = File.ReadAllBytes(file);
    rows.WriteLine($"FILE|{relative}|{bytes.Length}|{Convert.ToHexString(SHA256.HashData(bytes))}");
    if (string.Equals(Path.GetExtension(file), ".asset", StringComparison.OrdinalIgnoreCase)
        || string.Equals(Path.GetExtension(file), ".txt", StringComparison.OrdinalIgnoreCase))
    {
        rows.WriteLine($"CONTENT|{relative}|{Convert.ToBase64String(bytes)}");
    }
}
Console.WriteLine($"source_objects={string.Join(';', sourceObjects)}");
Console.WriteLine($"project_assets={assetFiles.Length}");
Console.WriteLine($"mono_behaviour={Path.GetRelativePath(outputRoot, monoYamlPath).Replace('\\', '/')}");
Console.WriteLine($"referenced_behaviour={Path.GetRelativePath(outputRoot, targetYamlPath).Replace('\\', '/')}");
return 0;

static int ExportNestedReferenceBundle(string inputRootArgument, string outputRootArgument, string rowsArgument)
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

static int ExportSiblingBundleReferenceOrder(string inputRootArgument, string outputRootArgument, string rowsArgument)
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
    Console.WriteLine($"source_objects={string.Join(';', sourceObjects)}");
    Console.WriteLine($"selected_target=FirstTarget|{firstTargetGuid}");
    Console.WriteLine($"bundle_bytes={outerBundle.Length}");
    Console.WriteLine($"project_asset_files={assetFiles.Length}");
    return 0;
}

static byte[] BuildUnityFs(params (string Path, byte[] Data)[] entries)
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

static void WriteNullTerminatedUtf8(List<byte> destination, string value)
{
    destination.AddRange(Encoding.UTF8.GetBytes(value));
    destination.Add(0);
}

static void WriteUInt16BigEndian(List<byte> destination, ushort value)
{
    destination.Add((byte)(value >> 8));
    destination.Add((byte)value);
}

static void WriteUInt32BigEndian(List<byte> destination, uint value)
{
    destination.Add((byte)(value >> 24));
    destination.Add((byte)(value >> 16));
    destination.Add((byte)(value >> 8));
    destination.Add((byte)value);
}

static void WriteInt32BigEndian(List<byte> destination, int value) =>
    WriteUInt32BigEndian(destination, unchecked((uint)value));

static void WriteInt64BigEndian(List<byte> destination, long value)
{
    ulong bits = unchecked((ulong)value);
    for (int shift = 56; shift >= 0; shift -= 8)
    {
        destination.Add((byte)(bits >> shift));
    }
}

static void PatchInt64BigEndian(List<byte> destination, int offset, long value)
{
    ulong bits = unchecked((ulong)value);
    for (int index = 0; index < 8; index++)
    {
        destination[offset + index] = (byte)(bits >> (56 - index * 8));
    }
}

static int ExportMonoStructure(string inputRootArgument, string outputRootArgument, string rowsArgument)
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

static int ExportManagedSchema(
    string inputRootArgument,
    string outputRootArgument,
    string rowsArgument,
    string assemblyArgument)
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
    byte[] serializedBytes = BuildManagedSchemaFile();
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
    rows.WriteLine($"INPUT_HEX|managed_schema.assets|{Convert.ToHexString(serializedBytes)}");
    rows.WriteLine($"ASSEMBLY|ComplexFixtureScripts.dll|{assemblyBytes.Length}|{Convert.ToHexString(SHA256.HashData(assemblyBytes))}");
    foreach (string item in matches)
    {
        rows.WriteLine($"OBJECT|{item}");
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

static int ExportManagedReferenceSchema(
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

static void DumpSourceSchema(SerializableType serializableType, string identity, StreamWriter rows)
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

static string GuidFromMeta(string path)
{
    string line = File.ReadAllLines(path)
        .FirstOrDefault(line => line.StartsWith("guid: ", StringComparison.Ordinal))
        ?? throw new InvalidDataException($"metadata file has no GUID: {path}");
    return line[6..];
}

static string GuidFromYamlLine(string line)
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

static byte[] BuildManagedSchemaFile()
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
        Version = UnityVersion.Parse("2019.4.40f1"),
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

static byte[] BuildManagedReferenceSchemaFile(string unityVersionText)
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

static byte[] BuildManagedReferenceTargetFile(string unityVersionText)
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

static void WriteManagedTypeTriple(List<byte> payload, string className, string namespaceName, string assemblyName)
{
    WriteString(payload, className);
    WriteString(payload, namespaceName);
    WriteString(payload, assemblyName);
}

static void AlignFour(List<byte> bytes)
{
    while ((bytes.Count & 3) != 0)
    {
        bytes.Add(0);
    }
}

static int ExportEmptyScript(string inputRootArgument, string outputRootArgument, string rowsArgument)
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

static byte[] NormalizeMonoImporterMeta(byte[] bytes)
{
    string text = Encoding.UTF8.GetString(bytes);
    string[] lines = text.Split(["\r\n", "\n"], StringSplitOptions.None);
    string normalized = string.Join(
        "\n",
        lines.Where(line => !line.StartsWith("timeCreated: ", StringComparison.Ordinal)));
    return Encoding.UTF8.GetBytes(normalized);
}

static byte[] BuildGenericScriptFile()
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

static int ExportReferenceScript(string inputRootArgument, string outputRootArgument, string rowsArgument)
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

static int ExportManagerReferenceScript(
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
    string behaviourRelative = $"Assets/{Path.GetRelativePath(assetsRoot, behaviourPath).Replace('\\', '/') }";
    byte[] behaviourBytes = File.ReadAllBytes(behaviourPath);
    rows.WriteLine($"FILE|{behaviourRelative}|{behaviourBytes.Length}|{Convert.ToHexString(SHA256.HashData(behaviourBytes))}");
    rows.WriteLine($"CONTENT_HEX|{behaviourRelative}|{Convert.ToHexString(behaviourBytes)}");
    Console.WriteLine($"manager={gameData.AssemblyManager.ScriptingBackend}|{gameData.AssemblyManager.IsSet}|{gameData.AssemblyManager.HasMscorlib2}");
    Console.WriteLine($"source_object={scripts[0]}");
    Console.WriteLine($"behaviour={behaviourRelative}");
    return 0;
}

static int ExportTextAsset(string inputRootArgument, string outputRootArgument, string rowsArgument, string modeArgument)
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

static int ExportSavedScript(string inputRootArgument, string outputRootArgument, string rowsArgument, string assemblyArgument)
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

static byte[] BuildSavedScriptFile()
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

static byte[] BuildSavedBehaviourFile()
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

static byte[] BuildReferenceScriptFile(string assemblyName = "UnityEngine", string versionText = "2019.4.40f1")
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

static byte[] BuildReferenceBehaviourFile(string versionText = "2019.4.40f1")
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

static int DumpReferenceAssemblyMap(string outputPathArgument, string? mscorlibV2Path)
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

static void EnsureEmptyOrMissingDirectory(string path, string label)
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

static void WriteInputRow(StreamWriter rows, string path, byte[] bytes)
{
    rows.WriteLine($"INPUT|{Path.GetFileName(path)}|{bytes.Length}|{Convert.ToHexString(SHA256.HashData(bytes))}");
    rows.WriteLine($"INPUT_HEX|{Path.GetFileName(path)}|{Convert.ToHexString(bytes)}");
}

static byte[] BuildManagedRootFile()
{
    const string versionText = "2019.4.40f1";
    List<byte> payload = [];
    WriteI32(payload, 0); // m_GameObject.m_FileID
    WriteI64(payload, 0); // m_GameObject.m_PathID
    payload.Add(1);       // m_Enabled
    while (payload.Count % 4 != 0)
    {
        payload.Add(0);
    }
    WriteI32(payload, 0); // m_Script.m_FileID
    WriteI64(payload, 0); // m_Script.m_PathID
    WriteString(payload, "ManagedRoot");

    WriteI64(payload, 100); // m_Root.rid -> NodeA
    WriteI32(payload, 2);   // counted managed-reference registry
    WriteI32(payload, 2);   // two entries
    WriteI64(payload, 100);
    WriteManagedType(payload, "NodeA");
    WriteI32(payload, 123); // NodeA.m_Value
    WriteI64(payload, 200); // NodeA.m_Link -> NodeB
    WriteI32(payload, 1);   // NodeA.m_Asset.m_FileID -> target.assets
    WriteI64(payload, 900); // NodeA.m_Asset.m_PathID
    WriteI64(payload, 200);
    WriteManagedType(payload, "NodeB");
    WriteI32(payload, 456); // NodeB.m_Value
    WriteI64(payload, 100); // NodeB.m_Link -> NodeA (cycle)
    WriteI32(payload, 0);   // NodeB.m_Asset.m_FileID
    WriteI64(payload, 0);   // NodeB.m_Asset.m_PathID

    SerializedType monoBehaviourType = new() { RawTypeID = 114 };
    AssignTree(monoBehaviourType, MonoBehaviourTree());
    SerializedTypeReference nodeAType = MakeReferenceType("NodeA", NodeTree("NodeA"));
    SerializedTypeReference nodeBType = MakeReferenceType("NodeB", NodeTree("NodeB"));
    SerializedFile file = new SerializedFileBuilder
    {
        Generation = FormatVersion.StoresTypeDependencies,
        Version = UnityVersion.Parse(versionText),
        Platform = (BuildTarget)19,
        EndianType = EndianType.LittleEndian,
        HasTypeTree = true,
        Types = { monoBehaviourType },
        RefTypes = { nodeAType, nodeBType },
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
        Objects =
        {
            new ObjectInfo(monoBehaviourType)
            {
                FileID = 42,
                TypeID = 114,
                ObjectData = payload.ToArray(),
            },
        },
    }.Build();
    file.FilePath = "managed.assets";
    return file.ToByteArray();
}

static byte[] BuildTargetBehaviourFile(string name = "ReferencedRoot", int health = 789)
{
    List<byte> payload = [];
    WriteI32(payload, 0); // m_GameObject.m_FileID
    WriteI64(payload, 0); // m_GameObject.m_PathID
    payload.Add(1);       // m_Enabled
    while (payload.Count % 4 != 0)
    {
        payload.Add(0);
    }
    WriteI32(payload, 0); // m_Script.m_FileID
    WriteI64(payload, 0); // m_Script.m_PathID
    WriteString(payload, name);
    WriteI32(payload, health); // m_Health
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
        Objects =
        {
            new ObjectInfo(monoBehaviourType)
            {
                FileID = 900,
                TypeID = 114,
                ObjectData = payload.ToArray(),
            },
        },
    }.Build();
    file.FilePath = "target.assets";
    return file.ToByteArray();
}

static TypeTree MonoBehaviourTargetTree()
{
    return MakeTree(
    [
        ("MonoBehaviour", "Base", 0, -1, 32768),
        ("PPtr<GameObject>", "m_GameObject", 1, 12, 65),
        ("int", "m_FileID", 2, 4, 65),
        ("SInt64", "m_PathID", 2, 8, 65),
        ("UInt8", "m_Enabled", 1, 1, 16641),
        ("PPtr<MonoScript>", "m_Script", 1, 12, 0),
        ("int", "m_FileID", 2, 4, 8388609),
        ("SInt64", "m_PathID", 2, 8, 8388609),
        ("string", "m_Name", 1, -1, 557057),
        ("Array", "Array", 2, -1, 540673),
        ("int", "size", 3, 4, 524289),
        ("char", "data", 3, 1, 524289),
        ("int", "m_Health", 1, 4, 0),
    ]);
}

static TypeTree MonoBehaviourTree()
{
    return MakeTree(
    [
        ("MonoBehaviour", "Base", 0, -1, 32768),
        ("PPtr<GameObject>", "m_GameObject", 1, 12, 65),
        ("int", "m_FileID", 2, 4, 65),
        ("SInt64", "m_PathID", 2, 8, 65),
        ("UInt8", "m_Enabled", 1, 1, 16641),
        ("PPtr<MonoScript>", "m_Script", 1, 12, 0),
        ("int", "m_FileID", 2, 4, 8388609),
        ("SInt64", "m_PathID", 2, 8, 8388609),
        ("string", "m_Name", 1, -1, 557057),
        ("Array", "Array", 2, -1, 540673),
        ("int", "size", 3, 4, 524289),
        ("char", "data", 3, 1, 524289),
        ("managedReference", "m_Root", 1, -1, 0),
        ("SInt64", "rid", 2, 8, 0),
        ("ManagedReferencesRegistry", "references", 1, -1, 0),
        ("int", "version", 2, 4, 0),
        ("vector", "RefIds", 2, -1, 0),
        ("Array", "Array", 3, -1, 0),
        ("int", "size", 4, 4, 0),
        ("ReferencedObjectData", "data", 4, -1, 0),
    ]);
}

static TypeTree NodeTree(string name)
{
    return MakeTree(
    [
        (name, name, 0, -1, 0),
        ("int", "m_Value", 1, 4, 0),
        ("managedReference", "m_Link", 1, -1, 0),
        ("SInt64", "rid", 2, 8, 0),
        ("PPtr<$Object>", "m_Asset", 1, 12, 0),
        ("SInt32", "m_FileID", 2, 4, 0),
        ("SInt64", "m_PathID", 2, 8, 0),
    ]);
}

static SerializedTypeReference MakeReferenceType(string className, TypeTree tree)
{
    SerializedTypeReference type = new()
    {
        RawTypeID = 114,
        ClassName = className,
        Namespace = "Example",
        AsmName = "Assembly-CSharp.dll",
    };
    AssignReferenceTree(type, tree);
    return type;
}

static void AssignTree(SerializedType type, TypeTree tree)
{
    type.OldType.Nodes.AddRange(tree.Nodes);
    type.OldType.StringBuffer = tree.StringBuffer;
}

static void AssignReferenceTree(SerializedTypeReference type, TypeTree tree)
{
    type.OldType.Nodes.AddRange(tree.Nodes);
    type.OldType.StringBuffer = tree.StringBuffer;
}

static TypeTree MakeTree((string Type, string Name, int Level, int ByteSize, int Meta)[] specs)
{
    List<byte> buffer = [];
    List<uint> typeOffsets = [];
    List<uint> nameOffsets = [];
    foreach ((string type, string nodeName, _, _, _) in specs)
    {
        typeOffsets.Add((uint)buffer.Count);
        buffer.AddRange(Encoding.UTF8.GetBytes(type));
        buffer.Add(0);
        nameOffsets.Add((uint)buffer.Count);
        buffer.AddRange(Encoding.UTF8.GetBytes(nodeName));
        buffer.Add(0);
    }
    TypeTree tree = new() { StringBuffer = buffer.ToArray() };
    for (int index = 0; index < specs.Length; index++)
    {
        tree.Nodes.Add(new TypeTreeNode
        {
            Version = 1,
            Level = (byte)specs[index].Level,
            TypeFlags = 0,
            TypeStrOffset = typeOffsets[index],
            NameStrOffset = nameOffsets[index],
            ByteSize = specs[index].ByteSize,
            Index = index,
            MetaFlag = (TransferMetaFlags)specs[index].Meta,
        });
    }
    return tree;
}

static void WriteManagedType(List<byte> payload, string className)
{
    WriteString(payload, className);
    WriteString(payload, "Example");
    WriteString(payload, "Assembly-CSharp");
}

static void WriteString(List<byte> payload, string value)
{
    byte[] bytes = Encoding.UTF8.GetBytes(value);
    WriteI32(payload, bytes.Length);
    payload.AddRange(bytes);
    while (payload.Count % 4 != 0)
    {
        payload.Add(0);
    }
}

static void WriteI32(List<byte> payload, int value) => payload.AddRange(BitConverter.GetBytes(value));

static void WriteI64(List<byte> payload, long value) => payload.AddRange(BitConverter.GetBytes(value));

static int CompareManagedAssemblyPe(string sourcePath, string rustPath)
{
    string[] sourceRows = ReadManagedAssemblyPeRows(sourcePath);
    string[] rustRows = ReadManagedAssemblyPeRows(rustPath);
    Console.WriteLine($"SOURCE_ROWS|{sourceRows.Length}");
    Console.WriteLine($"RUST_ROWS|{rustRows.Length}");
    int commonCount = Math.Min(sourceRows.Length, rustRows.Length);
    bool matches = true;
    for (int index = 0; index < commonCount; index++)
    {
        if (!string.Equals(sourceRows[index], rustRows[index], StringComparison.Ordinal))
        {
            Console.Error.WriteLine($"MISMATCH|{index}|SOURCE|{sourceRows[index]}|RUST|{rustRows[index]}");
            matches = false;
        }
    }
    if (sourceRows.Length != rustRows.Length)
    {
        Console.Error.WriteLine($"ROW_COUNT_MISMATCH|SOURCE|{sourceRows.Length}|RUST|{rustRows.Length}");
        matches = false;
    }
    if (!matches) return 1;
    Console.WriteLine("MANAGED_ASSEMBLY_PE_MODEL_MATCH");
    return 0;
}

static string[] ReadManagedAssemblyPeRows(string path)
{
    using var stream = File.OpenRead(path);
    using var peReader = new PEReader(stream);
    if (!peReader.HasMetadata)
    {
        throw new BadImageFormatException($"PE file has no CLR metadata: {path}");
    }
    MetadataReader reader = peReader.GetMetadataReader();
    var signatureProvider = new ManagedAssemblySignatureProvider();
    var rows = new List<string>();
    ModuleDefinition module = reader.GetModuleDefinition();
    rows.Add($"MODULE|{reader.GetString(module.Name)}|{reader.GetGuid(module.Mvid)}");
    rows.Add($"MACHINE|{peReader.PEHeaders.CoffHeader.Machine}");
    if (reader.IsAssembly)
    {
        System.Reflection.Metadata.AssemblyDefinition assembly = reader.GetAssemblyDefinition();
        string publicKey = assembly.PublicKey.IsNil ? "" : Convert.ToHexString(reader.GetBlobBytes(assembly.PublicKey));
        rows.Add($"ASSEMBLY|{reader.GetString(assembly.Name)}|{assembly.Version}|{assembly.Flags}|{publicKey}");
    }
    foreach (AssemblyReferenceHandle handle in reader.AssemblyReferences)
    {
        AssemblyReference reference = reader.GetAssemblyReference(handle);
        string publicKeyOrToken = reference.PublicKeyOrToken.IsNil
            ? ""
            : Convert.ToHexString(reader.GetBlobBytes(reference.PublicKeyOrToken));
        string culture = reference.Culture.IsNil ? "" : reader.GetString(reference.Culture);
        rows.Add($"REFERENCE|{reader.GetString(reference.Name)}|{reference.Version}|{reference.Flags}|{culture}|{publicKeyOrToken}");
    }
    foreach (ExportedTypeHandle handle in reader.ExportedTypes)
    {
        ExportedType exported = reader.GetExportedType(handle);
        string namespaceName = exported.Namespace.IsNil ? "" : reader.GetString(exported.Namespace);
        string implementation = exported.Implementation.Kind switch
        {
            HandleKind.AssemblyReference => $"ASSEMBLY:{reader.GetString(reader.GetAssemblyReference((AssemblyReferenceHandle)exported.Implementation).Name)}",
            HandleKind.ExportedType => $"EXPORTED:{reader.GetString(reader.GetExportedType((ExportedTypeHandle)exported.Implementation).Name)}",
            HandleKind.AssemblyFile => $"FILE:{reader.GetString(reader.GetAssemblyFile((AssemblyFileHandle)exported.Implementation).Name)}",
            _ => exported.Implementation.Kind.ToString(),
        };
        rows.Add($"EXPORTED_TYPE|{namespaceName}|{reader.GetString(exported.Name)}|{exported.Attributes}|{implementation}");
    }
    rows = rows.OrderBy(row => row, StringComparer.Ordinal).ToList();
    foreach (TypeDefinitionHandle typeHandle in reader.TypeDefinitions)
    {
        TypeDefinition type = reader.GetTypeDefinition(typeHandle);
        string typeName = GetTypeDefinitionName(reader, typeHandle);
        string baseType = type.BaseType.IsNil ? "" : GetTypeHandleName(reader, type.BaseType);
        rows.Add($"TYPE|{typeName}|{baseType}|{type.Attributes}");
        foreach (FieldDefinitionHandle fieldHandle in type.GetFields()
            .OrderBy(handle => reader.GetString(reader.GetFieldDefinition(handle).Name), StringComparer.Ordinal))
        {
            FieldDefinition field = reader.GetFieldDefinition(fieldHandle);
            string fieldType = field.DecodeSignature(signatureProvider, null);
            rows.Add($"FIELD|{typeName}|{reader.GetString(field.Name)}|{fieldType}|{field.Attributes}");
        }
        foreach (MethodDefinitionHandle methodHandle in type.GetMethods()
            .OrderBy(handle => reader.GetString(reader.GetMethodDefinition(handle).Name), StringComparer.Ordinal))
        {
            MethodDefinition method = reader.GetMethodDefinition(methodHandle);
            var signature = method.DecodeSignature(signatureProvider, null);
            string parameters = string.Join(',', signature.ParameterTypes);
            string body = "no-body";
            if (method.RelativeVirtualAddress != 0)
            {
                MethodBodyBlock methodBody = peReader.GetMethodBody(method.RelativeVirtualAddress);
                int ilLength = methodBody.GetILBytes()?.Length ?? 0;
                string locals = methodBody.LocalSignature.IsNil
                    ? ""
                    : string.Join(',', reader.GetStandaloneSignature(methodBody.LocalSignature)
                        .DecodeLocalSignature(signatureProvider, null));
                string exceptions = string.Join(';', methodBody.ExceptionRegions.Select(region =>
                    $"{region.Kind}:{region.TryOffset}+{region.TryLength}:{region.HandlerOffset}+{region.HandlerLength}:{region.FilterOffset}:{(region.CatchType.IsNil ? "" : GetTypeHandleName(reader, region.CatchType))}"));
                body = $"il={ilLength};maxStack={methodBody.MaxStack};initLocals={methodBody.LocalVariablesInitialized};locals={locals};exceptions={exceptions}";
            }
            string methodName = reader.GetString(method.Name);
            rows.Add($"METHOD|{typeName}|{methodName}|{signature.ReturnType}({parameters})|{method.Attributes}|0x{((int)method.ImplAttributes):X4}|{body}");
        }
        foreach (PropertyDefinitionHandle propertyHandle in type.GetProperties()
            .OrderBy(handle => reader.GetString(reader.GetPropertyDefinition(handle).Name), StringComparer.Ordinal))
        {
            PropertyDefinition property = reader.GetPropertyDefinition(propertyHandle);
            var signature = property.DecodeSignature(signatureProvider, null);
            PropertyAccessors accessors = property.GetAccessors();
            string getter = accessors.Getter.IsNil ? "" : reader.GetString(reader.GetMethodDefinition(accessors.Getter).Name);
            string setter = accessors.Setter.IsNil ? "" : reader.GetString(reader.GetMethodDefinition(accessors.Setter).Name);
            string others = string.Join(',', accessors.Others.Select(handle => reader.GetString(reader.GetMethodDefinition(handle).Name)));
            rows.Add($"PROPERTY|{typeName}|{reader.GetString(property.Name)}|{signature.ReturnType}({string.Join(',', signature.ParameterTypes)})|{property.Attributes}|{getter}|{setter}|{others}");
        }
        foreach (EventDefinitionHandle eventHandle in type.GetEvents()
            .OrderBy(handle => reader.GetString(reader.GetEventDefinition(handle).Name), StringComparer.Ordinal))
        {
            EventDefinition eventDefinition = reader.GetEventDefinition(eventHandle);
            EventAccessors accessors = eventDefinition.GetAccessors();
            string add = accessors.Adder.IsNil ? "" : reader.GetString(reader.GetMethodDefinition(accessors.Adder).Name);
            string remove = accessors.Remover.IsNil ? "" : reader.GetString(reader.GetMethodDefinition(accessors.Remover).Name);
            string raise = accessors.Raiser.IsNil ? "" : reader.GetString(reader.GetMethodDefinition(accessors.Raiser).Name);
            string others = string.Join(',', accessors.Others.Select(handle => reader.GetString(reader.GetMethodDefinition(handle).Name)));
            rows.Add($"EVENT|{typeName}|{reader.GetString(eventDefinition.Name)}|{GetTypeHandleName(reader, eventDefinition.Type)}|{eventDefinition.Attributes}|{add}|{remove}|{raise}|{others}");
        }
    }
    return rows.OrderBy(row => row, StringComparer.Ordinal).ToArray();
}

static string GetTypeHandleName(MetadataReader reader, EntityHandle handle) => handle.Kind switch
{
    HandleKind.TypeDefinition => GetTypeDefinitionName(reader, (TypeDefinitionHandle)handle),
    HandleKind.TypeReference => GetTypeReferenceName(reader, (TypeReferenceHandle)handle),
    HandleKind.TypeSpecification => reader.GetTypeSpecification((TypeSpecificationHandle)handle)
        .DecodeSignature(new ManagedAssemblySignatureProvider(), null),
    _ => handle.Kind.ToString(),
};

static string GetTypeDefinitionName(MetadataReader reader, TypeDefinitionHandle handle)
{
    TypeDefinition type = reader.GetTypeDefinition(handle);
    string name = reader.GetString(type.Name);
    TypeDefinitionHandle declaringType = type.GetDeclaringType();
    if (!declaringType.IsNil)
    {
        return $"{GetTypeDefinitionName(reader, declaringType)}+{name}";
    }
    string ns = reader.GetString(type.Namespace);
    return ns.Length == 0 ? name : $"{ns}.{name}";
}

static string GetTypeReferenceName(MetadataReader reader, TypeReferenceHandle handle)
{
    TypeReference type = reader.GetTypeReference(handle);
    string name = reader.GetString(type.Name);
    if (type.ResolutionScope.Kind == HandleKind.TypeReference)
    {
        return $"{GetTypeReferenceName(reader, (TypeReferenceHandle)type.ResolutionScope)}+{name}";
    }
    string ns = reader.GetString(type.Namespace);
    return ns.Length == 0 ? name : $"{ns}.{name}";
}

sealed class ManagedAssemblySignatureProvider : ISignatureTypeProvider<string, object?>
{
    public string GetArrayType(string elementType, ArrayShape shape) =>
        $"{elementType}[{new string(',', Math.Max(0, shape.Rank - 1))}]";

    public string GetByReferenceType(string elementType) => $"{elementType}&";

    public string GetFunctionPointerType(MethodSignature<string> signature) =>
        $"methodptr {signature.ReturnType}({string.Join(',', signature.ParameterTypes)})";

    public string GetGenericInstantiation(string genericType, ImmutableArray<string> typeArguments) =>
        $"{genericType}<{string.Join(',', typeArguments)}>";

    public string GetGenericMethodParameter(object? genericContext, int index) => $"!!{index}";

    public string GetGenericTypeParameter(object? genericContext, int index) => $"!{index}";

    public string GetModifiedType(string modifier, string unmodifiedType, bool isRequired) =>
        $"{(isRequired ? "modreq" : "modopt")}({modifier}) {unmodifiedType}";

    public string GetPinnedType(string elementType) => $"{elementType} pinned";

    public string GetPointerType(string elementType) => $"{elementType}*";

    public string GetPrimitiveType(PrimitiveTypeCode typeCode) => typeCode.ToString();

    public string GetSZArrayType(string elementType) => $"{elementType}[]";

    public string GetTypeFromDefinition(MetadataReader reader, TypeDefinitionHandle handle, byte rawTypeKind) =>
        TypeDefinitionName(reader, handle);

    public string GetTypeFromReference(MetadataReader reader, TypeReferenceHandle handle, byte rawTypeKind) =>
        TypeReferenceName(reader, handle);

    public string GetTypeFromSpecification(
        MetadataReader reader,
        object? genericContext,
        TypeSpecificationHandle handle,
        byte rawTypeKind) => reader.GetTypeSpecification(handle).DecodeSignature(this, genericContext);

    private static string TypeDefinitionName(MetadataReader reader, TypeDefinitionHandle handle)
    {
        TypeDefinition type = reader.GetTypeDefinition(handle);
        string name = reader.GetString(type.Name);
        TypeDefinitionHandle declaringType = type.GetDeclaringType();
        if (!declaringType.IsNil)
        {
            return $"{TypeDefinitionName(reader, declaringType)}+{name}";
        }
        string ns = reader.GetString(type.Namespace);
        return ns.Length == 0 ? name : $"{ns}.{name}";
    }

    private static string TypeReferenceName(MetadataReader reader, TypeReferenceHandle handle)
    {
        TypeReference type = reader.GetTypeReference(handle);
        string name = reader.GetString(type.Name);
        if (type.ResolutionScope.Kind == HandleKind.TypeReference)
        {
            return $"{TypeReferenceName(reader, (TypeReferenceHandle)type.ResolutionScope)}+{name}";
        }
        string ns = reader.GetString(type.Namespace);
        return ns.Length == 0 ? name : $"{ns}.{name}";
    }
}
