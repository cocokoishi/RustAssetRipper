// Isolated reference probe for the GameAssetFactory creation dispatch.
//
// Modes:
//   dispatch <version> <classid...>
//       Builds an empty SerializedFile at <version>, materializes its (empty)
//       SerializedAssetCollection through the shipped GameAssetFactory, then calls
//       AssetFactory.CreateSerialized directly for each class id and dumps the
//       generated class name (or MISSING) plus the TPK lookup outcome.
//   sweep <version> <maxclassid>
//       Prints only the class ids without a generated class (TPK / Unknown fallbacks).
//   tpk <classid> <version...>
//       Dumps the real TPK TypeTree node lists (release + editor) for one class.
//   author <rows>
//       Authors the factory differential fixture: each case is a SerializedFile
//       written to target/ser-factory-probe/, reloaded through SchemeReader and
//       materialized with the shipped GameAssetFactory. Emits SF_HEX|case|hex and
//       SF_READ rows for every case.
//   materialize <file> <rows>
//       Loads one SerializedFile through the shipped factory and dumps every
//       materialized object's class, generated type name, and stored name.
// Reads only its inputs; writes only the rows file and scratch under target/.
using System.Reflection;
using System.Text;
using AssetRipper.Assets;
using AssetRipper.Assets.Bundles;
using AssetRipper.Assets.Collections;
using AssetRipper.Assets.Metadata;
using AssetRipper.Import.AssetCreation;
using AssetRipper.Import.Logging;
using AssetRipper.Import.Structure.Assembly.Managers;
using AssetRipper.Import.Structure.Assembly.TypeTrees;
using AssetRipper.IO.Endian;
using AssetRipper.IO.Files;
using AssetRipper.IO.Files.SerializedFiles;
using AssetRipper.IO.Files.SerializedFiles.Parser;
using AssetRipper.IO.Files.SerializedFiles.Parser.TypeTrees;
using AssetRipper.Primitives;
using AssetRipper.SourceGenerated;

if (args.Length == 0)
{
	Console.Error.WriteLine("usage: dispatch <version> <classid...> | sweep <version> <max> | tpk <classid> <version...> | author <rows> | materialize <file> <rows>");
	return 2;
}

try
{
	switch (args[0])
	{
		case "dispatch":
			DumpDispatch(args[1], args[2..]);
			return 0;
		case "sweep":
			Sweep(args[1], int.Parse(args[2]));
			return 0;
		case "tpk":
			DumpTpk(int.Parse(args[1]), args[2..]);
			return 0;
		case "author":
			Author(args[1]);
			return 0;
		case "materialize":
			Materialize(args[1], args[2]);
			return 0;
		default:
			Console.Error.WriteLine("unknown mode");
			return 2;
	}
}
catch (Exception ex)
{
	Console.Error.WriteLine($"PROBE_FAIL|{ex.GetType().FullName}|{OneLine(ex.Message)}");
	return 1;
}

static string OneLine(string value) => value.Replace('\r', ' ').Replace('\n', ' ');

// An empty serialized file at the requested version gives a real collection whose
// Version/Platform/EndianType facts the factory consults, without needing any payload.
static SerializedAssetCollection EmptyCollection(UnityVersion version)
{
	SerializedFile file = new SerializedFileBuilder
	{
		Generation = FormatVersion.Unknown_10,
		Version = version,
		Platform = (BuildTarget)19,
		EndianType = EndianType.LittleEndian,
		HasTypeTree = false,
	}.Build();
	GameAssetFactory factory = new(new BaseManager(s => { }));
	GameBundle bundle = new();
	return bundle.AddCollectionFromSerializedFile(file, factory);
}

static void DumpDispatch(string versionText, string[] classIds)
{
	UnityVersion version = UnityVersion.Parse(versionText);
	SerializedAssetCollection collection = EmptyCollection(version);
	foreach (string text in classIds)
	{
		int classId = int.Parse(text);
		Console.WriteLine($"GEN|{classId}|{GeneratedName(collection, classId, version)}");
		Console.WriteLine($"TPK|{classId}|{TpkOutcome(classId, version)}");
	}
}

static void Sweep(string versionText, int maximum)
{
	UnityVersion version = UnityVersion.Parse(versionText);
	SerializedAssetCollection collection = EmptyCollection(version);
	for (int classId = 1; classId <= maximum; classId++)
	{
		if (GeneratedName(collection, classId, version) != "MISSING")
		{
			continue;
		}
		Console.WriteLine($"MISSING_GEN|{classId}|tpk={TpkOutcome(classId, version)}");
	}
}

static string GeneratedName(SerializedAssetCollection collection, int classId, UnityVersion version)
{
	try
	{
		IUnityObjectBase? asset = AssetFactory.CreateSerialized(new AssetInfo(collection, 42, classId), version);
		return asset is null ? "MISSING" : asset.GetType().Name;
	}
	catch (Exception ex)
	{
		return $"THROW:{ex.GetType().Name}";
	}
}

static string TpkOutcome(int classId, UnityVersion version)
{
	try
	{
		return TypeTreeNodeStruct.TryMakeFromTpk((ClassIDType)classId, version, out _, out _) ? "yes" : "no";
	}
	catch (Exception ex)
	{
		return $"THROW:{ex.GetType().Name}";
	}
}

static void DumpTpk(int classId, string[] versions)
{
	foreach (string versionText in versions)
	{
		UnityVersion version = UnityVersion.Parse(versionText);
		if (TypeTreeNodeStruct.TryMakeFromTpk((ClassIDType)classId, version, out TypeTreeNodeStruct release, out TypeTreeNodeStruct editor))
		{
			Console.WriteLine($"TPK|{classId}|{versionText}|release");
			DumpNodes(release, 0);
			Console.WriteLine($"TPK|{classId}|{versionText}|editor");
			DumpNodes(editor, 0);
		}
		else
		{
			Console.WriteLine($"TPK|{classId}|{versionText}|MISSING");
		}
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

// ---------------------------------------------------------------------------
// Fixture authoring
// ---------------------------------------------------------------------------

static void Author(string rowsPath)
{
	string[] cases =
	[
		"textasset_notree",
		"textasset_tree",
		"textasset_padding",
		"textasset_bad_tail",
		"unknown_class",
		"tpk_navmesh",
		"unreadable_version",
		"engine_default_resources",
		"mono_unloaded",
		"mono_with_structure",
		"monoscript_notree",
	];
	string scratch = Path.GetFullPath(Path.Combine(Environment.CurrentDirectory, "target", "ser-factory-probe"));
	Directory.CreateDirectory(scratch);
	Logger.Add(new CaptureLogger());
	using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false));
	rows.WriteLine("# Reference GameAssetFactory differential (probe tools/probes/ser-factory, .NET SDK 10.0.401).");
	rows.WriteLine("# GEN rows: AssetFactory.CreateSerialized(new AssetInfo(collection,42,classId), 2019.4.40f1)");
	rows.WriteLine("# on an empty real SerializedAssetCollection, plus the TypeTreeNodeStruct.TryMakeFromTpk outcome.");
	rows.WriteLine("# TPK rows: the real TPK TypeTree nodes used by the CreateAsset fallback.");
	rows.WriteLine("# SF_HEX rows: the authored SerializedFiles; SF_READ rows: the shipped factory materialization");
	rows.WriteLine("# of those exact bytes reloaded through SchemeReader (StandaloneWin64Player platform facts).");
	UnityVersion version = UnityVersion.Parse("2019.4.40f1");
	SerializedAssetCollection collection = EmptyCollection(version);
	foreach (int classId in new[] { 7, 49, 114, 115, 129, 141, 194 })
	{
		rows.WriteLine($"GEN|{classId}|{GeneratedName(collection, classId, version)}");
		rows.WriteLine($"TPK|{classId}|{TpkOutcome(classId, version)}");
	}
	foreach (int classId in new[] { 49, 114, 115, 194 })
	{
		WriteTpk(rows, classId, version);
	}
	foreach (string name in cases)
	{
		(string fileName, SerializedFile file) = BuildCase(name);
		byte[] bytes = file.ToByteArray();
		rows.WriteLine($"SF_HEX|{name}|{Convert.ToHexString(bytes)}");
		string path = Path.Combine(scratch, fileName);
		File.WriteAllBytes(path, bytes);
		using FileBase loaded = SchemeReader.LoadFile(path, LocalFileSystem.Instance);
		DumpMaterialization(rows, name, loaded);
	}
}

static void WriteTpk(StreamWriter rows, int classId, UnityVersion version)
{
	if (!TypeTreeNodeStruct.TryMakeFromTpk((ClassIDType)classId, version, out TypeTreeNodeStruct release, out TypeTreeNodeStruct editor))
	{
		rows.WriteLine($"TPK|{classId}|{version}|MISSING");
		return;
	}
	rows.WriteLine($"TPK|{classId}|{version}|release");
	WriteNodes(rows, release, 0);
	rows.WriteLine($"TPK|{classId}|{version}|editor");
	WriteNodes(rows, editor, 0);
}

static void WriteNodes(StreamWriter rows, TypeTreeNodeStruct node, int level)
{
	rows.WriteLine($"NODE|{level}|{node.TypeName}|{node.Name}|v{node.Version}|meta{(int)node.MetaFlag}");
	foreach (TypeTreeNodeStruct child in node.SubNodes)
	{
		WriteNodes(rows, child, level + 1);
	}
}

static (string, SerializedFile) BuildCase(string name)
{
	switch (name)
	{
		case "textasset_notree":
			return ("textasset_notree.assets", BuildTextAssetFile("2019.4.40f1", "Hero", "print(1)", null, []));
		case "textasset_tree":
			return ("textasset_tree.assets", BuildTextAssetFile("2019.4.40f1", "Hero", "print(1)", TextAssetReleaseTree(), []));
		case "textasset_padding":
			return ("textasset_padding.assets", BuildTextAssetFile("2019.4.40f1", "Hero", "print(1)", null, [0, 0, 0, 0]));
		case "textasset_bad_tail":
			return ("textasset_bad_tail.assets", BuildTextAssetFile("2019.4.40f1", "Hero", "print(1)", null, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE]));
		case "unknown_class":
			return ("unknown_class.assets", BuildFile("unknown_class.assets", "2019.4.40f1", 7, null, [1, 2, 3, 4, 5, 6, 7, 8]));
		case "tpk_navmesh":
			return ("tpk_navmesh.assets", BuildFile("tpk_navmesh.assets", "2019.4.40f1", 194, null, StringPayload("NavMeshData")));
		case "unreadable_version":
			return ("unreadable_version.assets", BuildTextAssetFile("3.4.0f1", "Hero", "print(1)", null, []));
		case "engine_default_resources":
			return ("unity default resources", BuildTextAssetFile("2019.4.40f1", "Hero", "print(1)", null, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE]));
		case "mono_unloaded":
			return ("mono_unloaded.assets", BuildMonoBehaviourFile());
		case "mono_with_structure":
			return ("mono_with_structure.assets", BuildMonoBehaviourWithStructureFile());
		case "monoscript_notree":
			return ("monoscript_notree.assets", BuildMonoScriptFile());
		default:
			throw new ArgumentException($"unknown case {name}");
	}
}

static SerializedFile BuildTextAssetFile(string versionText, string name, string script, TypeTree? tree, byte[] tail)
{
	List<byte> payload = [];
	WriteString(payload, name);
	WriteString(payload, script);
	payload.AddRange(tail);
	return BuildFile("x.assets", versionText, 49, tree, payload.ToArray());
}

static SerializedFile BuildFile(string fileName, string versionText, int classId, TypeTree? tree, byte[] payload)
{
	SerializedType type = new()
	{
		RawTypeID = classId,
	};
	if (tree is not null)
	{
		type.OldType.Nodes.AddRange(tree.Nodes);
		type.OldType.StringBuffer = tree.StringBuffer;
	}
	SerializedFile file = new SerializedFileBuilder
	{
		Generation = FormatVersion.Unknown_10,
		Version = UnityVersion.Parse(versionText),
		Platform = (BuildTarget)19,
		EndianType = EndianType.LittleEndian,
		HasTypeTree = false,
		Types = { type },
		Objects =
		{
			new ObjectInfo(type)
			{
				FileID = 42,
				TypeID = classId,
				ObjectData = payload,
			},
		},
	}.Build();
	file.FilePath = fileName;
	return file;
}

// The real TextAsset release tree at 2019.4.40f1 (probe `tpk 49 2019.4.40f1`).
static TypeTree TextAssetReleaseTree()
{
	(string Type, string Name, int Level, int ByteSize, int Meta)[] specs =
	[
		("TextAsset", "Base", 0, -1, 32768),
		("string", "m_Name", 1, -1, 557057),
		("Array", "Array", 2, -1, 540673),
		("int", "size", 3, 4, 524289),
		("char", "data", 3, 1, 524289),
		("string", "m_Script", 1, -1, 67141633),
		("Array", "Array", 2, -1, 67125249),
		("int", "size", 3, 4, 67108865),
		("char", "data", 3, 1, 67108865),
	];
	return MakeTree(specs);
}

// A MonoBehaviour whose generated header is followed by an unreadable remainder.
static SerializedFile BuildMonoBehaviourFile()
{
	List<byte> payload = [];
	WriteI32(payload, 0);          // m_GameObject.m_FileID
	WriteI64(payload, 0);          // m_GameObject.m_PathID
	payload.Add(1);                // m_Enabled (UInt8)
	while (payload.Count % 4 != 0)
	{
		payload.Add(0);
	}
	WriteI32(payload, 0);          // m_Script.m_FileID
	WriteI64(payload, 7);          // m_Script.m_PathID
	WriteString(payload, "Behaviour");
	payload.AddRange([0xDE, 0xAD, 0xBE, 0xEF]);
	return BuildFile("mono_unloaded.assets", "2019.4.40f1", 114, null, payload.ToArray());
}

// A MonoBehaviour whose type carries a script TypeTree: the generated header is
// followed by the script structure fields (starting after m_Name).
static SerializedFile BuildMonoBehaviourWithStructureFile()
{
	List<byte> payload = [];
	WriteI32(payload, 0);          // m_GameObject.m_FileID
	WriteI64(payload, 0);          // m_GameObject.m_PathID
	payload.Add(1);                // m_Enabled (UInt8)
	while (payload.Count % 4 != 0)
	{
		payload.Add(0);
	}
	WriteI32(payload, 0);          // m_Script.m_FileID
	WriteI64(payload, 7);          // m_Script.m_PathID
	WriteString(payload, "Behaviour");
	WriteI32(payload, 100);        // health
	return BuildFile("mono_with_structure.assets", "2019.4.40f1", 114, MonoBehaviourStructureTree(), payload.ToArray());
}

// TPK MonoBehaviour release tree plus one script field (`health`).
static TypeTree MonoBehaviourStructureTree()
{
	(string Type, string Name, int Level, int ByteSize, int Meta)[] specs =
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
		("int", "health", 1, 4, 0),
	];
	return MakeTree(specs);
}

// MonoScript release layout: m_Name, m_ExecutionOrder, m_PropertiesHash, three strings.
static SerializedFile BuildMonoScriptFile()
{
	List<byte> payload = [];
	WriteString(payload, "HeroController");
	WriteI32(payload, 200);
	for (int i = 0; i < 16; i++)
	{
		payload.Add((byte)i);
	}
	WriteString(payload, "HeroController");
	WriteString(payload, "VRC.SDK");
	WriteString(payload, "Assembly-CSharp");
	return BuildFile("monoscript_notree.assets", "2019.4.40f1", 115, null, payload.ToArray());
}

static TypeTree MakeTree((string Type, string Name, int Level, int ByteSize, int Meta)[] specs)
{
	List<byte> buffer = [];
	List<uint> typeOffsets = [];
	List<uint> nameOffsets = [];
	foreach ((string type, string nodeName, int _, int _, int _) in specs)
	{
		typeOffsets.Add((uint)buffer.Count);
		buffer.AddRange(Encoding.UTF8.GetBytes(type));
		buffer.Add(0);
		nameOffsets.Add((uint)buffer.Count);
		buffer.AddRange(Encoding.UTF8.GetBytes(nodeName));
		buffer.Add(0);
	}
	TypeTree tree = new()
	{
		StringBuffer = buffer.ToArray(),
	};
	for (int i = 0; i < specs.Length; i++)
	{
		tree.Nodes.Add(new TypeTreeNode
		{
			Version = 1,
			Level = (byte)specs[i].Level,
			TypeFlags = 0,
			TypeStrOffset = typeOffsets[i],
			NameStrOffset = nameOffsets[i],
			ByteSize = specs[i].ByteSize,
			Index = i,
			MetaFlag = (TransferMetaFlags)specs[i].Meta,
		});
	}
	return tree;
}

static byte[] StringPayload(string value)
{
	List<byte> payload = [];
	WriteString(payload, value);
	return payload.ToArray();
}

static void WriteString(List<byte> payload, string value)
{
	byte[] utf8 = Encoding.UTF8.GetBytes(value);
	WriteI32(payload, utf8.Length);
	payload.AddRange(utf8);
	while (payload.Count % 4 != 0)
	{
		payload.Add(0);
	}
}

static void WriteI32(List<byte> payload, int value) => payload.AddRange(BitConverter.GetBytes(value));
static void WriteI64(List<byte> payload, long value) => payload.AddRange(BitConverter.GetBytes(value));

// ---------------------------------------------------------------------------
// Materialization dump
// ---------------------------------------------------------------------------

static void DumpMaterialization(StreamWriter rows, string name, FileBase file)
{
	GameAssetFactory factory = new(new BaseManager(s => { }));
	CaptureLogger.Messages.Clear();
	if (file is not SerializedFile serializedFile)
	{
		rows.WriteLine($"SF_READ|{name}|KIND|{file.GetType().Name}");
		return;
	}
	GameBundle bundle = new();
	SerializedAssetCollection collection = bundle.AddCollectionFromSerializedFile(serializedFile, factory);
	rows.WriteLine($"SF_READ|{name}|FILE|{serializedFile.NameFixed}|version={serializedFile.Version}|assets={collection.Count}");
	foreach (IUnityObjectBase asset in collection)
	{
		rows.WriteLine($"SF_READ|{name}|ASSET|{asset.PathID}|class={asset.ClassID}|type={asset.GetType().Name}|classname={GetProperty(asset, "ClassName")}|name={NameOf(asset)}");
		foreach (string message in CaptureLogger.Messages)
		{
			rows.WriteLine($"SF_READ|{name}|LOG|{message}");
		}
		CaptureLogger.Messages.Clear();
		switch (asset.ClassID)
		{
			case (int)ClassIDType.BuildSettings:
				DumpBuildSettings(rows, name, asset);
				break;
			case (int)ClassIDType.MonoBehaviour:
				DumpMonoBehaviour(rows, name, asset);
				break;
			case (int)ClassIDType.TextAsset:
				object? script = GetProperty(asset, "Script");
				rows.WriteLine($"SF_READ|{name}|TEXT|{Describe(script)}");
				break;
			case (int)ClassIDType.MonoScript:
				rows.WriteLine($"SF_READ|{name}|SCRIPTNAME|{GetProperty(asset, "AssemblyName")}|{GetProperty(asset, "Namespace")}|{GetProperty(asset, "ExecutionOrder")}");
				break;
		}
	}
	foreach (string message in CaptureLogger.Messages)
	{
		rows.WriteLine($"SF_READ|{name}|LOG|{message}");
	}
}

static void Materialize(string path, string rowsPath)
{
	using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false));
	using FileBase file = SchemeReader.LoadFile(path, LocalFileSystem.Instance);
	DumpMaterialization(rows, Path.GetFileName(path), file);
}

static string NameOf(IUnityObjectBase asset)
{
	object? name = GetProperty(asset, "Name");
	return name?.ToString() ?? "NULL";
}

static void DumpBuildSettings(StreamWriter rows, string name, object asset)
{
	object? scenes = GetProperty(asset, "Scenes") ?? GetProperty(asset, "Levels");
	if (scenes is System.Collections.IEnumerable items)
	{
		foreach (object? item in items)
		{
			rows.WriteLine($"SF_READ|{name}|SCENE|{GetProperty(item, "String")}");
		}
	}
	foreach (string propertyName in new[] { "Version", "AuthToken" })
	{
		rows.WriteLine($"SF_READ|{name}|{propertyName.ToUpperInvariant()}|{GetProperty(GetProperty(asset, propertyName), "String")}");
	}
	object? guid = GetProperty(asset, "BuildGUID_GUID") ?? GetProperty(asset, "BuildGUID");
	rows.WriteLine(guid is null
		? $"SF_READ|{name}|GUID|ABSENT"
		: $"SF_READ|{name}|GUID|{GetProperty(guid, "Data_0_")},{GetProperty(guid, "Data_1_")},{GetProperty(guid, "Data_2_")},{GetProperty(guid, "Data_3_")}");
	foreach (string propertyName in new[] { "HasShadows", "EnableDynamicBatching", "HasPROVersion", "HasPublishingRights" })
	{
		rows.WriteLine($"SF_READ|{name}|BOOL|{propertyName}|{GetProperty(asset, propertyName)}");
	}
}

static void DumpMonoBehaviour(StreamWriter rows, string name, object asset)
{
	object? structure = GetProperty(asset, "Structure");
	rows.WriteLine($"SF_READ|{name}|STRUCTURE|{(structure is null ? "NULL" : structure.GetType().Name)}");
	object? script = GetProperty(asset, "Script");
	if (script is not null)
	{
		rows.WriteLine($"SF_READ|{name}|SCRIPT|{GetProperty(script, "FileID")}:{GetProperty(script, "PathID")}");
	}
}

static string Describe(object? value)
{
	if (value is null)
	{
		return "NULL";
	}
	object? text = GetProperty(value, "String");
	return $"<{value.GetType().Name}>{(text is null ? value.ToString() : text.ToString())}";
}

static object? GetProperty(object? value, string name)
{
	return value?.GetType().GetProperty(name, BindingFlags.Public | BindingFlags.Instance)?.GetValue(value);
}

sealed class CaptureLogger : ILogger
{
	public static readonly List<string> Messages = [];

	public void Log(LogType type, LogCategory category, string message)
	{
		Messages.Add($"{type}|{category}|{message.Replace('\r', ' ').Replace('\n', ' ')}");
	}

	public void BlankLine(int numLines)
	{
	}
}
