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

internal static partial class ManagedProjectProbe
{
    internal static int ExportTextureProject(
    string inputRootValue,
    string outputRootValue,
    string rowsValue,
    string imageFormatName,
    string unityVersionText,
    string storageMode)
    {
        string inputRoot = Path.GetFullPath(inputRootValue);
        string outputRoot = Path.GetFullPath(outputRootValue);
        string rowsPath = Path.GetFullPath(rowsValue);
        EnsureEmptyOrMissingDirectory(inputRoot, "input");
        EnsureEmptyOrMissingDirectory(outputRoot, "output");
        if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
        {
            throw new IOException($"rows file must be new: {rowsPath}");
        }
        Directory.CreateDirectory(inputRoot);
        Directory.CreateDirectory(outputRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);
        if (storageMode is not ("inline" or "streamed"))
        {
            throw new ArgumentException($"unknown texture storage mode: {storageMode}");
        }

        UnityVersion version = UnityVersion.Parse(unityVersionText);
        GameBundle bundle = new();
        ProcessedAssetCollection collection = new(bundle);
        collection.GetType().GetProperty("Version")!.SetValue(collection, version);
        object texture = MakeGenerated(
        "AssetRipper.SourceGenerated.Classes.ClassID_28.Texture2D",
        28,
        44,
        collection,
        version);
        SetGeneratedProperty(texture, "Name", "TextureFixture");
        SetGeneratedProperty(texture, "Width_C28", 2);
        SetGeneratedProperty(texture, "Height_C28", 2);
        SetGeneratedProperty(texture, "ImageCount_C28", 1);
        SetGeneratedProperty(texture, "MipMap_C28", false);
        SetGeneratedProperty(texture, "CompleteImageSize_C28_Int32", 16);
        byte[] imageBytes = new byte[]
        {
10, 20, 30, 255,
40, 50, 60, 255,
70, 80, 90, 255,
100, 110, 120, 255,
        };
        string? streamPath = null;
        if (storageMode == "streamed")
        {
            PropertyInfo streamProperty = FindGeneratedProperty(texture, "StreamData_C28");
            object streamInfo = streamProperty.GetValue(texture)
            ?? throw new InvalidOperationException("generated Texture2D has no StreamingInfo instance");
            Console.WriteLine($"STREAM_INFO_TYPE|{streamInfo.GetType().FullName}");
            SetGeneratedProperty(streamInfo, "Path", "TextureFixture.resS");
            SetGeneratedProperty(streamInfo, "Size", imageBytes.Length);
            PropertyInfo? offsetProperty = null;
            for (Type? current = streamInfo.GetType(); current is not null && offsetProperty is null; current = current.BaseType)
            {
                offsetProperty = current.GetProperty(
                "Offset_UInt64",
                BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly);
                offsetProperty ??= current.GetProperty(
                "Offset_UInt32",
                BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly);
            }
            if (offsetProperty is null)
            {
                throw new InvalidOperationException("generated StreamingInfo has no writable offset property");
            }
            SetGeneratedProperty(streamInfo, offsetProperty.Name, 0);
            SetGeneratedProperty(texture, "ImageData_C28", Array.Empty<byte>());
            streamPath = Path.Combine(inputRoot, "TextureFixture.resS");
            File.WriteAllBytes(streamPath, imageBytes);
        }
        else
        {
            SetGeneratedProperty(texture, "ImageData_C28", imageBytes);
        }
        PropertyInfo formatProperty = texture.GetType().GetProperty("Format_C28E")
        ?? throw new InvalidOperationException("generated Texture2D has no Format_C28E property");
        formatProperty.SetValue(texture, Enum.ToObject(formatProperty.PropertyType, 4)); // RGBA32
        collection.GetType().GetMethod("AddAsset", BindingFlags.NonPublic | BindingFlags.Instance)!
        .Invoke(collection, new[] { texture });

        byte[] textureBody;
        using (MemoryStream bodyStream = new())
        using (AssetWriter writer = new(bodyStream, collection))
        {
            ((IAssetWritable)texture).WriteRelease(writer);
            textureBody = bodyStream.ToArray();
        }

        SerializedType textureType = new() { RawTypeID = 28 };
        SerializedFile serializedFile = new SerializedFileBuilder
        {
            Generation = FormatVersion.StoresTypeDependencies,
            Version = version,
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = false,
            Types = { textureType },
            Objects =
{
new ObjectInfo(textureType)
{
FileID = 44,
TypeID = 28,
SerializedTypeIndex = 0,
ObjectData = textureBody,
},
},
        }.Build();
        serializedFile.FilePath = "TextureFixture.assets";
        byte[] inputBytes = serializedFile.ToByteArray();
        string inputPath = Path.Combine(inputRoot, "TextureFixture.assets");
        File.WriteAllBytes(inputPath, inputBytes);

        FullConfiguration configuration = new();
        configuration.ExportSettings.ImageExportFormat = Enum.Parse<ImageExportFormat>(imageFormatName, false);
        configuration.ExportSettings.PreferOriginalTextureExtension = false;
        ExportHandler handler = new(configuration);
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false));
        rows.WriteLine("# Source-generated class-28 Texture2D through SpriteProcessor and ExportHandler.Export.");
        WriteInputRow(rows, inputPath, inputBytes);
        if (streamPath is not null)
        {
            WriteInputRow(rows, streamPath, imageBytes);
        }
        foreach (string outputPath in Directory.EnumerateFiles(outputRoot, "*", SearchOption.AllDirectories)
        .OrderBy(path => Path.GetRelativePath(outputRoot, path), StringComparer.Ordinal))
        {
            string relativePath = Path.GetRelativePath(outputRoot, outputPath).Replace('\\', '/');
            byte[] outputBytes = File.ReadAllBytes(outputPath);
            if (relativePath.EndsWith(".meta", StringComparison.Ordinal))
            {
                string normalized = NormalizeTextureProjectMeta(new UTF8Encoding(false, true).GetString(outputBytes));
                outputBytes = Encoding.UTF8.GetBytes(normalized);
            }
            else if (Path.GetExtension(relativePath) is ".unity" or ".asset" or ".json" or ".txt")
            {
                string normalized = NormalizeLightmapProjectText(new UTF8Encoding(false, true).GetString(outputBytes));
                outputBytes = Encoding.UTF8.GetBytes(normalized);
            }
            rows.WriteLine($"SOURCE_OUTPUT|{relativePath}|{Convert.ToHexString(outputBytes)}");
        }
        Console.WriteLine($"TEXTURE_PROJECT|input={inputBytes.Length}|storage={storageMode}|files={Directory.GetFiles(outputRoot, "*", SearchOption.AllDirectories).Length}|format={imageFormatName}");
        return 0;
    }

    internal static int DescribeLightmap()
    {
        UnityVersion version = UnityVersion.Parse("2019.4.40f1");
        GameBundle bundle = new();
        ProcessedAssetCollection collection = new(bundle);
        collection.GetType().GetProperty("Version")!.SetValue(collection, version);
        object lightmapSettings = MakeGenerated(
            "AssetRipper.SourceGenerated.Classes.ClassID_157.LightmapSettings",
            157,
            157,
            collection,
            version);
        MethodInfo hasLightingDataAsset = lightmapSettings.GetType().GetMethod(
            "Has_LightingDataAsset",
            BindingFlags.Public | BindingFlags.Instance)
            ?? throw new MissingMethodException(lightmapSettings.GetType().FullName, "Has_LightingDataAsset");
        Console.WriteLine($"TYPE|{lightmapSettings.GetType().FullName}");
        Console.WriteLine($"HAS_LIGHTING_DATA_ASSET|{hasLightingDataAsset.Invoke(lightmapSettings, null)}");
        foreach (PropertyInfo property in lightmapSettings.GetType().GetProperties(BindingFlags.Public | BindingFlags.Instance))
        {
            Console.WriteLine($"PROPERTY|{property.Name}|{property.PropertyType.FullName}");
        }
        object lightmaps = FindGeneratedProperty(lightmapSettings, "Lightmaps").GetValue(lightmapSettings)
            ?? throw new InvalidOperationException("LightmapSettings.Lightmaps is null");
        MethodInfo addNew = lightmaps.GetType().GetMethods(BindingFlags.Public | BindingFlags.Instance)
            .Single(method => method.Name == "AddNew" && method.GetParameters().Length == 0);
        object lightmapData = addNew.Invoke(lightmaps, null)
            ?? throw new InvalidOperationException("LightmapSettings.Lightmaps.AddNew returned null");
        foreach (PropertyInfo property in lightmapData.GetType().GetProperties(BindingFlags.Public | BindingFlags.Instance))
        {
            Console.WriteLine($"LIGHTMAP_PROPERTY|{property.Name}|{property.PropertyType.FullName}");
            object? value = property.GetValue(lightmapData);
            if (value is not null)
            {
                foreach (MethodInfo method in value.GetType().GetMethods(BindingFlags.Public | BindingFlags.Instance)
                    .Where(method => method.Name.Contains("Asset", StringComparison.Ordinal)
                        || method.Name.Contains("Pointer", StringComparison.Ordinal)
                        || method.Name.Contains("PathID", StringComparison.Ordinal)))
                {
                    Console.WriteLine($"LIGHTMAP_METHOD|{property.Name}|{method.Name}|{string.Join(",", method.GetParameters().Select(parameter => parameter.ParameterType.Name))}");
                }
            }
        }
        return 0;
    }

    internal static int DescribeLightmapVersions()
    {
        string[] versionTexts =
        [
            "3.5.0f1", "4.0.0f1", "4.7.2f1", "5.0.0f1", "5.3.0f1", "5.4.0f1", "5.5.0f1", "5.6.0f1",
        "2017.1.0f1", "2017.2.0f3", "2017.3.0f3", "2017.4.0f1", "2018.1.0f1", "2018.2.0f1",
        "2018.3.0f2", "2018.4.0f1", "2019.1.0f1", "2019.2.0f1", "2019.3.0f1", "2019.4.0f1",
        "2020.1.0f1", "2020.2.0f1", "2020.3.0f1", "2021.1.0f1", "2021.2.0f1", "2021.3.0f1",
        "2022.1.0f1", "2022.2.0f1", "2022.3.0f1", "2023.1.0f1", "2023.2.0f1", "6000.0.0f1",
    ];
        foreach (string versionText in versionTexts)
        {
            try
            {
                UnityVersion version = UnityVersion.Parse(versionText);
                GameBundle bundle = new();
                ProcessedAssetCollection collection = new(bundle);
                collection.GetType().GetProperty("Version")!.SetValue(collection, version);
                object asset = MakeGenerated(
                    "AssetRipper.SourceGenerated.Classes.ClassID_157.LightmapSettings",
                    157,
                    157,
                    collection,
                    version);
                MethodInfo hasField = asset.GetType().GetMethod("Has_LightingDataAsset", BindingFlags.Public | BindingFlags.Instance)
                    ?? throw new MissingMethodException(asset.GetType().FullName, "Has_LightingDataAsset");
                bool hasLightingDataAsset = hasField.Invoke(asset, null) is true;
                Console.WriteLine($"VERSION|{versionText}|{asset.GetType().Name}|{hasLightingDataAsset}");

                object lightingDataAsset = MakeGenerated(
                    "AssetRipper.SourceGenerated.Classes.ClassID_1120.LightingDataAsset",
                    1120,
                    1,
                    collection,
                    version);
                Console.WriteLine(
                    $"LDA_VERSION|{versionText}|{asset.GetType().Name}|{hasLightingDataAsset}|{lightingDataAsset.GetType().Name}|"
                    + $"{HasGeneratedField(lightingDataAsset, "Scene")}|"
                    + $"{HasGeneratedField(lightingDataAsset, "SceneGUID")}|"
                    + $"{HasGeneratedField(lightingDataAsset, "EnlightenDataVersion")}|"
                    + $"{HasGeneratedField(lightingDataAsset, "HideFlags")}|"
                    + $"{HasGeneratedField(lightingDataAsset, "CorrespondingSourceObject")}|"
                    + $"{HasGeneratedField(lightingDataAsset, "PrefabParentObject")}|"
                    + $"{HasGeneratedField(lightingDataAsset, "PrefabInternal")}|"
                    + $"{HasGeneratedField(lightingDataAsset, "PrefabInstance")}|"
                    + $"{HasGeneratedField(lightingDataAsset, "PrefabAsset")}|"
                    + $"{GeneratedPropertyType(lightingDataAsset, "Scene")}|"
                    + $"{GeneratedPropertyType(lightingDataAsset, "SceneGUID")}");
            }
            catch (Exception exception)
            {
                Console.WriteLine($"VERSION_ERROR|{versionText}|{exception.GetType().Name}|{exception.Message}");
            }
        }
        return 0;
    }

    internal static int ExportLightmapProject(
        string inputRootValue,
        string outputRootValue,
        string rowsValue,
        string lightmapFormatName,
        string imageFormatName,
        string unityVersionText)
    {
        string inputRoot = Path.GetFullPath(inputRootValue);
        string outputRoot = Path.GetFullPath(outputRootValue);
        string rowsPath = Path.GetFullPath(rowsValue);
        EnsureEmptyOrMissingDirectory(inputRoot, "input");
        EnsureEmptyOrMissingDirectory(outputRoot, "output");
        if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
        {
            throw new IOException($"rows file must be new: {rowsPath}");
        }
        Directory.CreateDirectory(inputRoot);
        Directory.CreateDirectory(outputRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);

        UnityVersion version = UnityVersion.Parse(unityVersionText);
        GameBundle bundle = new();
        ProcessedAssetCollection collection = new(bundle);
        collection.GetType().GetProperty("Version")!.SetValue(collection, version);

        object texture = MakeGenerated(
            "AssetRipper.SourceGenerated.Classes.ClassID_28.Texture2D",
            28,
            202,
            collection,
            version);
        SetGeneratedProperty(texture, "Name", "LightmapFixture");
        SetGeneratedProperty(texture, "Width_C28", 2);
        SetGeneratedProperty(texture, "Height_C28", 2);
        SetGeneratedProperty(texture, "ImageCount_C28", 1);
        SetGeneratedProperty(texture, "MipMap_C28", false);
        SetGeneratedProperty(texture, "CompleteImageSize_C28_Int32", 16);
        SetGeneratedProperty(texture, "Format_C28E", 4); // RGBA32
        SetGeneratedProperty(texture, "ImageData_C28", new byte[]
        {
        10, 20, 30, 255,
        40, 50, 60, 255,
        70, 80, 90, 255,
        100, 110, 120, 255,
        });

        object lightmapSettings = MakeGenerated(
            "AssetRipper.SourceGenerated.Classes.ClassID_157.LightmapSettings",
            157,
            101,
            collection,
            version);
        object lightmaps = FindGeneratedProperty(lightmapSettings, "Lightmaps").GetValue(lightmapSettings)
            ?? throw new InvalidOperationException("LightmapSettings.Lightmaps is null");
        MethodInfo addLightmap = lightmaps.GetType().GetMethods(BindingFlags.Public | BindingFlags.Instance)
            .Single(method => method.Name == "AddNew" && method.GetParameters().Length == 0);
        object lightmapData = addLightmap.Invoke(lightmaps, null)
            ?? throw new InvalidOperationException("LightmapSettings.Lightmaps.AddNew returned null");
        object lightmapPointer = FindGeneratedProperty(lightmapData, "Lightmap").GetValue(lightmapData)
            ?? throw new InvalidOperationException("LightmapData.Lightmap is null");
        MethodInfo setTexture = lightmapPointer.GetType().GetMethods(BindingFlags.Public | BindingFlags.Instance)
            .Single(method => method.Name == "SetAsset"
                && method.GetParameters().Length == 2
                && method.GetParameters()[1].ParameterType.IsInstanceOfType(texture));
        setTexture.Invoke(lightmapPointer, new[] { collection, texture });

        object renderSettings = MakeGenerated(
            "AssetRipper.SourceGenerated.Classes.ClassID_104.RenderSettings",
            104,
            102,
            collection,
            version);
        object[] assets = { lightmapSettings, renderSettings, texture };
        MethodInfo addAsset = collection.GetType().GetMethod("AddAsset", BindingFlags.NonPublic | BindingFlags.Instance)
            ?? throw new MissingMethodException(collection.GetType().FullName, "AddAsset");
        SerializedFileBuilder builder = new()
        {
            Generation = FormatVersion.StoresTypeDependencies,
            Version = version,
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = false,
        };
        foreach (object asset in assets)
        {
            int classId = (int)asset.GetType().GetProperty("ClassID")!.GetValue(asset)!;
            long pathId = (long)asset.GetType().GetProperty("PathID")!.GetValue(asset)!;
            addAsset.Invoke(collection, new[] { asset });
            byte[] body;
            using (MemoryStream bodyStream = new())
            using (AssetWriter writer = new(bodyStream, collection))
            {
                ((IAssetWritable)asset).WriteRelease(writer);
                body = bodyStream.ToArray();
            }
            SerializedType type = new() { RawTypeID = classId };
            int typeIndex = builder.Types.Count;
            builder.Types.Add(type);
            builder.Objects.Add(new ObjectInfo(type)
            {
                FileID = pathId,
                TypeID = classId,
                SerializedTypeIndex = typeIndex,
                ObjectData = body,
            });
        }

        SerializedFile serializedFile = builder.Build();
        serializedFile.FilePath = "LightmapFixture.assets";
        byte[] inputBytes = serializedFile.ToByteArray();
        string inputPath = Path.Combine(inputRoot, "LightmapFixture.assets");
        File.WriteAllBytes(inputPath, inputBytes);

        FullConfiguration configuration = new();
        configuration.ExportSettings.LightmapTextureExportFormat =
            Enum.Parse<LightmapTextureExportFormat>(lightmapFormatName, false);
        configuration.ExportSettings.ImageExportFormat = Enum.Parse<ImageExportFormat>(imageFormatName, false);
        configuration.ExportSettings.PreferOriginalTextureExtension = false;
        ExportHandler handler = new(configuration);
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine($"UNITY_VERSION|{unityVersionText}");
        rows.WriteLine($"SETTINGS|LightmapTextureExportFormat={lightmapFormatName}|ImageExportFormat={imageFormatName}|PreferOriginalTextureExtension=false");
        WriteInputRow(rows, inputPath, inputBytes);
        foreach (string outputPath in Directory.EnumerateFiles(outputRoot, "*", SearchOption.AllDirectories)
            .OrderBy(path => Path.GetRelativePath(outputRoot, path), StringComparer.Ordinal))
        {
            string relativePath = Path.GetRelativePath(outputRoot, outputPath).Replace('\\', '/');
            byte[] outputBytes = File.ReadAllBytes(outputPath);
            if (relativePath.EndsWith(".meta", StringComparison.Ordinal))
            {
                string normalized = NormalizeTextureProjectMeta(new UTF8Encoding(false, true).GetString(outputBytes));
                outputBytes = Encoding.UTF8.GetBytes(normalized);
            }
            else if (Path.GetExtension(relativePath) is ".unity" or ".asset" or ".json" or ".txt")
            {
                string normalized = NormalizeLightmapProjectText(new UTF8Encoding(false, true).GetString(outputBytes));
                outputBytes = Encoding.UTF8.GetBytes(normalized);
            }
            rows.WriteLine($"SOURCE_OUTPUT|{relativePath}|{Convert.ToHexString(outputBytes)}");
        }
        Console.WriteLine($"LIGHTMAP_PROJECT|input={inputBytes.Length}|version={unityVersionText}|files={Directory.GetFiles(outputRoot, "*", SearchOption.AllDirectories).Length}|format={lightmapFormatName}|image={imageFormatName}");
        return 0;
    }

    internal static int ExportPathIdMapProject(
        string inputRootValue,
        string outputRootValue,
        string rowsValue,
        string unityVersionText)
    {
        string inputRoot = Path.GetFullPath(inputRootValue);
        string outputRoot = Path.GetFullPath(outputRootValue);
        string rowsPath = Path.GetFullPath(rowsValue);
        EnsureEmptyOrMissingDirectory(inputRoot, "input");
        EnsureEmptyOrMissingDirectory(outputRoot, "output");
        if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
        {
            throw new IOException($"rows file must be new: {rowsPath}");
        }
        Directory.CreateDirectory(inputRoot);
        Directory.CreateDirectory(outputRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);

        UnityVersion version = UnityVersion.Parse(unityVersionText);
        GameBundle bundle = new();
        ProcessedAssetCollection collection = new(bundle);
        collection.GetType().GetProperty("Version")!.SetValue(collection, version);
        SerializedFileBuilder builder = new()
        {
            Generation = FormatVersion.StoresTypeDependencies,
            Version = version,
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = false,
        };

        void AddGeneratedAsset(
            string factoryName,
            int classId,
            long pathId,
            string assetName,
            Action<object>? configure = null)
        {
            object asset = MakeGenerated(
                $"AssetRipper.SourceGenerated.Classes.ClassID_{classId}.{factoryName}",
                classId,
                pathId,
                collection,
                version);
            SetGeneratedProperty(asset, "Name", assetName);
            configure?.Invoke(asset);
            byte[] body;
            using (MemoryStream bodyStream = new())
            using (AssetWriter writer = new(bodyStream, collection))
            {
                ((IAssetWritable)asset).WriteRelease(writer);
                body = bodyStream.ToArray();
            }

            SerializedType serializedType = new() { RawTypeID = classId };
            builder.Types.Add(serializedType);
            builder.Objects.Add(new ObjectInfo(serializedType)
            {
                FileID = pathId,
                TypeID = classId,
                SerializedTypeIndex = builder.Types.Count - 1,
                ObjectData = body,
            });
        }

        AddGeneratedAsset("Mesh", 43, 101, "MeshFixture");
        AddGeneratedAsset("Texture2D", 28, 202, "TextureFixture", texture =>
        {
            SetGeneratedProperty(texture, "Width_C28", 2);
            SetGeneratedProperty(texture, "Height_C28", 2);
            SetGeneratedProperty(texture, "ImageCount_C28", 1);
            SetGeneratedProperty(texture, "MipMap_C28", false);
            SetGeneratedProperty(texture, "CompleteImageSize_C28_Int32", 16);
            SetGeneratedProperty(texture, "Format_C28E", 4); // RGBA32
            SetGeneratedProperty(texture, "ImageData_C28", new byte[]
            {
            10, 20, 30, 255,
            40, 50, 60, 255,
            70, 80, 90, 255,
            100, 110, 120, 255,
            });
        });
        AddGeneratedAsset("AudioClip", 83, 303, "AudioFixture");
        AddGeneratedAsset("TextAsset", 49, 404, "TextFixture");

        SerializedFile serializedFile = builder.Build();
        serializedFile.FilePath = "PathMapFixture.assets";
        byte[] inputBytes = serializedFile.ToByteArray();
        string inputPath = Path.Combine(inputRoot, "PathMapFixture.assets");
        File.WriteAllBytes(inputPath, inputBytes);

        FullConfiguration configuration = new();
        ExportHandler handler = new(configuration);
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

        string pathIdMapPath = Path.Combine(outputRoot, "AuxiliaryFiles", "path_id_map.json");
        if (!File.Exists(pathIdMapPath))
        {
            throw new InvalidDataException("project export omitted AuxiliaryFiles/path_id_map.json");
        }
        byte[] pathIdMapBytes = File.ReadAllBytes(pathIdMapPath);
        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine($"UNITY_VERSION|{unityVersionText}");
        WriteInputRow(rows, inputPath, inputBytes);
        rows.WriteLine($"PATH_ID_MAP|{pathIdMapBytes.Length}|{Convert.ToHexString(SHA256.HashData(pathIdMapBytes))}");
        rows.WriteLine($"PATH_ID_MAP_HEX|{Convert.ToHexString(pathIdMapBytes)}");
        Console.WriteLine($"PATH_ID_MAP_PROJECT|input={inputBytes.Length}|version={unityVersionText}|map={pathIdMapBytes.Length}");
        return 0;
    }

    internal static int DumpTextureEnums()
    {
        Assembly generatedAssembly = Assembly.Load("AssetRipper.SourceGenerated");
        foreach (string typeName in new[]
        {
        "AssetRipper.SourceGenerated.Enums.TextureUsageMode",
        "AssetRipper.SourceGenerated.Enums.ColorSpace",
        "AssetRipper.SourceGenerated.Enums.TextureImporterGenerateCubemap",
        "AssetRipper.SourceGenerated.Enums.TextureImporterShape",
        "AssetRipper.SourceGenerated.Enums.TextureImporterType",
    })
        {
            Type? type = generatedAssembly.GetType(typeName);
            if (type is null || !type.IsEnum)
            {
                Console.WriteLine($"ENUM_MISSING|{typeName}");
                continue;
            }
            foreach (string name in Enum.GetNames(type))
            {
                object value = Enum.Parse(type, name);
                Console.WriteLine($"ENUM|{typeName}|{name}|{Convert.ToInt64(value)}");
            }
        }
        UnityVersion version = UnityVersion.Parse("2019.4.40f1");
        GameBundle bundle = new();
        ProcessedAssetCollection collection = new(bundle);
        collection.GetType().GetProperty("Version")!.SetValue(collection, version);
        object texture = MakeGenerated(
            "AssetRipper.SourceGenerated.Classes.ClassID_28.Texture2D",
            28,
            44,
            collection,
            version);
        PropertyInfo streamProperty = FindGeneratedProperty(texture, "StreamData_C28");
        Console.WriteLine($"PROPERTY|StreamData_C28|{streamProperty.PropertyType.FullName}");
        object? streamValue = streamProperty.GetValue(texture);
        if (streamValue is not null)
        {
            Console.WriteLine($"STREAM_VALUE_TYPE|{streamValue.GetType().FullName}|valueType={streamValue.GetType().IsValueType}");
        }
        foreach (PropertyInfo property in streamProperty.PropertyType.GetProperties(BindingFlags.Public | BindingFlags.Instance))
        {
            Console.WriteLine($"STREAM_PROPERTY|{property.Name}|{property.PropertyType.FullName}|{property.CanWrite}");
        }
        return 0;
    }
}
