// Source-runtime integration probe for serialized managed-reference registries.
// It authors only synthetic SerializedFiles under the caller's scratch directory,
// then imports and exports those files through the frozen AssetRipper product path.
using System.Security.Cryptography;
using System.Text;
using AssetRipper.Export.Configuration;
using AssetRipper.Export.UnityProjects;
using AssetRipper.Import.Logging;
using AssetRipper.IO.Endian;
using AssetRipper.IO.Files;
using AssetRipper.IO.Files.SerializedFiles;
using AssetRipper.IO.Files.SerializedFiles.Parser;
using AssetRipper.IO.Files.SerializedFiles.Parser.TypeTrees;
using AssetRipper.Primitives;

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
