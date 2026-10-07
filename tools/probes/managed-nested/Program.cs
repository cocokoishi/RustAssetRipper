using AssetRipper.Import.Structure.Assembly;
using AssetRipper.Import.Structure.Assembly.Serializable;
using AssetRipper.Import.Structure.Assembly.TypeTrees;
using AssetRipper.IO.Endian;
using AssetRipper.IO.Files.SerializedFiles.Parser.TypeTrees;
using AssetRipper.Primitives;
using AssetRipper.SerializationLogic;

var indexed = args.Contains("--legacy", StringComparer.OrdinalIgnoreCase);
var fixturePath = args.FirstOrDefault(File.Exists);
var rootTree = MakeTree([
    N("Root", "Root", 0),
    .. ReferenceNodes("m_Root", 1, indexed),
    .. RegistryNodes(1)]);
var nodeATree = MakeTree([
    N("NodeA", "NodeA", 0),
    .. ReferenceNodes("m_Link", 1, indexed),
    N("NestedState", "m_Nested", 1),
    .. ReferenceNodes("m_NestedLink", 2, indexed),
    .. RegistryNodes(2)]);
var nodeBTree = MakeTree([
    N("NodeB", "NodeB", 0),
    .. ReferenceNodes("m_Link", 1, indexed)]);

if (!TypeTreeNodeStruct.TryMakeFromTypeTree(rootTree, out TypeTreeNodeStruct rootNode))
{
    throw new InvalidDataException("The root TypeTree could not be converted.");
}

var structure = SerializableTreeType.FromRootNode(rootNode).CreateSerializableStructure();
var bytes = fixturePath is null ? BuildFixture(indexed) : LoadFixture(fixturePath);
var reader = new EndianSpanReader(bytes, EndianType.LittleEndian);
structure.Read(ref reader, new UnityVersion(6000), default, new FixtureResolver(nodeATree, nodeBTree));
if (reader.Position != reader.Length)
{
    throw new InvalidDataException($"The source reader consumed {reader.Position} of {reader.Length} bytes.");
}

Console.WriteLine($"source_consumed={reader.Position}/{reader.Length}");
Console.WriteLine($"fixture_hex={Convert.ToHexString(bytes)}");
Console.WriteLine($"root_fields={string.Join(',', structure.Type.Fields.Select(field => field.Name))}");
Console.WriteLine($"mode={(indexed ? "legacy-indexed" : "modern-rid")}");
Console.WriteLine("nested_registry=decoded");

static byte[] LoadFixture(string fixturePath)
{
    var line = File.ReadLines(fixturePath).FirstOrDefault(candidate => candidate.StartsWith("PAYLOAD|", StringComparison.Ordinal));
    if (line is null)
    {
        throw new InvalidDataException($"Fixture {fixturePath} has no PAYLOAD record.");
    }

    var fields = line.Split('|');
    if (fields.Length != 3)
    {
        throw new InvalidDataException($"Fixture record is malformed: {line}");
    }

    var bytes = Convert.FromHexString(fields[2]);
    if (!int.TryParse(fields[1], out var expectedLength) || bytes.Length != expectedLength)
    {
        throw new InvalidDataException($"Fixture length does not match its record: {line}");
    }

    return bytes;
}

static TypeTree MakeTree(IEnumerable<TypeTreeNode> nodes)
{
    var tree = new TypeTree();
    tree.Nodes.AddRange(nodes);
    return tree;
}

static TypeTreeNode N(string type, string name, int level, bool align = false) =>
    new(type, name, level, align);

static IEnumerable<TypeTreeNode> ReferenceNodes(string name, int level, bool indexed)
{
    yield return N("managedReference", name, level);
    yield return N(indexed ? "SInt32" : "SInt64", indexed ? "id" : "rid", level + 1);
}

static IEnumerable<TypeTreeNode> RegistryNodes(int level)
{
    yield return N("ManagedReferencesRegistry", "references", level);
    yield return N("int", "version", level + 1);
    yield return N("vector", "RefIds", level + 1);
    yield return N("Array", "Array", level + 2);
    yield return N("int", "size", level + 3);
    yield return N("ReferencedObjectData", "data", level + 3);
}

static byte[] BuildFixture(bool indexed)
{
    using var stream = new MemoryStream();
    using var writer = new BinaryWriter(stream);
    WriteReferenceId(writer, indexed); // Root.m_Root
    WriteRegistryHeader(writer, "NodeA", indexed);
    WriteReferenceId(writer, indexed); // NodeA.m_Link resolves in the root scope
    WriteReferenceId(writer, indexed); // NestedState.m_NestedLink resolves in its registry scope
    WriteRegistryHeader(writer, "NodeB", indexed);
    WriteReferenceId(writer, indexed); // NodeB.m_Link resolves in the nested scope
    if (indexed)
    {
        WriteTerminus(writer); // nested registry
        WriteTerminus(writer); // root registry
    }
    return stream.ToArray();
}

static void WriteRegistryHeader(BinaryWriter writer, string className, bool indexed)
{
    writer.Write(indexed ? 1 : 2);
    if (!indexed)
    {
        writer.Write(1); // one counted entry
        writer.Write(100L);
    }
    WriteRegistryString(writer, className);
    WriteRegistryString(writer, "Example");
    WriteRegistryString(writer, "Assembly-CSharp");
}

static void WriteReferenceId(BinaryWriter writer, bool indexed)
{
    if (indexed)
    {
        writer.Write(0);
    }
    else
    {
        writer.Write(100L);
    }
}

static void WriteTerminus(BinaryWriter writer)
{
    WriteRegistryString(writer, "Terminus");
    WriteRegistryString(writer, "UnityEngine.DMAT");
    WriteRegistryString(writer, "FAKE_ASM");
}

static void WriteRegistryString(BinaryWriter writer, string value)
{
    var bytes = System.Text.Encoding.UTF8.GetBytes(value);
    writer.Write(bytes.Length);
    writer.Write(bytes);
    while (writer.BaseStream.Position % 4 != 0)
    {
        writer.Write((byte)0);
    }
}

sealed class FixtureResolver(TypeTree nodeATree, TypeTree nodeBTree) : ITypeResolver
{
    public bool TryGetSerializableType(
        ScriptIdentifier scriptID,
        UnityVersion version,
        [System.Diagnostics.CodeAnalysis.NotNullWhen(true)]
        out SerializableType? scriptType,
        [System.Diagnostics.CodeAnalysis.NotNullWhen(false)]
        out string? failureReason)
    {
        var tree = scriptID.Name switch
        {
            "NodeA" => nodeATree,
            "NodeB" => nodeBTree,
            _ => null,
        };
        if (tree is not null && TypeTreeNodeStruct.TryMakeFromTypeTree(tree, out TypeTreeNodeStruct root))
        {
            scriptType = SerializableTreeType.FromReferencedObjectNode(root);
            failureReason = null;
            return true;
        }

        scriptType = null;
        failureReason = "The fixture resolver has no matching TypeTree.";
        return false;
    }
}
