using AssetRipper.Assets;
using AssetRipper.Assets.Bundles;
using AssetRipper.Export.Configuration;
using AssetRipper.Export.UnityProjects;
using AssetRipper.Import.Logging;
using AssetRipper.Import.Structure.Assembly.Managers;
using AssetRipper.IO.Files;
using AssetRipper.Primitives;
using AssetRipper.Processing;
using AssetRipper.SourceGenerated.Classes.ClassID_48;
using System.Security.Cryptography;
using System.Reflection;

if (args is ["--inspect-shader"])
{
    Type shaderType = typeof(Shader_2022_3_20);
    foreach (var property in shaderType.GetProperties().OrderBy(property => property.Name, StringComparer.Ordinal))
    {
        Console.WriteLine($"PROPERTY|{property.Name}|{property.PropertyType.FullName}");
    }
    foreach (Type type in shaderType.Assembly.GetTypes()
        .Where(type => type.Namespace?.EndsWith("ClassID_48", StringComparison.Ordinal) == true
            && type.Name.StartsWith("Shader", StringComparison.Ordinal))
        .OrderBy(type => type.Name, StringComparer.Ordinal))
    {
        Console.WriteLine($"SHADER_TYPE|{type.FullName}");
    }
    foreach (Type type in shaderType.Assembly.GetTypes()
        .Where(type => type.Namespace?.EndsWith("ClassID_28", StringComparison.Ordinal) == true
            && type.Name.StartsWith("Texture2D", StringComparison.Ordinal))
        .OrderBy(type => type.Name, StringComparer.Ordinal))
    {
        Console.WriteLine($"TEXTURE_TYPE|{type.FullName}");
    }
    Type? pointerType = shaderType.Assembly.GetType("AssetRipper.SourceGenerated.Subclasses.PPtr_Texture.PPtr_Texture_5");
    if (pointerType is not null)
    {
        foreach (var property in pointerType.GetProperties().OrderBy(property => property.Name, StringComparer.Ordinal))
        {
            Console.WriteLine($"PPTR_PROPERTY|{property.Name}|{property.PropertyType.FullName}|set={property.SetMethod is not null}");
        }
        foreach (var method in pointerType.GetMethods().Where(method => method.Name.Contains("Value", StringComparison.Ordinal) || method.Name.Contains("Path", StringComparison.Ordinal) || method.Name.Contains("File", StringComparison.Ordinal)).OrderBy(method => method.Name, StringComparer.Ordinal))
        {
            Console.WriteLine($"PPTR_METHOD|{method}");
        }
        foreach (var field in pointerType.GetFields(BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic).OrderBy(field => field.Name, StringComparer.Ordinal))
        {
            Console.WriteLine($"PPTR_FIELD|{field.Name}|{field.FieldType.FullName}");
        }
    }
    return 0;
}

bool includeNonModifiableTexture = args.Length > 0 && args[0] == "--export-nonmod";
if ((!includeNonModifiableTexture && (args.Length is < 3 or > 4 || args[0] != "--export"))
    || (includeNonModifiableTexture && args.Length is < 2 or > 3))
{
    Console.Error.WriteLine("usage: shader-project --export <dummy|yaml|decompile> <new-or-empty-output-root> [5.0.0f1|2022.3.22f1] | --export-nonmod <new-or-empty-output-root> [2018.2.21f1|2022.3.22f1]");
    return 2;
}

ShaderExportMode mode = includeNonModifiableTexture ? ShaderExportMode.Dummy : args[1] switch
{
    "dummy" => ShaderExportMode.Dummy,
    "yaml" => ShaderExportMode.Yaml,
    "decompile" => ShaderExportMode.Decompile,
    _ => throw new ArgumentException($"unknown shader mode: {args[1]}"),
};
string outputRoot = Path.GetFullPath(includeNonModifiableTexture ? args[1] : args[2]);
string versionText = includeNonModifiableTexture
    ? args.Length == 3 ? args[2] : "2018.2.21f1"
    : args.Length == 4 ? args[3] : "5.0.0f1";
if (includeNonModifiableTexture && versionText is not ("2018.2.21f1" or "2022.3.22f1"))
{
    Console.Error.WriteLine("the non-modifiable texture probe requires a generated Shader class with the field");
    return 2;
}
if (Directory.Exists(outputRoot) && Directory.EnumerateFileSystemEntries(outputRoot).Any())
{
    Console.Error.WriteLine($"output root is not empty: {outputRoot}");
    return 2;
}

Logger.Add(new ConsoleLogger());
UnityVersion version = UnityVersion.Parse(versionText);
GameBundle bundle = new();
var collection = bundle.AddNewProcessedCollection("Shader", version);
string shaderScript = mode == ShaderExportMode.Yaml
    ? "Shader \"x\" {\nSubShader {\nProgram \"vp\" {}\n}\n}\n"
    : "Shader \"x\" { }\n";
if (versionText == "2022.3.22f1")
{
    Shader_2022_3_20 shader = collection.CreateAsset(48, assetInfo => new Shader_2022_3_20(assetInfo));
    shader.Name = "Test/Simple";
    if (includeNonModifiableTexture)
    {
        var pair = shader.NonModifiableTextures.AddNew();
        pair.Key = new Utf8String("_MainTex");
    }
}
else if (versionText == "2018.2.21f1")
{
    Shader_2018_2 shader = collection.CreateAsset(48, assetInfo => new Shader_2018_2(assetInfo));
    shader.Name = "Test/Simple";
    shader.Script = new Utf8String("Shader \"Test/Simple\" { }\n");
    if (includeNonModifiableTexture)
    {
        var pair = shader.NonModifiableTextures.AddNew();
        pair.Key = new Utf8String("_MainTex");
    }
}
else
{
    Shader_5 shader = collection.CreateAsset(48, assetInfo => new Shader_5(assetInfo));
    shader.Name = "Test/Simple";
    shader.Script = new Utf8String(shaderScript);
}
GameData gameData = new(bundle, version, new BaseManager(_ => { }), null);
FullConfiguration settings = new();
settings.ExportSettings.ShaderExportMode = mode;
ExportHandler handler = new(settings);
handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

string projectAssets = Path.Combine(outputRoot, "ExportedProject", "Assets");
foreach (string file in Directory.GetFiles(projectAssets, "*", SearchOption.AllDirectories)
    .Order(StringComparer.Ordinal))
{
    string relative = Path.GetRelativePath(outputRoot, file).Replace('\\', '/');
    byte[] bytes = File.ReadAllBytes(file);
    if (relative.EndsWith(".meta", StringComparison.Ordinal))
    {
        string normalized = string.Join('\n', File.ReadAllLines(file)
            .Where(line => !line.StartsWith("guid:", StringComparison.Ordinal)
                && !line.StartsWith("timeCreated:", StringComparison.Ordinal))) + "\n";
        Console.WriteLine($"META|{relative}|{normalized.Replace("\n", "\\n", StringComparison.Ordinal)}");
    }
    else if (relative.EndsWith(".cs", StringComparison.Ordinal))
    {
        Console.WriteLine($"SUPPORT|{relative}|{Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant()}");
    }
    else
    {
        Console.WriteLine($"ASSET|{relative}|{Convert.ToBase64String(bytes)}");
    }
}
return 0;
