using AsmResolver.DotNet;
using AssetRipper.Import.Structure.Assembly;
using AssetRipper.Import.Structure.Assembly.Managers;
using AssetRipper.Import.Structure.Assembly.TypeTrees;
using AssetRipper.Import.Structure.Platforms;
using AssetRipper.IO.Files;
using AssetRipper.Primitives;
using AssetRipper.SerializationLogic;

if (args.Length != 1 && args.Length != 4)
{
    Console.Error.WriteLine("usage: managed-assembly <managed-assembly-path> [assembly-name namespace class-name]");
    return 2;
}

string path = Path.GetFullPath(args[0]);
string assemblyName = args.Length == 4 ? args[1] : "Assembly-CSharp";
string namespaceName = args.Length == 4 ? args[2] : "Game";
string className = args.Length == 4 ? args[3] : "HeroController";
byte[] assemblyBytes = File.ReadAllBytes(path);
string fileName = Path.GetFileName(path);
Console.WriteLine($"INPUT|{fileName}|{assemblyBytes.Length}|{Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(assemblyBytes))}");
var manager = new MonoManager(_ => { });
manager.Initialize(new ProbePlatformGameStructure(LocalFileSystem.Instance));
using MemoryStream stream = new(assemblyBytes, writable: false);
var loadedAssembly = manager.Read(stream, fileName);
Console.WriteLine($"ASSEMBLY|file={fileName}|manifest={loadedAssembly.Name}");

if (args.Length == 4)
{
    ScriptIdentifier customId = manager.GetScriptID(assemblyName, namespaceName, className);
    Console.WriteLine($"QUERY|{assemblyName}|{namespaceName}|{className}|{customId.UniqueName}|{manager.IsPresent(customId)}|{manager.IsValid(customId)}");
    TypeDefinition customType = manager.GetTypeDefinition(customId);
    foreach (FieldDefinition field in customType.Fields)
    {
        string typeName = field.Signature?.FieldType.FullName ?? "<missing>";
        string attributes = string.Join(",", field.CustomAttributes
            .Select(attribute => attribute.Constructor?.DeclaringType?.FullName)
            .Where(attribute => !string.IsNullOrEmpty(attribute)));
        Console.WriteLine($"FIELD|{field.Name}|{typeName}|{field.IsPublic}|{field.IsStatic}|{field.IsLiteral}|{field.IsNotSerialized}|{field.IsInitOnly}|{attributes}");
    }

    if (!manager.TryGetSerializableType(
            customId,
            UnityVersion.Parse("2019.4.40f1"),
            out SerializableType? customSchema,
            out string? customFailure))
    {
        throw new InvalidOperationException($"Could not build source FieldSerializer schema: {customFailure}");
    }
    DumpSchemaNode(TypeTreeNodeStruct.FromSerializableType(customSchema), 0);
    return 0;
}

foreach ((string assembly, string ns, string name) in new[]
{
    ("Assembly-CSharp", "Game", "HeroController"),
    ("FixtureScripts", "Game", "HeroController"),
    ("Assembly-CSharp", "Game", "AbstractController"),
    ("Assembly-CSharp", "game", "HeroController"),
    ("assembly-csharp", "Game", "HeroController"),
    ("Assembly-CSharp", "Game", "MissingController"),
})
{
    ScriptIdentifier id = manager.GetScriptID(assembly, ns, name);
    bool present = manager.IsPresent(id);
    bool valid = manager.IsValid(id);
    string resolved = id.IsDefault ? "<default>" : id.UniqueName;
    Console.WriteLine($"QUERY|{assembly}|{ns}|{name}|{resolved}|{present}|{valid}");
}

TypeDefinition hero = manager.GetTypeDefinition(manager.GetScriptID(
    "Assembly-CSharp",
    "Game",
    "HeroController"));
foreach (FieldDefinition field in hero.Fields)
{
    string typeName = field.Signature?.FieldType.FullName ?? "<missing>";
    string attributes = string.Join(",", field.CustomAttributes
        .Select(attribute => attribute.Constructor?.DeclaringType?.FullName)
        .Where(attribute => !string.IsNullOrEmpty(attribute)));
    Console.WriteLine($"FIELD|{field.Name}|{typeName}|{field.IsPublic}|{field.IsStatic}|{field.IsLiteral}|{field.IsNotSerialized}|{field.IsInitOnly}|{attributes}");
}

ScriptIdentifier heroId = manager.GetScriptID("Assembly-CSharp", "Game", "HeroController");
if (!manager.TryGetSerializableType(
        heroId,
        UnityVersion.Parse("2019.4.40f1"),
        out SerializableType? serializableType,
        out string? failureReason))
{
    throw new InvalidOperationException($"Could not build source FieldSerializer schema: {failureReason}");
}
DumpSchemaNode(TypeTreeNodeStruct.FromSerializableType(serializableType), 0);

return 0;

static void DumpSchemaNode(TypeTreeNodeStruct node, int level)
{
    Console.WriteLine($"SCHEMA_NODE|{level}|{node.TypeName}|{node.Name}|{node.Version}|{(int)node.MetaFlag}");
    foreach (TypeTreeNodeStruct child in node.SubNodes)
    {
        DumpSchemaNode(child, level + 1);
    }
}

sealed class ProbePlatformGameStructure : PlatformGameStructure
{
    public ProbePlatformGameStructure(FileSystem fileSystem) : base(fileSystem) { }
}
