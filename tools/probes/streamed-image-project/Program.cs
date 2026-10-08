// Runs the isolated AssetRipper project exporter on explicitly supplied inputs.
// The output root must be new or empty. All output remains under the caller's target directory.
using AssetRipper.Export.Configuration;
using AssetRipper.Export.UnityProjects;
using AssetRipper.Import.Logging;
using AssetRipper.Import.Structure.Assembly.Managers;
using AssetRipper.IO.Files;
using AssetRipper.IO.Files.ResourceFiles;
using AssetRipper.Primitives;
using AssetRipper.Processing;
using AssetRipper.Assets.Bundles;
using AssetRipper.Assets.Metadata;
using AssetRipper.SourceGenerated.Classes.ClassID_28;
using AssetRipper.SourceGenerated.Extensions;

bool useExternalResourceProvider = args.Length == 3 && args[0] == "--external";
if (!useExternalResourceProvider && args.Length != 1)
{
    Console.Error.WriteLine("usage: streamed-image-project <empty-output-root> | --external <empty-output-root> <resource-file>");
    return 2;
}

int outputArgumentIndex = useExternalResourceProvider ? 1 : 0;
string outputRoot = Path.GetFullPath(args[outputArgumentIndex]);
if (Directory.Exists(outputRoot) && Directory.EnumerateFileSystemEntries(outputRoot).Any())
{
    Console.Error.WriteLine($"output root is not empty: {outputRoot}");
    return 2;
}

Logger.Add(new ConsoleLogger());
UnityVersion version = UnityVersion.Parse("2022.3.22f1");
GameBundle bundle = new();
var collection = bundle.AddNewProcessedCollection("Texture2D", version);
Texture2D_2022_2_0_b3 texture = collection.CreateAsset(
    28,
    assetInfo => new Texture2D_2022_2_0_b3(assetInfo));
texture.Name = "Texture2D";
texture.StreamData_C189.SetValues(new Utf8String("texture.resS"), 2, 3);
ProbeResourceProvider? provider = null;
if (useExternalResourceProvider)
{
    provider = new ProbeResourceProvider(Path.GetFullPath(args[2]));
    bundle.ResourceProvider = provider;
}
else
{
    bundle.AddResource(new ResourceFile([0x30, 0x31, 0x32, 0x33, 0x34, 0x35], "fixture/texture.resS", "texture.resS"));
}
GameData gameData = new(bundle, version, new BaseManager(_ => { }), null);
Console.WriteLine($"SOURCE_OBJECT|class={texture.ClassID}|implements={texture is AssetRipper.SourceGenerated.Classes.ClassID_189.IImageTexture}");
FullConfiguration settings = new();
ExportHandler handler = new(settings);
handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

string projectAssets = Path.Combine(outputRoot, "ExportedProject", "Assets");
string exportedTexture = Path.Combine(projectAssets, "Texture2D", "Texture2D.texture2D");
if (!File.Exists(exportedTexture))
{
    throw new InvalidDataException($"AssetRipper did not emit the Texture2D YAML: {exportedTexture}");
}
string yaml = File.ReadAllText(exportedTexture);
if (!yaml.Contains("_typelessdata: 323334", StringComparison.Ordinal)
    || !yaml.Contains("    offset: 0", StringComparison.Ordinal)
    || !yaml.Contains("    size: 0", StringComparison.Ordinal)
    || !yaml.Contains("    path:\n", StringComparison.Ordinal))
{
    throw new InvalidDataException($"AssetRipper output did not materialize and clear the streamed texture data:\n{yaml}");
}
texture.StreamData_C189.GetValues(out Utf8String restoredPath, out ulong restoredOffset, out uint restoredSize);
if (restoredPath.ToString() != "texture.resS" || restoredOffset != 2 || restoredSize != 3 || texture.ImageData_C189.Length != 0)
{
    throw new InvalidDataException("AssetRipper export failed to restore the source Texture2D values after YAML serialization.");
}
if (provider is not null && (provider.FindCalls != 1 || provider.LastName != "texture.resS"))
{
    throw new InvalidDataException($"AssetRipper external provider calls differed: {provider.FindCalls}, {provider.LastName}");
}
Console.WriteLine("SOURCE_ASSERT|streamed bytes staged; stream cleared in YAML; source object restored after export");
if (provider is not null)
{
    Console.WriteLine($"SOURCE_PROVIDER|calls={provider.FindCalls}|name={provider.LastName}");
    Console.WriteLine("SOURCE_STREAMED_YAML|class=28|data=323334|offset=0|size=0|path_empty=true");
}
string[] files = Directory.Exists(projectAssets)
    ? Directory.GetFiles(projectAssets, "*", SearchOption.AllDirectories).Order(StringComparer.Ordinal).ToArray()
    : Array.Empty<string>();
foreach (string file in files)
{
    string relative = Path.GetRelativePath(outputRoot, file).Replace('\\', '/');
    Console.WriteLine($"OUTPUT|{relative}|{new FileInfo(file).Length}");
}
return 0;

sealed class ProbeResourceProvider(string resourcePath) : IResourceProvider
{
    public int FindCalls { get; private set; }
    public string? LastName { get; private set; }

    public ResourceFile? FindResource(string identifier)
    {
        FindCalls++;
        LastName = identifier;
        if (identifier != "texture.resS")
        {
            return null;
        }

        return new ResourceFile(resourcePath, identifier, LocalFileSystem.Instance);
    }
}
