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

if (args.Length is < 3 or > 4 || args[0] != "--export")
{
    Console.Error.WriteLine("usage: shader-project --export <dummy|yaml|decompile> <new-or-empty-output-root> [5.0.0f1|2022.3.22f1]");
    return 2;
}

ShaderExportMode mode = args[1] switch
{
    "dummy" => ShaderExportMode.Dummy,
    "yaml" => ShaderExportMode.Yaml,
    "decompile" => ShaderExportMode.Decompile,
    _ => throw new ArgumentException($"unknown shader mode: {args[1]}"),
};
string outputRoot = Path.GetFullPath(args[2]);
string versionText = args.Length == 4 ? args[3] : "5.0.0f1";
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
