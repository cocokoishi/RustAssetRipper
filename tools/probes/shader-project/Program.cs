using AssetRipper.Assets;
using AssetRipper.Assets.Bundles;
using AssetRipper.Assets.Generics;
using AssetRipper.Export.Configuration;
using AssetRipper.Export.UnityProjects;
using AssetRipper.Import.Logging;
using AssetRipper.Import.Structure.Assembly.Managers;
using AssetRipper.IO.Files;
using AssetRipper.Primitives;
using AssetRipper.Processing;
using AssetRipper.SourceGenerated.Classes.ClassID_21;
using AssetRipper.SourceGenerated.Classes.ClassID_28;
using AssetRipper.SourceGenerated.Classes.ClassID_48;
using AssetRipper.SourceGenerated.Extensions;
using AssetRipper.SourceGenerated.Subclasses.ColorRGBAf;
using AssetRipper.SourceGenerated.Subclasses.UnityTexEnv;
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

if (args is ["--inspect-shader-2019"])
{
    Type shaderType = typeof(Shader_2019_3_0_b0);
    foreach (var property in shaderType.GetProperties().OrderBy(property => property.Name, StringComparer.Ordinal))
    {
        Console.WriteLine($"PROPERTY|{property.Name}|{property.PropertyType.FullName}|set={property.SetMethod is not null}");
    }
    return 0;
}

if (args is ["--inspect-texture-2019"])
{
    Type textureType = typeof(Texture2D_2019_4_9);
    foreach (var property in textureType.GetProperties().OrderBy(property => property.Name, StringComparer.Ordinal))
    {
        Console.WriteLine($"PROPERTY|{property.Name}|{property.PropertyType.FullName}|set={property.SetMethod is not null}");
    }
    return 0;
}

if (args is ["--inspect-material"])
{
    Assembly generatedAssembly = typeof(Shader_2022_3_20).Assembly;
    foreach (Type type in generatedAssembly.GetTypes()
        .Where(type => type.Namespace?.EndsWith("ClassID_21", StringComparison.Ordinal) == true
            && type.Name.StartsWith("Material_", StringComparison.Ordinal))
        .OrderBy(type => type.Name, StringComparer.Ordinal))
    {
        Console.WriteLine($"MATERIAL_TYPE|{type.FullName}");
        foreach (PropertyInfo property in type.GetProperties().OrderBy(property => property.Name, StringComparer.Ordinal))
        {
            Console.WriteLine($"MATERIAL_PROPERTY|{type.Name}|{property.Name}|{property.PropertyType.FullName}|set={property.SetMethod is not null}");
        }
    }
    foreach (Type type in generatedAssembly.GetTypes()
        .Where(type => type.Namespace?.Contains("UnityPropertySheet", StringComparison.Ordinal) == true
            || type.Namespace?.Contains("UnityTexEnv", StringComparison.Ordinal) == true)
        .OrderBy(type => type.FullName, StringComparer.Ordinal))
    {
        Console.WriteLine($"MATERIAL_SUBTYPE|{type.FullName}");
        foreach (PropertyInfo property in type.GetProperties().OrderBy(property => property.Name, StringComparer.Ordinal))
        {
            Console.WriteLine($"MATERIAL_SUBPROPERTY|{type.Name}|{property.Name}|{property.PropertyType.FullName}|set={property.SetMethod is not null}");
        }
    }
    return 0;
}

if (args.Length == 2
    && args[0] is "--export-material" or "--export-material-rich" or "--export-material-multi" or "--export-material-duplicate" or "--export-material-2019-4" or "--export-material-texture-ref" or "--export-material-2018-3" or "--export-material-2017-4")
{
    string materialOutputRoot = args[1];
    bool includeSavedProperties = args[0] is "--export-material-rich" or "--export-material-multi" or "--export-material-duplicate" or "--export-material-2019-4" or "--export-material-texture-ref" or "--export-material-2018-3" or "--export-material-2017-4";
    bool includeMultipleSavedProperties = args[0] == "--export-material-multi";
    bool includeDuplicateSavedProperties = args[0] == "--export-material-duplicate";
    bool includeTextureReference = args[0] == "--export-material-texture-ref";
    bool use2019_4 = args[0] is "--export-material-2019-4" or "--export-material-texture-ref";
    bool use2018_3 = args[0] == "--export-material-2018-3";
    bool use2017_4 = args[0] == "--export-material-2017-4";
    string materialOutputRootFull = Path.GetFullPath(materialOutputRoot);
    if (Directory.Exists(materialOutputRootFull) && Directory.EnumerateFileSystemEntries(materialOutputRootFull).Any())
    {
        Console.Error.WriteLine($"output root is not empty: {materialOutputRootFull}");
        return 2;
    }
    Logger.Add(new ConsoleLogger());
    string materialVersionText = use2019_4 ? "2019.4.40f1" : use2018_3 ? "2018.3.0f1" : use2017_4 ? "2017.4.40f1" : "2018.2.21f1";
    UnityVersion materialVersion = UnityVersion.Parse(materialVersionText);
    GameBundle materialBundle = new();
    var materialCollection = materialBundle.AddNewProcessedCollection("Material", materialVersion);
    IShader shader;
    IMaterial material;
    if (use2019_4)
    {
        Shader_2019_3_0_b0 shader2019_4 = materialCollection.CreateAsset(48, assetInfo => new Shader_2019_3_0_b0(assetInfo));
        shader2019_4.Name = new Utf8String("ShaderFixture");
        shader = shader2019_4;
        material = materialCollection.CreateAsset(21, assetInfo => new Material_2018_3(assetInfo));
    }
    else if (use2018_3)
    {
        Shader_2018_3 shader2018_3 = materialCollection.CreateAsset(48, assetInfo => new Shader_2018_3(assetInfo));
        shader2018_3.Name = new Utf8String("ShaderFixture");
        shader2018_3.Script = new Utf8String("Shader \"ShaderFixture\" { }\n");
        shader = shader2018_3;
        material = materialCollection.CreateAsset(21, assetInfo => new Material_2018_3(assetInfo));
    }
    else if (use2017_4)
    {
        Shader_2017_3 shader2017_4 = materialCollection.CreateAsset(48, assetInfo => new Shader_2017_3(assetInfo));
        shader2017_4.Name = new Utf8String("ShaderFixture");
        shader2017_4.Script = new Utf8String("Shader \"ShaderFixture\" { }\n");
        shader = shader2017_4;
        material = materialCollection.CreateAsset(21, assetInfo => new Material_2017_1_0_b9(assetInfo));
    }
    else
    {
        Shader_2018_2 shader2018_2 = materialCollection.CreateAsset(48, assetInfo => new Shader_2018_2(assetInfo));
        shader2018_2.Name = new Utf8String("ShaderFixture");
        shader2018_2.Script = new Utf8String("Shader \"ShaderFixture\" { }\n");
        shader = shader2018_2;
        material = materialCollection.CreateAsset(21, assetInfo => new Material_2018_2(assetInfo));
    }
    material.Name_C21 = new Utf8String("MaterialFixture");
    material.Shader_C21P = shader;
    material.CustomRenderQueue_C21 = 2450;
    material.LightmapFlags_C21 = 4;
    material.EnableInstancingVariants_C21 = true;
    material.DoubleSidedGI_C21 = true;
    material.AllowLocking_C21 = true;
    material.ShaderKeywords_C21_Utf8String = new Utf8String("_FEATURE_A _FEATURE_B");
    material.LockedProperties_C21 = new Utf8String("{\"renderQueue\":true}");
    material.ModifiedSerializedProperties_C21 = 3;
    var tag = material.StringTagMap_C21!.AddNew();
    tag.Key = new Utf8String("RenderType");
    tag.Value = new Utf8String("TransparentCutout");
    if (includeSavedProperties)
    {
        AssetPair<Utf8String, UnityTexEnv_5> texture =
            material.SavedProperties_C21.TexEnvs_AssetDictionary_Utf8String_UnityTexEnv_5!.AddNew();
        texture.Key = "_MainTex";
        texture.Value.Scale.X = 1.25f;
        texture.Value.Scale.Y = -2.5f;
        texture.Value.Offset.X = 0.125f;
        texture.Value.Offset.Y = 0.75f;
        if (includeTextureReference)
        {
            var textureCollection = materialBundle.AddNewProcessedCollection("Texture2D", materialVersion);
            Texture2D_2019_4_9 textureAsset = textureCollection.CreateAsset(
                28,
                assetInfo => new Texture2D_2019_4_9(assetInfo));
            textureAsset.Name = new Utf8String("TextureFixture");
            textureAsset.Width_C28 = 1;
            textureAsset.Height_C28 = 1;
            textureAsset.Format_C28 = 4;
            textureAsset.CompleteImageSize_C28_Int32 = 4;
            textureAsset.ImageCount_C28 = 1;
            textureAsset.ImageData_C28 = [255, 0, 0, 255];
            texture.Value.Texture.SetAsset(materialCollection, textureAsset);
        }
        if (includeMultipleSavedProperties || includeDuplicateSavedProperties)
        {
            AssetPair<Utf8String, UnityTexEnv_5> secondTexture =
                material.SavedProperties_C21.TexEnvs_AssetDictionary_Utf8String_UnityTexEnv_5!.AddNew();
            secondTexture.Key = includeDuplicateSavedProperties ? "_MainTex" : "_ATexture";
            secondTexture.Value.Scale.X = 2f;
            secondTexture.Value.Scale.Y = 0.5f;
            secondTexture.Value.Offset.X = -0.25f;
            secondTexture.Value.Offset.Y = 0.125f;
        }

        AssetPair<Utf8String, float> roughness =
            material.SavedProperties_C21.Floats_AssetDictionary_Utf8String_Single!.AddNew();
        roughness.Key = includeMultipleSavedProperties ? "_ZFloat" : "_Glossiness";
        roughness.Value = 0.375f;
        if (includeMultipleSavedProperties || includeDuplicateSavedProperties)
        {
            AssetPair<Utf8String, float> secondFloat =
                material.SavedProperties_C21.Floats_AssetDictionary_Utf8String_Single!.AddNew();
            secondFloat.Key = includeDuplicateSavedProperties ? "_Glossiness" : "_AFloat";
            secondFloat.Value = -0.125f;
        }

        AssetPair<Utf8String, ColorRGBAf> tint =
            material.SavedProperties_C21.Colors_AssetDictionary_Utf8String_ColorRGBAf!.AddNew();
        tint.Key = includeMultipleSavedProperties ? "_ZColor" : "_Color";
        tint.Value.SetValues(0.125f, 0.25f, 0.5f, 0.75f);
        if (includeMultipleSavedProperties || includeDuplicateSavedProperties)
        {
            AssetPair<Utf8String, ColorRGBAf> secondColor =
                material.SavedProperties_C21.Colors_AssetDictionary_Utf8String_ColorRGBAf!.AddNew();
            secondColor.Key = includeDuplicateSavedProperties ? "_Color" : "_AColor";
            secondColor.Value.SetValues(1f, 0.5f, 0.25f, 1f);
        }
    }
    GameData materialData = new(materialBundle, materialVersion, new BaseManager(_ => { }), null);
    FullConfiguration materialSettings = new();
    ExportHandler materialHandler = new(materialSettings);
    materialHandler.Export(materialData, materialOutputRootFull, LocalFileSystem.Instance);
    string materialFile = Directory.GetFiles(
            Path.Combine(materialOutputRootFull, "ExportedProject", "Assets"),
            "*.mat",
            SearchOption.AllDirectories)
        .Single();
    string normalized = File.ReadAllText(materialFile).Replace("\r\n", "\n", StringComparison.Ordinal);
    string shaderMetaFile = Directory.GetFiles(
            Path.Combine(materialOutputRootFull, "ExportedProject", "Assets"),
            "*.shader.meta",
            SearchOption.AllDirectories)
        .Single();
    string shaderGuid = File.ReadLines(shaderMetaFile)
        .Single(line => line.StartsWith("guid: ", StringComparison.Ordinal))[6..];
    normalized = normalized.Replace(shaderGuid, "<SHADER_GUID>", StringComparison.Ordinal);
    if (includeTextureReference)
    {
        string textureMetaFile = Directory.GetFiles(
                Path.Combine(materialOutputRootFull, "ExportedProject", "Assets"),
                "*.meta",
                SearchOption.AllDirectories)
            .Single(path => Path.GetFileName(path).StartsWith("TextureFixture.", StringComparison.Ordinal));
        string textureGuid = File.ReadLines(textureMetaFile)
            .Single(line => line.StartsWith("guid: ", StringComparison.Ordinal))[6..];
        normalized = normalized.Replace(textureGuid, "<TEXTURE_GUID>", StringComparison.Ordinal);
    }
    Console.WriteLine($"MATERIAL|{Convert.ToBase64String(System.Text.Encoding.UTF8.GetBytes(normalized))}");
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
