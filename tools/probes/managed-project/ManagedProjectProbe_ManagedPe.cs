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
    internal static int ExportAudioYamlProject(string inputRootValue, string outputRootValue, string rowsValue)
    {
        const string unityVersionText = "2019.4.40f1";
        const string audioName = "AudioFixture";
        const string streamName = "AudioFixture.resS";
        const ulong streamOffset = 6;
        const ulong streamSize = 3;
        byte[] resourceBytes = "prefixABCsuffix"u8.ToArray();

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
        object audio = MakeGenerated(
        "AssetRipper.SourceGenerated.Classes.ClassID_83.AudioClip",
        83,
        303,
        collection,
        version);
        SetGeneratedProperty(audio, "Name", audioName);
        object resource = FindGeneratedProperty(audio, "Resource").GetValue(audio)
        ?? throw new InvalidOperationException("generated AudioClip has no Resource instance");
        SetGeneratedProperty(resource, "Source", streamName);
        SetGeneratedProperty(resource, "Offset", checked((int)streamOffset));
        SetGeneratedProperty(resource, "Size", checked((int)streamSize));

        byte[] audioBody;
        using (MemoryStream bodyStream = new())
        using (AssetWriter writer = new(bodyStream, collection))
        {
            ((IAssetWritable)audio).WriteRelease(writer);
            audioBody = bodyStream.ToArray();
        }
        SerializedType serializedType = new() { RawTypeID = 83 };
        SerializedFile serializedFile = new SerializedFileBuilder
        {
            Generation = FormatVersion.StoresTypeDependencies,
            Version = version,
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = false,
            Types = { serializedType },
            Objects =
{
new ObjectInfo(serializedType)
{
FileID = 303,
TypeID = 83,
SerializedTypeIndex = 0,
ObjectData = audioBody,
},
},
        }.Build();
        serializedFile.FilePath = "AudioFixture.assets";
        byte[] inputBytes = serializedFile.ToByteArray();
        string inputPath = Path.Combine(inputRoot, "AudioFixture.assets");
        string resourcePath = Path.Combine(inputRoot, streamName);
        File.WriteAllBytes(inputPath, inputBytes);
        File.WriteAllBytes(resourcePath, resourceBytes);

        FullConfiguration configuration = new();
        configuration.ExportSettings.AudioExportFormat = AudioExportFormat.Yaml;
        ExportHandler handler = new(configuration);
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
        rows.WriteLine($"UNITY_VERSION|{unityVersionText}");
        WriteInputRow(rows, inputPath, inputBytes);
        WriteInputRow(rows, resourcePath, resourceBytes);
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
            rows.WriteLine($"SOURCE_OUTPUT|{relativePath}|{Convert.ToHexString(outputBytes)}");
        }
        Console.WriteLine($"AUDIO_YAML_PROJECT|input={inputBytes.Length}|resource={resourceBytes.Length}|files={Directory.GetFiles(outputRoot, "*", SearchOption.AllDirectories).Length}");
        return 0;
    }

    internal static object MakeGenerated(string factory, int classId, long pathId, AssetCollection collection, UnityVersion version)
    {
        Assembly generatedAssembly = Assembly.Load("AssetRipper.SourceGenerated");
        Type factoryType = generatedAssembly.GetType(factory)
            ?? throw new InvalidOperationException($"generated factory type was not found: {factory}");
        MethodInfo? create = factoryType.GetMethod("Create", new[] { typeof(AssetInfo), typeof(UnityVersion) })
            ?? factoryType.GetMethod("Create", new[] { typeof(AssetInfo) });
        if (create is null)
        {
            throw new InvalidOperationException($"generated factory has no Create method: {factory}");
        }
        object assetInfo = new AssetInfo(collection, pathId, classId);
        object[] arguments = create.GetParameters().Length == 2
            ? new[] { assetInfo, (object)version }
            : new[] { assetInfo };
        return create.Invoke(null, arguments)
            ?? throw new InvalidOperationException($"generated factory returned null: {factory}");
    }

    internal static void SetGeneratedProperty(object asset, string propertyName, object value)
    {
        PropertyInfo property = FindGeneratedProperty(asset, propertyName);
        if (!property.CanWrite)
        {
            throw new InvalidOperationException($"generated asset property is not writable: {propertyName}");
        }
        if (value is int integer)
        {
            if (property.PropertyType.IsEnum)
            {
                value = Enum.ToObject(property.PropertyType, integer);
            }
            else if (property.PropertyType == typeof(uint))
            {
                value = (uint)integer;
            }
            else if (property.PropertyType == typeof(long))
            {
                value = (long)integer;
            }
            else if (property.PropertyType == typeof(ulong))
            {
                value = (ulong)integer;
            }
        }
        else if (value is string stringValue && property.PropertyType.Name == "Utf8String")
        {
            value = Activator.CreateInstance(property.PropertyType, stringValue)
                ?? throw new InvalidOperationException($"could not create Utf8String for {propertyName}");
        }
        property.SetValue(asset, value);
    }

    internal static PropertyInfo FindGeneratedProperty(object asset, string propertyName)
    {
        Type type = asset.GetType();
        for (Type? current = type; current is not null; current = current.BaseType)
        {
            PropertyInfo? property = current.GetProperty(
                propertyName,
                BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly);
            if (property is not null)
            {
                return property;
            }
        }
        throw new InvalidOperationException($"generated asset has no {propertyName} property");
    }

    internal static bool HasGeneratedField(object asset, string propertyName)
    {
        MethodInfo? method = asset.GetType().GetMethod(
            $"Has_{propertyName}",
            BindingFlags.Public | BindingFlags.Instance);
        return method?.Invoke(asset, null) is true;
    }

    internal static string GeneratedPropertyType(object asset, string propertyName)
    {
        try
        {
            return FindGeneratedProperty(asset, propertyName).PropertyType.FullName ?? "<no-full-name>";
        }
        catch (InvalidOperationException)
        {
            return "<absent>";
        }
    }

    internal static string NormalizeTextureProjectMeta(string text)
    {
        text = text.Replace("\r\n", "\n", StringComparison.Ordinal);
        text = Regex.Replace(text, "(?m)^guid: [0-9a-fA-F]{32}$", "guid: <GUID>");
        return Regex.Replace(text, "(?m)^timeCreated: [0-9]+$", "timeCreated: <TIME>");
    }

    internal static string NormalizeLightmapProjectText(string text)
    {
        text = text.Replace("\r\n", "\n", StringComparison.Ordinal).Replace('\r', '\n');
        text = Regex.Replace(text, "(?i)\\b[0-9a-f]{32}\\b", "<GUID>");
        return Regex.Replace(text, "(?m)^timeCreated: [0-9]+$", "timeCreated: <TIME>");
    }

    internal static byte[] BuildManagedRootFile(
        string targetFileName = "target.assets",
        bool arrayAssetPointer = false)
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
        if (arrayAssetPointer)
        {
            WriteI32(payload, 2); // NodeA.m_Assets array size
            WriteI32(payload, 1); // NodeA.m_Assets[0].m_FileID -> missing.assets
            WriteI64(payload, 900); // NodeA.m_Assets[0].m_PathID
            WriteI32(payload, 1); // NodeA.m_Assets[1].m_FileID -> missing.assets
            WriteI64(payload, 901); // NodeA.m_Assets[1].m_PathID
        }
        else
        {
            WriteI32(payload, 1);   // NodeA.m_Asset.m_FileID -> targetFileName
            WriteI64(payload, 900); // NodeA.m_Asset.m_PathID
        }
        WriteI64(payload, 200);
        WriteManagedType(payload, "NodeB");
        WriteI32(payload, 456); // NodeB.m_Value
        WriteI64(payload, 100); // NodeB.m_Link -> NodeA (cycle)
        WriteI32(payload, 0);   // NodeB.m_Asset.m_FileID
        WriteI64(payload, 0);   // NodeB.m_Asset.m_PathID

        SerializedType monoBehaviourType = new() { RawTypeID = 114 };
        AssignTree(monoBehaviourType, MonoBehaviourTree());
        SerializedTypeReference nodeAType = MakeReferenceType(
            "NodeA",
            arrayAssetPointer ? NodeArrayTree("NodeA") : NodeTree("NodeA"));
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
                AssetPath = targetFileName,
                PathNameOrigin = targetFileName,
                PathName = targetFileName,
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

    internal static byte[] BuildTargetBehaviourFile(
        string name = "ReferencedRoot",
        int health = 789,
        string fileName = "target.assets")
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
        WriteString(payload, name);
        WriteI32(payload, health); // m_Health
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
        file.FilePath = fileName;
        return file.ToByteArray();
    }

    internal static TypeTree MonoBehaviourTargetTree()
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

    internal static TypeTree MonoBehaviourTree()
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

    internal static TypeTree NodeTree(string name)
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

    internal static TypeTree NodeArrayTree(string name)
    {
        return MakeTree(
        [
            (name, name, 0, -1, 0),
        ("int", "m_Value", 1, 4, 0),
        ("managedReference", "m_Link", 1, -1, 0),
        ("SInt64", "rid", 2, 8, 0),
        ("vector", "m_Assets", 1, -1, 0),
        ("Array", "Array", 2, -1, 0),
        ("int", "size", 3, 4, 0),
        ("PPtr<$Object>", "data", 3, 12, 0),
        ("SInt32", "m_FileID", 4, 4, 0),
        ("SInt64", "m_PathID", 4, 8, 0),
    ]);
    }

    internal static SerializedTypeReference MakeReferenceType(string className, TypeTree tree)
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

    internal static void AssignTree(SerializedType type, TypeTree tree)
    {
        type.OldType.Nodes.AddRange(tree.Nodes);
        type.OldType.StringBuffer = tree.StringBuffer;
    }

    internal static void AssignReferenceTree(SerializedTypeReference type, TypeTree tree)
    {
        type.OldType.Nodes.AddRange(tree.Nodes);
        type.OldType.StringBuffer = tree.StringBuffer;
    }

    internal static TypeTree MakeTree((string Type, string Name, int Level, int ByteSize, int Meta)[] specs)
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

    internal static void WriteManagedType(List<byte> payload, string className)
    {
        WriteString(payload, className);
        WriteString(payload, "Example");
        WriteString(payload, "Assembly-CSharp");
    }

    internal static void WriteString(List<byte> payload, string value)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(value);
        WriteI32(payload, bytes.Length);
        payload.AddRange(bytes);
        while (payload.Count % 4 != 0)
        {
            payload.Add(0);
        }
    }

    internal static void WriteI32(List<byte> payload, int value) => payload.AddRange(BitConverter.GetBytes(value));

    internal static void WriteI64(List<byte> payload, long value) => payload.AddRange(BitConverter.GetBytes(value));

    internal static int CompareManagedAssemblyPe(string sourcePath, string rustPath)
    {
        string[] sourceRows = ReadManagedAssemblyPeRows(sourcePath);
        string[] rustRows = ReadManagedAssemblyPeRows(rustPath);
        Console.WriteLine($"SOURCE_ROWS|{sourceRows.Length}");
        Console.WriteLine($"RUST_ROWS|{rustRows.Length}");
        int commonCount = Math.Min(sourceRows.Length, rustRows.Length);
        bool matches = true;
        for (int index = 0; index < commonCount; index++)
        {
            if (!string.Equals(sourceRows[index], rustRows[index], StringComparison.Ordinal))
            {
                Console.Error.WriteLine($"MISMATCH|{index}|SOURCE|{sourceRows[index]}|RUST|{rustRows[index]}");
                matches = false;
            }
        }
        if (sourceRows.Length != rustRows.Length)
        {
            Console.Error.WriteLine($"ROW_COUNT_MISMATCH|SOURCE|{sourceRows.Length}|RUST|{rustRows.Length}");
            matches = false;
        }
        if (!matches) return 1;
        Console.WriteLine("MANAGED_ASSEMBLY_PE_MODEL_MATCH");
        return 0;
    }

    internal static string[] ReadManagedAssemblyPeRows(string path)
    {
        using var stream = File.OpenRead(path);
        using var peReader = new PEReader(stream);
        if (!peReader.HasMetadata)
        {
            throw new BadImageFormatException($"PE file has no CLR metadata: {path}");
        }
        MetadataReader reader = peReader.GetMetadataReader();
        var signatureProvider = new ManagedAssemblySignatureProvider();
        var rows = new List<string>();
        ModuleDefinition module = reader.GetModuleDefinition();
        rows.Add($"MODULE|{reader.GetString(module.Name)}|{reader.GetGuid(module.Mvid)}");
        rows.Add($"MACHINE|{peReader.PEHeaders.CoffHeader.Machine}");
        if (reader.IsAssembly)
        {
            System.Reflection.Metadata.AssemblyDefinition assembly = reader.GetAssemblyDefinition();
            string publicKey = assembly.PublicKey.IsNil ? "" : Convert.ToHexString(reader.GetBlobBytes(assembly.PublicKey));
            rows.Add($"ASSEMBLY|{reader.GetString(assembly.Name)}|{assembly.Version}|{assembly.Flags}|{publicKey}");
        }
        foreach (AssemblyReferenceHandle handle in reader.AssemblyReferences)
        {
            AssemblyReference reference = reader.GetAssemblyReference(handle);
            string publicKeyOrToken = reference.PublicKeyOrToken.IsNil
                ? ""
                : Convert.ToHexString(reader.GetBlobBytes(reference.PublicKeyOrToken));
            string culture = reference.Culture.IsNil ? "" : reader.GetString(reference.Culture);
            rows.Add($"REFERENCE|{reader.GetString(reference.Name)}|{reference.Version}|{reference.Flags}|{culture}|{publicKeyOrToken}");
        }
        foreach (ExportedTypeHandle handle in reader.ExportedTypes)
        {
            ExportedType exported = reader.GetExportedType(handle);
            string namespaceName = exported.Namespace.IsNil ? "" : reader.GetString(exported.Namespace);
            string implementation = exported.Implementation.Kind switch
            {
                HandleKind.AssemblyReference => $"ASSEMBLY:{reader.GetString(reader.GetAssemblyReference((AssemblyReferenceHandle)exported.Implementation).Name)}",
                HandleKind.ExportedType => $"EXPORTED:{reader.GetString(reader.GetExportedType((ExportedTypeHandle)exported.Implementation).Name)}",
                HandleKind.AssemblyFile => $"FILE:{reader.GetString(reader.GetAssemblyFile((AssemblyFileHandle)exported.Implementation).Name)}",
                _ => exported.Implementation.Kind.ToString(),
            };
            rows.Add($"EXPORTED_TYPE|{namespaceName}|{reader.GetString(exported.Name)}|{exported.Attributes}|{implementation}");
        }
        rows = rows.OrderBy(row => row, StringComparer.Ordinal).ToList();
        foreach (TypeDefinitionHandle typeHandle in reader.TypeDefinitions)
        {
            TypeDefinition type = reader.GetTypeDefinition(typeHandle);
            string typeName = GetTypeDefinitionName(reader, typeHandle);
            string baseType = type.BaseType.IsNil ? "" : GetTypeHandleName(reader, type.BaseType);
            rows.Add($"TYPE|{typeName}|{baseType}|{type.Attributes}");
            foreach (FieldDefinitionHandle fieldHandle in type.GetFields()
                .OrderBy(handle => reader.GetString(reader.GetFieldDefinition(handle).Name), StringComparer.Ordinal))
            {
                FieldDefinition field = reader.GetFieldDefinition(fieldHandle);
                string fieldType = field.DecodeSignature(signatureProvider, null);
                rows.Add($"FIELD|{typeName}|{reader.GetString(field.Name)}|{fieldType}|{field.Attributes}");
            }
            foreach (MethodDefinitionHandle methodHandle in type.GetMethods()
                .OrderBy(handle => reader.GetString(reader.GetMethodDefinition(handle).Name), StringComparer.Ordinal))
            {
                MethodDefinition method = reader.GetMethodDefinition(methodHandle);
                var signature = method.DecodeSignature(signatureProvider, null);
                string parameters = string.Join(',', signature.ParameterTypes);
                string body = "no-body";
                if (method.RelativeVirtualAddress != 0)
                {
                    MethodBodyBlock methodBody = peReader.GetMethodBody(method.RelativeVirtualAddress);
                    int ilLength = methodBody.GetILBytes()?.Length ?? 0;
                    string locals = methodBody.LocalSignature.IsNil
                        ? ""
                        : string.Join(',', reader.GetStandaloneSignature(methodBody.LocalSignature)
                            .DecodeLocalSignature(signatureProvider, null));
                    string exceptions = string.Join(';', methodBody.ExceptionRegions.Select(region =>
                        $"{region.Kind}:{region.TryOffset}+{region.TryLength}:{region.HandlerOffset}+{region.HandlerLength}:{region.FilterOffset}:{(region.CatchType.IsNil ? "" : GetTypeHandleName(reader, region.CatchType))}"));
                    body = $"il={ilLength};maxStack={methodBody.MaxStack};initLocals={methodBody.LocalVariablesInitialized};locals={locals};exceptions={exceptions}";
                }
                string methodName = reader.GetString(method.Name);
                rows.Add($"METHOD|{typeName}|{methodName}|{signature.ReturnType}({parameters})|{method.Attributes}|0x{((int)method.ImplAttributes):X4}|{body}");
            }
            foreach (PropertyDefinitionHandle propertyHandle in type.GetProperties()
                .OrderBy(handle => reader.GetString(reader.GetPropertyDefinition(handle).Name), StringComparer.Ordinal))
            {
                PropertyDefinition property = reader.GetPropertyDefinition(propertyHandle);
                var signature = property.DecodeSignature(signatureProvider, null);
                PropertyAccessors accessors = property.GetAccessors();
                string getter = accessors.Getter.IsNil ? "" : reader.GetString(reader.GetMethodDefinition(accessors.Getter).Name);
                string setter = accessors.Setter.IsNil ? "" : reader.GetString(reader.GetMethodDefinition(accessors.Setter).Name);
                string others = string.Join(',', accessors.Others.Select(handle => reader.GetString(reader.GetMethodDefinition(handle).Name)));
                rows.Add($"PROPERTY|{typeName}|{reader.GetString(property.Name)}|{signature.ReturnType}({string.Join(',', signature.ParameterTypes)})|{property.Attributes}|{getter}|{setter}|{others}");
            }
            foreach (EventDefinitionHandle eventHandle in type.GetEvents()
                .OrderBy(handle => reader.GetString(reader.GetEventDefinition(handle).Name), StringComparer.Ordinal))
            {
                EventDefinition eventDefinition = reader.GetEventDefinition(eventHandle);
                EventAccessors accessors = eventDefinition.GetAccessors();
                string add = accessors.Adder.IsNil ? "" : reader.GetString(reader.GetMethodDefinition(accessors.Adder).Name);
                string remove = accessors.Remover.IsNil ? "" : reader.GetString(reader.GetMethodDefinition(accessors.Remover).Name);
                string raise = accessors.Raiser.IsNil ? "" : reader.GetString(reader.GetMethodDefinition(accessors.Raiser).Name);
                string others = string.Join(',', accessors.Others.Select(handle => reader.GetString(reader.GetMethodDefinition(handle).Name)));
                rows.Add($"EVENT|{typeName}|{reader.GetString(eventDefinition.Name)}|{GetTypeHandleName(reader, eventDefinition.Type)}|{eventDefinition.Attributes}|{add}|{remove}|{raise}|{others}");
            }
        }
        return rows.OrderBy(row => row, StringComparer.Ordinal).ToArray();
    }

    internal static string GetTypeHandleName(MetadataReader reader, EntityHandle handle) => handle.Kind switch
    {
        HandleKind.TypeDefinition => GetTypeDefinitionName(reader, (TypeDefinitionHandle)handle),
        HandleKind.TypeReference => GetTypeReferenceName(reader, (TypeReferenceHandle)handle),
        HandleKind.TypeSpecification => reader.GetTypeSpecification((TypeSpecificationHandle)handle)
            .DecodeSignature(new ManagedAssemblySignatureProvider(), null),
        _ => handle.Kind.ToString(),
    };

    internal static string GetTypeDefinitionName(MetadataReader reader, TypeDefinitionHandle handle)
    {
        TypeDefinition type = reader.GetTypeDefinition(handle);
        string name = reader.GetString(type.Name);
        TypeDefinitionHandle declaringType = type.GetDeclaringType();
        if (!declaringType.IsNil)
        {
            return $"{GetTypeDefinitionName(reader, declaringType)}+{name}";
        }
        string ns = reader.GetString(type.Namespace);
        return ns.Length == 0 ? name : $"{ns}.{name}";
    }

    internal static string GetTypeReferenceName(MetadataReader reader, TypeReferenceHandle handle)
    {
        TypeReference type = reader.GetTypeReference(handle);
        string name = reader.GetString(type.Name);
        if (type.ResolutionScope.Kind == HandleKind.TypeReference)
        {
            return $"{GetTypeReferenceName(reader, (TypeReferenceHandle)type.ResolutionScope)}+{name}";
        }
        string ns = reader.GetString(type.Namespace);
        return ns.Length == 0 ? name : $"{ns}.{name}";
    }
}
