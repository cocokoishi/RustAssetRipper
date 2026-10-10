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
using static ManagedProjectProbe;

if (args.Length > 0 && args[0] == "compare-managed-assembly-pe")
{
    if (args.Length != 3)
    {
        Console.Error.WriteLine("usage: managed-project compare-managed-assembly-pe <source-saved-dll> <rust-saved-dll>");
        return 2;
    }
    return CompareManagedAssemblyPe(args[1], args[2]);
}

if (args.Length > 0 && args[0] == "export-texture-project")
{
    if (args.Length is < 5 or > 7)
    {
        Console.Error.WriteLine("usage: managed-project export-texture-project <input-root> <output-root> <rows-file> <image-export-format> [unity-version] [inline|streamed]");
        return 2;
    }
    return ExportTextureProject(
        args[1],
        args[2],
        args[3],
        args[4],
        args.Length >= 6 ? args[5] : "2019.4.40f1",
        args.Length == 7 ? args[6] : "inline");
}

if (args.Length == 1 && args[0] == "describe-lightmap")
{
    return DescribeLightmap();
}

if (args.Length == 1 && args[0] == "describe-lighting-assets")
{
    return DescribeLightingAssets();
}

if (args.Length == 1 && args[0] == "describe-lightmap-versions")
{
    return DescribeLightmapVersions();
}

if (args.Length > 0 && args[0] == "export-lightmap-project")
{
    if (args.Length != 7)
    {
        Console.Error.WriteLine("usage: managed-project export-lightmap-project <input-root> <output-root> <rows-file> <lightmap-format> <image-format> <unity-version>");
        return 2;
    }
    return ExportLightmapProject(args[1], args[2], args[3], args[4], args[5], args[6]);
}

if (args.Length > 0 && args[0] == "export-lightmap-renderer-project")
{
    if (args.Length != 4)
    {
        Console.Error.WriteLine("usage: managed-project export-lightmap-renderer-project <input-root> <output-root> <rows-file>");
        return 2;
    }
    return ExportLightmapProject(args[1], args[2], args[3], "Image", "Bmp", "5.6.0f1", "mesh-renderer");
}

if (args.Length > 0 && args[0] == "export-lightmap-meshfilter-project")
{
    if (args.Length != 4)
    {
        Console.Error.WriteLine("usage: managed-project export-lightmap-meshfilter-project <input-root> <output-root> <rows-file>");
        return 2;
    }
    return ExportLightmapProject(args[1], args[2], args[3], "Image", "Bmp", "5.6.0f1", "mesh-filter-renderer");
}

if (args.Length > 0 && args[0] == "export-lightmap-light-project")
{
    if (args.Length != 4)
    {
        Console.Error.WriteLine("usage: managed-project export-lightmap-light-project <input-root> <output-root> <rows-file>");
        return 2;
    }
    return ExportLightmapProject(args[1], args[2], args[3], "Image", "Bmp", "5.6.0f1", "light");
}

if (args.Length > 0 && args[0] == "export-path-id-map-project")
{
    if (args.Length is < 4 or > 5)
    {
        Console.Error.WriteLine("usage: managed-project export-path-id-map-project <input-root> <output-root> <rows-file> [unity-version]");
        return 2;
    }
    return ExportPathIdMapProject(
        args[1],
        args[2],
        args[3],
        args.Length == 5 ? args[4] : "2019.4.40f1");
}

if (args.Length > 0 && args[0] == "export-audio-yaml-project")
{
    if (args.Length != 4)
    {
        Console.Error.WriteLine("usage: managed-project export-audio-yaml-project <input-root> <output-root> <rows-file>");
        return 2;
    }
    return ExportAudioYamlProject(args[1], args[2], args[3]);
}

if (args.Length == 1 && args[0] == "dump-texture-enums")
{
    return DumpTextureEnums();
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

if (args.Length > 0 && args[0] == "export-engine-resource-pointer")
{
    if (args.Length is < 4 or > 5)
    {
        Console.Error.WriteLine("usage: managed-project export-engine-resource-pointer <input-root> <output-root> <rows-file> [engine-file-name]");
        return 2;
    }
    return ExportEngineResourcePointer(
        args[1],
        args[2],
        args[3],
        args.Length == 5 ? args[4] : "unity default resources");
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

internal

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
