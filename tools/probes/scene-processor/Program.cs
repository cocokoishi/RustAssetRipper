// Isolated reference probe for the Scene/Prefab processor sequence.
//
// Mode tpk <version> <classId...>: dump the real TPK TypeTree node lists (release+editor) so the
//   Rust fixture builder can reproduce the frozen layouts byte-exactly.
// Mode process <rows> <file...>: load each SerializedFile through the shipped factory into one
//   GameBundle, then run the real SceneDefinitionProcessor and PrefabProcessor in source order
//   and dump the observable outcome (collections, scene definitions, generated hierarchies,
//   generated Transform objects). Writes only the rows file; reads only inputs.
// Mode notree-unresolved-component <input> <rows>: author a no-TypeTree GameObject with one
//   missing external Transform PPtr and record the real processor output.
using System.Collections;
using System.Reflection;
using AssetRipper.Checksum;
using AssetRipper.Assets;
using AssetRipper.Assets.Bundles;
using AssetRipper.Assets.Collections;
using AssetRipper.Assets.IO.Writing;
using AssetRipper.Assets.Metadata;
using AssetRipper.Export.Configuration;
using AssetRipper.Export.UnityProjects;
using AssetRipper.Import.AssetCreation;
using AssetRipper.Import.Logging;
using AssetRipper.Import.Structure.Assembly.Managers;
using AssetRipper.Import.Structure.Assembly.TypeTrees;
using AssetRipper.IO.Endian;
using AssetRipper.IO.Files;
using AssetRipper.IO.Files.SerializedFiles;
using AssetRipper.IO.Files.SerializedFiles.Parser;
using AssetRipper.Primitives;
using AssetRipper.Processing;
using AssetRipper.Processing.Prefabs;
using AssetRipper.Processing.Scenes;
using AssetRipper.SourceGenerated;
using AssetRipper.SourceGenerated.Classes.ClassID_1;
using AssetRipper.SourceGenerated.Classes.ClassID_142;
using AssetRipper.SourceGenerated.Classes.ClassID_4;
using AssetRipper.SourceGenerated.Classes.ClassID_43;
using AssetRipper.SourceGenerated.Extensions;
using AssetRipper.SourceGenerated.Subclasses.BlendShapeData;
using AssetRipper.SourceGenerated.Subclasses.MeshBlendShape;
using AssetRipper.SourceGenerated.Subclasses.MeshBlendShapeChannel;
using System.Security.Cryptography;

if (args.Length == 0)
{
	Console.Error.WriteLine("usage: tpk <version> <classId...> | naming <version> <name...> | process <rows> <file...> | blend-shape <rows> | notree-unresolved-component <input> <rows> | legacy-export <input> <rows> <output> | raw-fallback-export <input> <rows> <output>");
	return 2;
}

try
{
	if (args[0] == "tpk")
	{
		DumpTpk(args[1], args[2..]);
		return 0;
	}
	if (args[0] == "naming" && args.Length >= 3)
	{
		DumpSceneNaming(args[1], args[2..]);
		return 0;
	}
	if (args[0] == "blend-shape" && args.Length == 2)
	{
		ProbeBlendShape(args[1]);
		return 0;
	}
	if (args[0] == "process" && args.Length >= 3)
	{
		Process(args[1], args[2..]);
		return 0;
	}
	if (args[0] == "legacy-export" && args.Length == 4)
	{
		ExportLegacyPrefab(args[1], args[2], args[3]);
		return 0;
	}
	if (args[0] == "notree-prefab-export" && args.Length == 4)
	{
		ExportNoTreePrefab(args[1], args[2], args[3]);
		return 0;
	}
	if (args[0] == "notree-unresolved-component" && args.Length == 3)
	{
		ProbeNoTreeUnresolvedComponent(args[1], args[2]);
		return 0;
	}
	if (args[0] == "raw-fallback-export" && args.Length == 4)
	{
		ExportRawFallback(args[1], args[2], args[3]);
		return 0;
	}
	Console.Error.WriteLine("usage: tpk <version> <classId...> | naming <version> <name...> | process <rows> <file...> | legacy-export <input> <rows> <output> | notree-prefab-export <input> <rows> <output> | raw-fallback-export <input> <rows> <output>");
	return 2;
}
catch (Exception ex)
{
	Console.Error.WriteLine($"PROBE_FAIL|{ex.GetType().FullName}|{OneLine(ex.Message)}");
	Console.Error.WriteLine(ex.StackTrace);
	return 1;
}

static string OneLine(string value) => value.Replace('\r', ' ').Replace('\n', ' ');

static void ProbeBlendShape(string rowsPath)
{
	if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
	{
		throw new IOException($"refusing to overwrite probe rows {rowsPath}");
	}
	Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(rowsPath))!);
	using StreamWriter rows = new(rowsPath, false, new System.Text.UTF8Encoding(false)) { NewLine = "\n" };

	IMesh modern = CreateMesh(UnityVersion.Parse("5.0.0f1"));
	if (!modern.Has_Shapes())
	{
		throw new InvalidDataException($"2015 mesh {modern.GetType().Name} has no generated m_Shapes property");
	}
	IBlendShapeData blendShapeData = modern.Shapes;
	MeshBlendShapeChannel first = blendShapeData.Channels.AddNew();
	first.SetValues("Smile", 2, 3);
	uint smileHash = Crc32Algorithm.HashUTF8("Smile");
	MeshBlendShapeChannel second = blendShapeData.Channels.AddNew();
	second.SetValues("Frown", 7, 1);
	second.NameHash = smileHash;
	string? firstMatch = modern.FindBlendShapeNameByCRC(smileHash);
	string? modernMiss = modern.FindBlendShapeNameByCRC(Crc32Algorithm.HashUTF8("Missing"));
	rows.WriteLine($"MESH|5.0.0f1|{modern.GetType().Name}|shapes={modern.Has_Shapes()}|shapes_list={modern.Has_ShapesList()}");
	rows.WriteLine($"SET|{first.Name.String}|{first.NameHash}|{first.FrameIndex}|{first.FrameCount}");
	rows.WriteLine($"SET|{second.Name.String}|{second.NameHash}|{second.FrameIndex}|{second.FrameCount}");
	rows.WriteLine($"QUERY|channels-first-match|{smileHash}|{firstMatch ?? "<null>"}");
	rows.WriteLine($"QUERY|channels-miss|{Crc32Algorithm.HashUTF8("Missing")}|{modernMiss ?? "<null>"}");

	IMesh legacy = CreateMesh(UnityVersion.Parse("4.1.0f1"));
	if (!legacy.Has_ShapesList())
	{
		throw new InvalidDataException($"4.1 mesh {legacy.GetType().Name} has no generated m_ShapesList property");
	}
	MeshBlendShape_4_1 absentName = legacy.ShapesList.AddNew();
	absentName.Name_R = null;
	MeshBlendShape_4_1 emptyName = legacy.ShapesList.AddNew();
	emptyName.Name = new Utf8String(string.Empty);
	MeshBlendShape_4_1 named = legacy.ShapesList.AddNew();
	named.Name = new Utf8String("LegacySmile");
	uint emptyHash = Crc32Algorithm.HashUTF8(string.Empty);
	uint legacyHash = Crc32Algorithm.HashUTF8("LegacySmile");
	string? emptyMatch = legacy.FindBlendShapeNameByCRC(emptyHash);
	string? legacyMatch = legacy.FindBlendShapeNameByCRC(legacyHash);
	string? legacyMiss = legacy.FindBlendShapeNameByCRC(Crc32Algorithm.HashUTF8("Missing"));
	rows.WriteLine($"MESH|4.1.0f1|{legacy.GetType().Name}|shapes={legacy.Has_Shapes()}|shapes_list={legacy.Has_ShapesList()}");
	rows.WriteLine($"QUERY|legacy-absent-name|{emptyHash}|{(absentName.IsCRCMatch(emptyHash) ? "true" : "false")}");
	rows.WriteLine($"QUERY|legacy-empty-name|{emptyHash}|{(emptyName.IsCRCMatch(emptyHash) ? "true" : "false")}");
	rows.WriteLine($"QUERY|legacy-name|{legacyHash}|{legacyMatch ?? "<null>"}");
	rows.WriteLine($"QUERY|legacy-miss|{Crc32Algorithm.HashUTF8("Missing")}|{legacyMiss ?? "<null>"}");
	if (emptyMatch is not null)
	{
		rows.WriteLine($"QUERY|legacy-empty-resolved|{emptyHash}|<empty>");
	}
	IMesh serializedLegacy = CreateMesh(UnityVersion.Parse("4.1.0f1"));
	var serializedLegacyShapes = serializedLegacy.ShapesList
		?? throw new InvalidDataException("4.1 mesh has no generated ShapesList collection");
	serializedLegacyShapes.AddNew().Name = new Utf8String(string.Empty);
	serializedLegacyShapes.AddNew().Name = new Utf8String("LegacySmile");
	byte[] modernFile = SerializeMesh(modern, UnityVersion.Parse("5.0.0f1"), "Mesh2015.assets");
	byte[] legacyFile = SerializeMesh(serializedLegacy, UnityVersion.Parse("4.1.0f1"), "Mesh41.assets");
	WriteInputRow(rows, "modern", modernFile);
	WriteInputRow(rows, "legacy", legacyFile);
	rows.Flush();

	Console.WriteLine($"modern_mesh={modern.GetType().Name}; first_match={firstMatch ?? "<null>"}; miss={modernMiss ?? "<null>"}");
	Console.WriteLine($"legacy_mesh={legacy.GetType().Name}; absent_match={absentName.IsCRCMatch(emptyHash)}; empty_match={emptyName.IsCRCMatch(emptyHash)}; name_match={legacyMatch ?? "<null>"}; miss={legacyMiss ?? "<null>"}");
}

static IMesh CreateMesh(UnityVersion version)
{
	SerializedFile template = new SerializedFileBuilder
	{
		Generation = FormatVersion.Unknown_10,
		Version = version,
		Platform = BuildTarget.NoTarget,
		EndianType = EndianType.LittleEndian,
		HasTypeTree = false,
	}.Build();
	GameAssetFactory factory = new(new BaseManager(_ => { }));
	GameBundle bundle = new();
	SerializedAssetCollection collection = bundle.AddCollectionFromSerializedFile(template, factory);
	IUnityObjectBase asset = AssetFactory.CreateSerialized(new AssetInfo(collection, 42, 43), version)
		?? throw new InvalidDataException($"source factory did not create a Mesh for {version}");
	return asset as IMesh
		?? throw new InvalidDataException($"source factory returned {asset.GetType().Name}, expected IMesh");
}

static byte[] SerializeMesh(IMesh mesh, UnityVersion version, string fileName)
{
	SerializedType type = new() { RawTypeID = 43 };
	byte[] payload = WriteAsset((IUnityObjectBase)mesh, true);
	SerializedFile file = new SerializedFileBuilder
	{
		Generation = FormatVersion.Unknown_10,
		Version = version,
		Platform = BuildTarget.NoTarget,
		EndianType = EndianType.LittleEndian,
		HasTypeTree = false,
		Types = { type },
		Objects =
		{
			new ObjectInfo(type) { FileID = 42, TypeID = 43, ObjectData = payload },
		},
	}.Build();
	file.FilePath = fileName;
	return file.ToByteArray();
}

static void WriteInputRow(StreamWriter rows, string name, byte[] bytes)
{
	rows.WriteLine($"INPUT|{name}|{bytes.Length}|{Convert.ToHexString(SHA256.HashData(bytes))}");
	rows.WriteLine($"INPUT_HEX|{name}|{Convert.ToHexString(bytes)}");
}

static void DumpTpk(string versionText, string[] classIds)
{
	UnityVersion version = UnityVersion.Parse(versionText);
	foreach (string text in classIds)
	{
		int classId = int.Parse(text);
		ClassIDType type = (ClassIDType)classId;
		if (TypeTreeNodeStruct.TryMakeFromTpk(type, version, out TypeTreeNodeStruct release, out TypeTreeNodeStruct editor))
		{
			Console.WriteLine($"TPK|{versionText}|{classId}|release");
			DumpNodes(release, 0);
			Console.WriteLine($"TPK|{versionText}|{classId}|editor");
			DumpNodes(editor, 0);
		}
		else
		{
			Console.WriteLine($"TPK|{versionText}|{classId}|MISSING");
		}
	}
}

static void DumpSceneNaming(string versionText, string[] names)
{
	UnityVersion version = UnityVersion.Parse(versionText);
	foreach (string name in names)
	{
		try
		{
			bool success = SceneHelpers.TryGetFileNameToSceneIndex(name, version, out int index);
			Console.WriteLine($"INDEX|{versionText}|{name}|{success}|{index}");
		}
		catch (Exception exception)
		{
			Console.WriteLine($"INDEX|{versionText}|{name}|ERROR|{exception.GetType().Name}");
		}
	}
	for (int index = 0; index <= 2; index++)
	{
		Console.WriteLine($"FILENAME|{versionText}|{index}|{SceneHelpers.SceneIndexToFileName(index, version)}");
	}
}

static void DumpNodes(TypeTreeNodeStruct node, int level)
{
	Console.WriteLine($"NODE|{level}|{node.TypeName}|{node.Name}|v{node.Version}|meta{(int)node.MetaFlag}");
	foreach (TypeTreeNodeStruct child in node.SubNodes)
	{
		DumpNodes(child, level + 1);
	}
}

static void Process(string rowsPath, string[] paths)
{
	Logger.Add(new ConsoleLogger());
	GameAssetFactory factory = new GameAssetFactory(new BaseManager(s => { }));
	GameBundle bundle = new();
	SerializedBundle serializedBundle = new();
	bundle.AddBundle(serializedBundle);
	UnityVersion version = default;
	foreach (string path in paths)
	{
		FileBase file = SchemeReader.LoadFile(path, LocalFileSystem.Instance);
		if (file is not SerializedFile serializedFile)
		{
			using StreamWriter skipped = new(rowsPath, true, System.Text.Encoding.UTF8);
			skipped.WriteLine($"KIND|{Path.GetFileName(path)}|{file.GetType().Name}");
			continue;
		}
		version = serializedFile.Version;
		serializedBundle.AddCollectionFromSerializedFile(serializedFile, factory);
	}
	GameData gameData = new(bundle, version, new BaseManager(s => { }), null);

	new SceneDefinitionProcessor().Process(gameData);
	new PrefabProcessor().Process(gameData);

	using StreamWriter rows = new(rowsPath, false, System.Text.Encoding.UTF8);
	rows.WriteLine($"FILE|{paths.Length}|version={version}");
	int collectionIndex = 0;
	foreach (AssetCollection collection in bundle.FetchAssetCollections())
	{
		rows.WriteLine($"COLL|{collectionIndex}|{collection.Name}|{collection.Count}|scene={IsScene(collection)}");
		foreach (IUnityObjectBase asset in collection)
		{
			rows.WriteLine($"ASSET|{collectionIndex}|{asset.PathID}|{asset.ClassID}|{asset.GetType().Name}");
		}
		collectionIndex++;
	}
	foreach (IUnityObjectBase asset in bundle.FetchAssets())
	{
		if (asset is PrefabHierarchyObject prefabHierarchy)
		{
			rows.WriteLine($"PREFAB-HIER|root={prefabHierarchy.Root.PathID}:{prefabHierarchy.Root.Name}|prefab={prefabHierarchy.Prefab.PathID}|gameObjects={FormatAssets(prefabHierarchy.GameObjects)}|components={FormatAssets(prefabHierarchy.Components)}|prefabInstances={FormatAssets(prefabHierarchy.PrefabInstances)}|exportable={FormatAssets(prefabHierarchy.ExportableAssets)}|hidden={FormatAssets(prefabHierarchy.HiddenAssets)}");
		}
		if (asset is IAssetBundle assetBundle)
		{
			rows.WriteLine($"BUNDLE-COMPUTED|{assetBundle.GetAssetBundleName()}");
			foreach (PropertyInfo property in asset.GetType().GetProperties(BindingFlags.Public | BindingFlags.Instance)
				.Where(property => property.Name.Contains("Scene", StringComparison.Ordinal)
					|| property.Name.Contains("AssetBundle", StringComparison.Ordinal)
					|| property.Name == "Name")
				.OrderBy(property => property.Name, StringComparer.Ordinal))
			{
				try
				{
					rows.WriteLine($"BUNDLE-PROPERTY|{property.Name}|{SafeString(property.GetValue(asset))}");
				}
				catch (Exception exception)
				{
					rows.WriteLine($"BUNDLE-PROPERTY|{property.Name}|ERROR:{exception.GetType().Name}");
				}
			}
		}
		if (asset is SceneHierarchyObject hierarchy)
		{
			Bundle? hierarchyBundle = hierarchy.Scene.Collections.Select(collection => collection.Bundle)
				.FirstOrDefault(candidate => candidate is SerializedBundle);
			IAssetBundle? hierarchyAssetBundle = hierarchyBundle?.FetchAssets().OfType<IAssetBundle>().FirstOrDefault();
			rows.WriteLine($"HIER-CHECK|{hierarchyBundle?.Name ?? "<null>"}|{hierarchyAssetBundle?.GetAssetBundleName() ?? "<null>"}");
			rows.WriteLine($"HIER-SET|{hierarchy.AssetBundleName ?? "<null>"}|collections={string.Join(",", hierarchy.Scene.Collections.Select(collection => $"{collection.Name}:{collection.Bundle.GetType().Name}"))}");
			DumpHierarchy(rows, hierarchy);
		}
	}
}

static void ExportLegacyPrefab(string inputPath, string rowsPath, string outputRoot)
{
	string inputFullPath = Path.GetFullPath(inputPath);
	string rowsFullPath = Path.GetFullPath(rowsPath);
	string outputFullPath = Path.GetFullPath(outputRoot);
	if (File.Exists(inputFullPath) || Directory.Exists(inputFullPath))
	{
		throw new IOException($"refusing to overwrite probe input {inputFullPath}");
	}
	if (File.Exists(rowsFullPath) || Directory.Exists(rowsFullPath))
	{
		throw new IOException($"refusing to overwrite probe rows {rowsFullPath}");
	}
	if (Directory.Exists(outputFullPath) && Directory.EnumerateFileSystemEntries(outputFullPath).Any())
	{
		throw new IOException($"refusing to export into non-empty output {outputFullPath}");
	}

	Directory.CreateDirectory(Path.GetDirectoryName(inputFullPath)!);
	Directory.CreateDirectory(Path.GetDirectoryName(rowsFullPath)!);
	Directory.CreateDirectory(outputFullPath);
	File.WriteAllBytes(inputFullPath, BuildLegacyRootFile());

	Logger.Add(new ConsoleLogger());
	FullConfiguration settings = new();
	ExportHandler handler = new(settings);
	GameData gameData = handler.LoadAndProcess([inputFullPath], LocalFileSystem.Instance);
	handler.Export(gameData, outputFullPath, LocalFileSystem.Instance);

	string[] exportedFiles = Directory.GetFiles(outputFullPath, "*", SearchOption.AllDirectories)
		.OrderBy(path => Path.GetRelativePath(outputFullPath, path), StringComparer.Ordinal)
		.ToArray();
	string[] prefabFiles = exportedFiles
		.Where(path => string.Equals(Path.GetExtension(path), ".prefab", StringComparison.OrdinalIgnoreCase))
		.ToArray();
	if (prefabFiles.Length != 1)
	{
		throw new InvalidDataException($"expected one legacy Prefab output, found {prefabFiles.Length}");
	}

	using StreamWriter rows = new(rowsFullPath, false, new System.Text.UTF8Encoding(false)) { NewLine = "\n" };
	rows.WriteLine("# Frozen source AssetRipper 2017.4.40f1 generated legacy Prefab project export.");
	byte[] inputBytes = File.ReadAllBytes(inputFullPath);
	rows.WriteLine($"INPUT|{Path.GetFileName(inputFullPath)}|{Convert.ToHexString(SHA256.HashData(inputBytes))}");
	rows.WriteLine($"INPUT_HEX|{Convert.ToHexString(inputBytes)}");
	rows.WriteLine($"VERSION|{gameData.ProjectVersion}");
	foreach (string file in exportedFiles)
	{
		string relativePath = Path.GetRelativePath(outputFullPath, file).Replace('\\', '/');
		byte[] content = File.ReadAllBytes(file);
		rows.WriteLine($"FILE|{relativePath}|{content.Length}|{Convert.ToHexString(SHA256.HashData(content))}");
		if (string.Equals(Path.GetExtension(file), ".prefab", StringComparison.OrdinalIgnoreCase)
			|| string.Equals(Path.GetFileName(file), Path.GetFileName(prefabFiles[0]) + ".meta", StringComparison.OrdinalIgnoreCase))
		{
			rows.WriteLine($"CONTENT|{relativePath}|{Convert.ToBase64String(content)}");
		}
	}
}

static void ExportNoTreePrefab(string inputPath, string rowsPath, string outputRoot)
{
	string inputFullPath = Path.GetFullPath(inputPath);
	string rowsFullPath = Path.GetFullPath(rowsPath);
	string outputFullPath = Path.GetFullPath(outputRoot);
	if (File.Exists(inputFullPath) || Directory.Exists(inputFullPath))
	{
		throw new IOException($"refusing to overwrite probe input {inputFullPath}");
	}
	if (File.Exists(rowsFullPath) || Directory.Exists(rowsFullPath))
	{
		throw new IOException($"refusing to overwrite probe rows {rowsFullPath}");
	}
	if (Directory.Exists(outputFullPath) && Directory.EnumerateFileSystemEntries(outputFullPath).Any())
	{
		throw new IOException($"refusing to export into non-empty output {outputFullPath}");
	}

	Directory.CreateDirectory(Path.GetDirectoryName(inputFullPath)!);
	Directory.CreateDirectory(Path.GetDirectoryName(rowsFullPath)!);
	Directory.CreateDirectory(outputFullPath);
	byte[] inputBytes = BuildNoTreePrefabFile();
	File.WriteAllBytes(inputFullPath, inputBytes);

	Logger.Add(new ConsoleLogger());
	FullConfiguration settings = new();
	ExportHandler handler = new(settings);
	GameData gameData = handler.LoadAndProcess([inputFullPath], LocalFileSystem.Instance);
	handler.Export(gameData, outputFullPath, LocalFileSystem.Instance);

	string[] exportedFiles = Directory.GetFiles(outputFullPath, "*", SearchOption.AllDirectories)
		.OrderBy(path => Path.GetRelativePath(outputFullPath, path), StringComparer.Ordinal)
		.ToArray();
	using StreamWriter rows = new(rowsFullPath, false, new System.Text.UTF8Encoding(false)) { NewLine = "\n" };
	rows.WriteLine("# Frozen AssetRipper source-generated no-TypeTree Unity 2019.4.40f1 project export.");
	rows.WriteLine($"INPUT|{Path.GetFileName(inputFullPath)}|{inputBytes.Length}|{Convert.ToHexString(SHA256.HashData(inputBytes))}");
	rows.WriteLine($"INPUT_HEX|{Convert.ToHexString(inputBytes)}");
	foreach (IUnityObjectBase asset in gameData.GameBundle.FetchAssets())
	{
		string assetName = asset is INamed named ? named.Name.ToString() : "<unnamed>";
		rows.WriteLine($"OBJECT|{asset.GetType().Name}|{asset.ClassID}|{asset.PathID}|{assetName}");
		if (asset is IGameObject gameObject)
		{
			rows.WriteLine($"GAMEOBJECT|{asset.PathID}|{gameObject.Layer}|{gameObject.IsActive_Boolean}|{FormatAssets(gameObject.Components)}");
		}
		if (asset is ITransform transform)
		{
			rows.WriteLine($"TRANSFORM|{asset.PathID}|{transform.LocalPosition_C4.X}|{transform.LocalPosition_C4.Y}|{transform.LocalPosition_C4.Z}|{transform.LocalRotation_C4.X}|{transform.LocalRotation_C4.Y}|{transform.LocalRotation_C4.Z}|{transform.LocalRotation_C4.W}|{transform.LocalScale_C4.X}|{transform.LocalScale_C4.Y}|{transform.LocalScale_C4.Z}|{transform.RootOrder_C4}");
		}
	}
	foreach (string file in exportedFiles)
	{
		string relativePath = Path.GetRelativePath(outputFullPath, file).Replace('\\', '/');
		byte[] content = File.ReadAllBytes(file);
		rows.WriteLine($"FILE|{relativePath}|{content.Length}|{Convert.ToHexString(SHA256.HashData(content))}");
		if (string.Equals(Path.GetExtension(file), ".prefab", StringComparison.OrdinalIgnoreCase)
			|| string.Equals(Path.GetFileName(file), Path.GetFileNameWithoutExtension(file) + ".meta", StringComparison.OrdinalIgnoreCase)
			&& file.EndsWith(".prefab.meta", StringComparison.OrdinalIgnoreCase))
		{
			rows.WriteLine($"CONTENT|{relativePath}|{Convert.ToBase64String(content)}");
		}
	}
}

static byte[] BuildNoTreePrefabFile()
{
	UnityVersion version = UnityVersion.Parse("2019.4.40f1");
	SerializedFile template = new SerializedFileBuilder
	{
		Generation = FormatVersion.Unknown_10,
		Version = version,
		Platform = BuildTarget.NoTarget,
		EndianType = EndianType.LittleEndian,
		HasTypeTree = false,
	}.Build();
	GameAssetFactory factory = new(new BaseManager(s => { }));
	GameBundle bundle = new();
	SerializedAssetCollection collection = bundle.AddCollectionFromSerializedFile(template, factory);
	IUnityObjectBase gameObjectAsset = AssetFactory.CreateSerialized(new AssetInfo(collection, 10, 1), version)
		?? throw new InvalidDataException("source factory did not create the 2019.4 GameObject");
	if (gameObjectAsset is not IGameObject gameObject)
	{
		throw new InvalidDataException($"source factory returned {gameObjectAsset.GetType().Name}, expected IGameObject");
	}
	IUnityObjectBase transformAsset = AssetFactory.CreateSerialized(new AssetInfo(collection, 11, 4), version)
		?? throw new InvalidDataException("source factory did not create the 2019.4 Transform");
	if (transformAsset is not ITransform transform)
	{
		throw new InvalidDataException($"source factory returned {transformAsset.GetType().Name}, expected ITransform");
	}
	transform.InitializeDefault();
	transform.GameObject_C4P = gameObject;
	transform.LocalPosition_C4.X = 1.25f;
	transform.LocalPosition_C4.Y = -2.5f;
	transform.LocalPosition_C4.Z = 3.75f;
	transform.LocalRotation_C4.X = 0.0f;
	transform.LocalRotation_C4.Y = 0.70710677f;
	transform.LocalRotation_C4.Z = 0.0f;
	transform.LocalRotation_C4.W = 0.70710677f;
	transform.LocalScale_C4.X = 2.0f;
	transform.LocalScale_C4.Y = 0.5f;
	transform.LocalScale_C4.Z = 1.25f;
	transform.RootOrder_C4 = 0;
	gameObject.AddComponent(ClassIDType.Transform, transform);
	gameObject.Name = new Utf8String("NoTreeRoot");
	gameObject.Layer = 12;
	gameObject.IsActive_Boolean = true;

	byte[] gameObjectPayload = WriteAsset(gameObjectAsset, true);
	byte[] transformPayload = WriteAsset(transformAsset, true);
	SerializedType gameObjectType = new() { RawTypeID = 1 };
	SerializedType transformType = new() { RawTypeID = 4 };
	SerializedFile file = new SerializedFileBuilder
	{
		Generation = FormatVersion.Unknown_10,
		Version = version,
		Platform = BuildTarget.NoTarget,
		EndianType = EndianType.LittleEndian,
		HasTypeTree = false,
		Types = { gameObjectType, transformType },
		Objects =
		{
			new ObjectInfo(gameObjectType) { FileID = 10, TypeID = 1, ObjectData = gameObjectPayload },
			new ObjectInfo(transformType) { FileID = 11, TypeID = 4, ObjectData = transformPayload },
		},
	}.Build();
	file.FilePath = "NoTreeRoot.unity";
	return file.ToByteArray();
}

static void ProbeNoTreeUnresolvedComponent(string inputPath, string rowsPath)
{
	string inputFullPath = Path.GetFullPath(inputPath);
	string rowsFullPath = Path.GetFullPath(rowsPath);
	string processorRowsPath = rowsFullPath + ".processor";
	if (File.Exists(inputFullPath) || Directory.Exists(inputFullPath))
	{
		throw new IOException($"refusing to overwrite probe input {inputFullPath}");
	}
	if (File.Exists(rowsFullPath) || Directory.Exists(rowsFullPath)
		|| File.Exists(processorRowsPath) || Directory.Exists(processorRowsPath))
	{
		throw new IOException($"refusing to overwrite probe rows {rowsFullPath}");
	}
	Directory.CreateDirectory(Path.GetDirectoryName(inputFullPath)!);
	Directory.CreateDirectory(Path.GetDirectoryName(rowsFullPath)!);
	byte[] inputBytes = BuildNoTreeUnresolvedComponentFile();
	File.WriteAllBytes(inputFullPath, inputBytes);
	Process(processorRowsPath, [inputFullPath]);

	using StreamWriter rows = new(rowsFullPath, false, new System.Text.UTF8Encoding(false)) { NewLine = "\n" };
	rows.WriteLine("# Frozen AssetRipper source processor output for an unresolved no-TypeTree component PPtr.");
	rows.WriteLine($"INPUT|{Path.GetFileName(inputFullPath)}|{inputBytes.Length}|{Convert.ToHexString(SHA256.HashData(inputBytes))}");
	rows.WriteLine($"INPUT_HEX|{Convert.ToHexString(inputBytes)}");
	foreach (string row in File.ReadLines(processorRowsPath))
	{
		rows.WriteLine(row);
	}
	File.Delete(processorRowsPath);
}

static byte[] BuildNoTreeUnresolvedComponentFile()
{
	UnityVersion version = UnityVersion.Parse("2019.4.40f1");
	SerializedFile template = new SerializedFileBuilder
	{
		Generation = FormatVersion.Unknown_10,
		Version = version,
		Platform = BuildTarget.NoTarget,
		EndianType = EndianType.LittleEndian,
		HasTypeTree = false,
	}.Build();
	GameAssetFactory factory = new(new BaseManager(_ => { }));
	GameBundle bundle = new();
	SerializedAssetCollection collection = bundle.AddCollectionFromSerializedFile(template, factory);
	IUnityObjectBase gameObjectAsset = AssetFactory.CreateSerialized(new AssetInfo(collection, 10, 1), version)
		?? throw new InvalidDataException("source factory did not create the unresolved-component GameObject");
	if (gameObjectAsset is not IGameObject gameObject)
	{
		throw new InvalidDataException($"source factory returned {gameObjectAsset.GetType().Name}, expected IGameObject");
	}
	gameObject.Name = new Utf8String("UnresolvedComponentRoot");
	gameObject.Layer = 3;
	gameObject.IsActive_Boolean = true;
	SerializedFile externalTemplate = new SerializedFileBuilder
	{
		Generation = FormatVersion.Unknown_10,
		Version = version,
		Platform = BuildTarget.NoTarget,
		EndianType = EndianType.LittleEndian,
		HasTypeTree = false,
	}.Build();
	externalTemplate.FilePath = "missing_component.assets";
	SerializedAssetCollection externalCollection = bundle.AddCollectionFromSerializedFile(externalTemplate, factory);
	collection.AddDependency(externalCollection);
	IUnityObjectBase missingTransform = AssetFactory.CreateSerialized(new AssetInfo(externalCollection, 99, 4), version)
		?? throw new InvalidDataException("source factory did not create the unresolved external Transform");
	if (missingTransform is not ITransform transform)
	{
		throw new InvalidDataException($"source factory returned {missingTransform.GetType().Name}, expected ITransform");
	}
	gameObject.AddComponent(ClassIDType.Transform, transform);
	byte[] payload = WriteAsset(gameObjectAsset, true);
	SerializedType gameObjectType = new() { RawTypeID = 1 };
	SerializedFile file = new SerializedFileBuilder
	{
		Generation = FormatVersion.Unknown_10,
		Version = version,
		Platform = BuildTarget.NoTarget,
		EndianType = EndianType.LittleEndian,
		HasTypeTree = false,
		Types = { gameObjectType },
		Dependencies =
		{
			new FileIdentifier
			{
				AssetPath = "missing_component.assets",
				PathNameOrigin = "missing_component.assets",
				PathName = "missing_component.assets",
				Type = AssetType.Serialized,
			},
		},
		Objects =
		{
			new ObjectInfo(gameObjectType)
			{
				FileID = 10,
				TypeID = 1,
				ObjectData = payload,
			},
		},
	}.Build();
	file.FilePath = "UnresolvedComponent.unity";
	return file.ToByteArray();
}

static byte[] WriteAsset(IUnityObjectBase asset, bool editor)
{
	using MemoryStream stream = new();
	using (AssetWriter writer = new(stream, asset.Collection))
	{
		if (editor)
		{
			asset.WriteEditor(writer);
		}
		else
		{
			asset.WriteRelease(writer);
		}
	}
	return stream.ToArray();
}

static void ExportRawFallback(string inputPath, string rowsPath, string outputRoot)
{
	string inputFullPath = Path.GetFullPath(inputPath);
	string rowsFullPath = Path.GetFullPath(rowsPath);
	string outputFullPath = Path.GetFullPath(outputRoot);
	if (!File.Exists(inputFullPath))
	{
		throw new FileNotFoundException("raw-fallback probe input was not found", inputFullPath);
	}
	if (File.Exists(rowsFullPath) || Directory.Exists(rowsFullPath))
	{
		throw new IOException($"refusing to overwrite probe rows {rowsFullPath}");
	}
	if (Directory.Exists(outputFullPath) && Directory.EnumerateFileSystemEntries(outputFullPath).Any())
	{
		throw new IOException($"refusing to export into non-empty output {outputFullPath}");
	}

	Directory.CreateDirectory(Path.GetDirectoryName(rowsFullPath)!);
	Directory.CreateDirectory(outputFullPath);
	Logger.Add(new ConsoleLogger());
	FullConfiguration settings = new();
	settings.ExportSettings.ExportUnreadableAssets = true;
	ExportHandler handler = new(settings);
	GameData gameData = handler.LoadAndProcess([inputFullPath], LocalFileSystem.Instance);
	handler.Export(gameData, outputFullPath, LocalFileSystem.Instance);

	string[] exportedFiles = Directory.GetFiles(outputFullPath, "*", SearchOption.AllDirectories)
		.OrderBy(path => Path.GetRelativePath(outputFullPath, path), StringComparer.Ordinal)
		.ToArray();
	string[] rawFiles = exportedFiles
		.Where(path => string.Equals(Path.GetExtension(path), ".unknown", StringComparison.OrdinalIgnoreCase)
			|| string.Equals(Path.GetExtension(path), ".unreadable", StringComparison.OrdinalIgnoreCase))
		.ToArray();
	if (rawFiles.Length != 1)
	{
		throw new InvalidDataException($"expected one raw fallback output, found {rawFiles.Length}");
	}
	string rawPath = rawFiles[0];
	if (File.Exists(rawPath + ".meta"))
	{
		throw new InvalidDataException($"raw fallback unexpectedly has a Unity metadata sidecar: {rawPath}.meta");
	}
	byte[] inputBytes = File.ReadAllBytes(inputFullPath);
	byte[] rawBytes = File.ReadAllBytes(rawPath);
	using StreamWriter rows = new(rowsFullPath, false, new System.Text.UTF8Encoding(false)) { NewLine = "\n" };
	rows.WriteLine("# Frozen AssetRipper raw fallback project-export probe.");
	rows.WriteLine($"INPUT|{Path.GetFileName(inputFullPath)}|{inputBytes.Length}|{Convert.ToHexString(SHA256.HashData(inputBytes))}");
	foreach (IUnityObjectBase asset in gameData.GameBundle.FetchAssets())
	{
		if (asset is not RawDataObject rawAsset)
		{
			continue;
		}
		string name = asset is INamed named ? named.Name.ToString() : "<unnamed>";
		rows.WriteLine($"OBJECT|{asset.GetType().Name}|{asset.ClassID}|{asset.ClassName}|{name}|{rawAsset.RawData.Length}|{rawAsset.RawDataHash:X8}");
	}
	string relativePath = Path.GetRelativePath(outputFullPath, rawPath).Replace('\\', '/');
	rows.WriteLine($"RAW_FILE|{relativePath}|{rawBytes.Length}|{Convert.ToHexString(SHA256.HashData(rawBytes))}");
	rows.WriteLine($"RAW_HEX|{Convert.ToHexString(rawBytes)}");
	foreach (string file in exportedFiles)
	{
		string fileRelativePath = Path.GetRelativePath(outputFullPath, file).Replace('\\', '/');
		byte[] content = File.ReadAllBytes(file);
		rows.WriteLine($"FILE|{fileRelativePath}|{content.Length}|{Convert.ToHexString(SHA256.HashData(content))}");
	}
}

static byte[] BuildLegacyRootFile()
{
	UnityVersion version = UnityVersion.Parse("2017.4.40f1");
	SerializedFile template = new SerializedFileBuilder
	{
		Generation = FormatVersion.Unknown_10,
		Version = version,
		Platform = BuildTarget.NoTarget,
		EndianType = EndianType.LittleEndian,
		HasTypeTree = false,
	}.Build();
	GameAssetFactory factory = new(new BaseManager(s => { }));
	GameBundle bundle = new();
	SerializedAssetCollection collection = bundle.AddCollectionFromSerializedFile(template, factory);
	IUnityObjectBase asset = AssetFactory.CreateSerialized(new AssetInfo(collection, 10, 1), version)
		?? throw new InvalidDataException("source factory did not create the 2017.4 GameObject");
	if (asset is not IGameObject gameObject)
	{
		throw new InvalidDataException($"source factory returned {asset.GetType().Name}, expected IGameObject");
	}
	IUnityObjectBase transformAsset = AssetFactory.CreateSerialized(new AssetInfo(collection, 11, 4), version)
		?? throw new InvalidDataException("source factory did not create the 2017.4 Transform");
	if (transformAsset is not ITransform transform)
	{
		throw new InvalidDataException($"source factory returned {transformAsset.GetType().Name}, expected ITransform");
	}
	transform.InitializeDefault();
	transform.GameObject_C4P = gameObject;
	gameObject.AddComponent(ClassIDType.Transform, transform);
	gameObject.Name = new Utf8String("LegacyRoot");
	using MemoryStream payloadStream = new();
	using (AssetWriter writer = new(payloadStream, asset.Collection))
	{
		asset.WriteEditor(writer);
	}
	using MemoryStream transformPayloadStream = new();
	using (AssetWriter writer = new(transformPayloadStream, transformAsset.Collection))
	{
		transformAsset.WriteEditor(writer);
	}

	SerializedType gameObjectType = new() { RawTypeID = 1 };
	SerializedType transformType = new() { RawTypeID = 4 };
	SerializedFile file = new SerializedFileBuilder
	{
		Generation = FormatVersion.Unknown_10,
		Version = version,
		Platform = BuildTarget.NoTarget,
		EndianType = EndianType.LittleEndian,
		HasTypeTree = false,
		Types = { gameObjectType, transformType },
		Objects =
		{
			new ObjectInfo(gameObjectType)
			{
				FileID = 10,
				TypeID = 1,
				ObjectData = payloadStream.ToArray(),
			},
			new ObjectInfo(transformType)
			{
				FileID = 11,
				TypeID = 4,
				ObjectData = transformPayloadStream.ToArray(),
			},
		},
	}.Build();
	file.FilePath = "LegacyRoot.unity";
	return file.ToByteArray();
}

static bool IsScene(AssetCollection collection) => collection.IsScene;

static void DumpHierarchy(StreamWriter rows, SceneHierarchyObject hierarchy)
{
	string name = SafeString(GetProperty(hierarchy, "Name"));
	string bundleName = SafeString(GetProperty(hierarchy, "AssetBundleName"));
	object? scene = GetProperty(hierarchy, "Scene");
	string sceneName = SafeString(GetProperty(scene, "Name"));
	string scenePath = SafeString(GetProperty(scene, "Path"));
	rows.WriteLine($"HIER|scene={sceneName}|path={scenePath}|name={name}|bundle={bundleName}");
	rows.WriteLine($"HIER-GUID|{SafeString(GetProperty(scene, "GUID"))}");
	IEnumerable? gameObjects = GetProperty(hierarchy, "GameObjects") as IEnumerable;
	rows.WriteLine($"HIER-GO|{FormatAssets(gameObjects)}");
	IEnumerable? components = GetProperty(hierarchy, "Components") as IEnumerable;
	rows.WriteLine($"HIER-COMPONENTS|{components?.Cast<object>().Count() ?? 0}");
	IEnumerable? managers = GetProperty(hierarchy, "Managers") as IEnumerable;
	rows.WriteLine($"HIER-MANAGERS|{FormatAssets(managers)}");
	IEnumerable? prefabInstances = GetProperty(hierarchy, "PrefabInstances") as IEnumerable;
	rows.WriteLine($"HIER-PREFABS|{prefabInstances?.Cast<object>().Count() ?? 0}");
	object? sceneRoots = GetProperty(hierarchy, "SceneRoots");
	rows.WriteLine($"HIER-SCENEROOTS|{(sceneRoots is null ? "null" : SafeString(GetProperty(sceneRoots, "PathID")))}");
	MethodInfo? getRoots = hierarchy.GetType().GetMethod("GetRoots", BindingFlags.Public | BindingFlags.Instance);
	if (getRoots?.Invoke(hierarchy, null) is IEnumerable roots)
	{
		rows.WriteLine($"HIER-ROOTS|{FormatAssets(roots)}");
	}
}

static string FormatAssets(IEnumerable? assets)
{
	if (assets is null)
	{
		return "null";
	}
	List<string> parts = new();
	foreach (object asset in assets)
	{
		string assetName = SafeString(GetProperty(asset, "Name"));
		string pathId = SafeString(GetProperty(asset, "PathID"));
		string type = asset.GetType().Name;
		parts.Add($"{type}#{pathId}:{assetName}");
	}
	return string.Join(",", parts);
}

static string SafeString(object? value) => value?.ToString() ?? "null";

static object? GetProperty(object? value, string name)
{
	return value?.GetType().GetProperty(name)?.GetValue(value);
}
