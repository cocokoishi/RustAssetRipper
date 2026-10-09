// Source-runtime integration probe for serialized managed-reference registries.
// It authors only synthetic SerializedFiles under the caller's scratch directory,
// then imports and exports those files through the frozen AssetRipper product path.
using System.Security.Cryptography;
using System.Collections;
using System.Reflection;
using System.Text;
using AssetRipper.Export.Configuration;
using AssetRipper.Export.UnityProjects;
using AssetRipper.Export.UnityProjects.Scripts;
using AssetRipper.Import.Logging;
using AssetRipper.Import.Structure.Assembly.Managers;
using AssetRipper.IO.Endian;
using AssetRipper.IO.Files;
using AssetRipper.IO.Files.SerializedFiles;
using AssetRipper.IO.Files.SerializedFiles.Parser;
using AssetRipper.IO.Files.SerializedFiles.Parser.TypeTrees;
using AssetRipper.Primitives;

if (args.Length > 0 && args[0] == "export-mono-structure")
{
    if (args.Length != 4)
    {
        Console.Error.WriteLine("usage: managed-project export-mono-structure <input-root> <output-root> <rows-file>");
        return 2;
    }
    return ExportMonoStructure(args[1], args[2], args[3]);
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
    return DumpReferenceAssemblyMap(args[1]);
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

static byte[] BuildReferenceScriptFile()
{
    List<byte> scriptPayload = [];
    WriteString(scriptPayload, "Object"); // m_Name
    WriteI32(scriptPayload, 0); // m_ExecutionOrder
    scriptPayload.AddRange(new byte[16]); // Hash128 m_PropertiesHash
    WriteString(scriptPayload, "Object"); // m_ClassName
    WriteString(scriptPayload, "UnityEngine"); // m_Namespace
    WriteString(scriptPayload, "UnityEngine"); // m_AssemblyName

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
                ObjectData = scriptPayload.ToArray(),
            },
        },
    }.Build();
    file.FilePath = "reference_script.assets";
    return file.ToByteArray();
}

static byte[] BuildReferenceBehaviourFile()
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
        Version = UnityVersion.Parse("2019.4.40f1"),
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

static int DumpReferenceAssemblyMap(string outputPathArgument)
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
    writer.WriteLine("# AssetRipper ReferenceAssemblies.GetReferenceAssemblies with HasMscorlib2=false.");
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

static byte[] BuildTargetBehaviourFile()
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
    WriteString(payload, "ReferencedRoot");
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
