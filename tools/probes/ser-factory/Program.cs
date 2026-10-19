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
using System.Collections;
using System.IO.Compression;
using System.Globalization;
using System.Text;
using AssetRipper.Assets;
using AssetRipper.Assets.Bundles;
using AssetRipper.Assets.Collections;
using AssetRipper.Assets.IO.Writing;
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
using AssetRipper.Tpk;
using AssetRipper.Tpk.TypeTrees;

if (args.Length == 0)
{
		Console.Error.WriteLine("usage: dispatch <version> <classid...> | dispatch-matrix <output> | payload-matrix <output> | workspace-matrix <output.gz> [max-boundaries] | sweep <version> <max> | tpk <classid> <version...> | tpk-info <raw-output> <data-output> | tpk-select <output> | author <rows> | materialize <file> <rows>");
	return 2;
}

try
{
	switch (args[0])
	{
		case "dispatch":
			DumpDispatch(args[1], args[2..]);
			return 0;
		case "dispatch-matrix":
			DumpFactoryDispatchMatrix(args[1]);
			return 0;
		case "payload-matrix":
			DumpGeneratedPayloadMatrix(args[1]);
			return 0;
		case "workspace-matrix":
			DumpGeneratedWorkspaceMatrix(args[1], args.Length > 2 ? int.Parse(args[2], CultureInfo.InvariantCulture) : null);
			return 0;
		case "sweep":
			Sweep(args[1], int.Parse(args[2]));
			return 0;
		case "tpk":
			DumpTpk(int.Parse(args[1]), args[2..]);
			return 0;
		case "tpk-info":
			DumpTpkInfo(args[1], args[2]);
			return 0;
		case "tpk-select":
			DumpTpkSelectionMatrix(args[1]);
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

static void DumpTpkSelectionMatrix(string outputPath)
{
	TpkTypeTreeBlob blob = (TpkTypeTreeBlob)TpkFile.FromStream(SourceTpk.GetStream()).GetDataBlob();
	string fullPath = Path.GetFullPath(outputPath);
	Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
	using StreamWriter writer = new(fullPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
	writer.WriteLine("# Source TypeTreeNodeStruct.TryMakeFromTpk version-selection matrix.");
	writer.WriteLine("# SELECT|class-id|query-version|MISSING, THROW|exception-type, or OK|release-count|release-fnv64|editor-count|editor-fnv64");
	UnityVersion beforeFirst = UnityVersion.Parse("0.0.0f0");
	UnityVersion latest = blob.Versions[^1];
	foreach (TpkClassInformation classInformation in blob.ClassInformation.OrderBy(item => item.ID))
	{
		SortedSet<UnityVersion> queries = new() { beforeFirst, latest };
		foreach (var entry in classInformation.Classes)
		{
			queries.Add(entry.Key);
			int versionIndex = blob.Versions.BinarySearch(entry.Key);
			if (versionIndex > 0)
			{
				queries.Add(blob.Versions[versionIndex - 1]);
			}
		}
		foreach (UnityVersion query in queries)
		{
			try
			{
				if (!TypeTreeNodeStruct.TryMakeFromTpk((ClassIDType)classInformation.ID, query, out TypeTreeNodeStruct release, out TypeTreeNodeStruct editor))
				{
					writer.WriteLine($"SELECT|{classInformation.ID}|{query}|MISSING");
					continue;
				}
				var releaseSignature = Signature(release);
				var editorSignature = Signature(editor);
				writer.WriteLine($"SELECT|{classInformation.ID}|{query}|OK|{releaseSignature.Count}|{releaseSignature.Hash:X16}|{editorSignature.Count}|{editorSignature.Hash:X16}");
			}
			catch (Exception exception)
			{
				writer.WriteLine($"SELECT|{classInformation.ID}|{query}|THROW|{exception.GetType().Name}");
			}
		}
	}
}

static (int Count, ulong Hash) Signature(TypeTreeNodeStruct root)
{
	const ulong offsetBasis = 14695981039346656037;
	const ulong prime = 1099511628211;
	ulong hash = offsetBasis;
	int count = 0;
	Visit(root, 0);
	return (count, hash);

	void Visit(TypeTreeNodeStruct node, int depth)
	{
		string line = string.Concat(
			depth.ToString(CultureInfo.InvariantCulture), "|", node.TypeName, "|", node.Name, "|",
			node.Version.ToString(CultureInfo.InvariantCulture), "|", ((int)node.MetaFlag).ToString(CultureInfo.InvariantCulture), "\n");
		foreach (byte value in Encoding.UTF8.GetBytes(line))
		{
			hash ^= value;
			hash *= prime;
		}
		count++;
		foreach (TypeTreeNodeStruct child in node.SubNodes)
		{
			Visit(child, depth + 1);
		}
	}
}

static void DumpTpkInfo(string rawOutputPath, string dataOutputPath)
{
	using Stream source = SourceTpk.GetStream();
	using MemoryStream buffer = new();
	source.CopyTo(buffer);
	byte[] bytes = buffer.ToArray();
	string fullPath = Path.GetFullPath(rawOutputPath);
	Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
	File.WriteAllBytes(fullPath, bytes);
	Console.WriteLine($"TPK_RAW|{bytes.Length}|{Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(bytes))}");

	TpkFile tpk = TpkFile.FromStream(new MemoryStream(bytes));
	TpkTypeTreeBlob blob = (TpkTypeTreeBlob)tpk.GetDataBlob();
	DumpMembers("blob", blob);
	object? classInformation = blob.GetType().GetProperty("ClassInformation")?.GetValue(blob);
	object? firstClass = First(classInformation);
	if (firstClass is not null)
	{
		DumpMembers("class", firstClass);
		object? classes = firstClass.GetType().GetProperty("Classes")?.GetValue(firstClass);
		object? firstVersion = First(classes);
		if (firstVersion is not null)
		{
			DumpMembers("class-version", firstVersion);
			object? classValue = firstVersion.GetType().GetProperty("Value")?.GetValue(firstVersion);
			if (classValue is not null)
			{
				DumpMembers("unity-class", classValue);
			}
		}
	}
	object? nodeBuffer = blob.GetType().GetProperty("NodeBuffer")?.GetValue(blob);
	if (nodeBuffer is not null)
	{
		DumpMembers("node-buffer", nodeBuffer);
		PropertyInfo? indexer = nodeBuffer.GetType().GetProperties().FirstOrDefault(p => p.GetIndexParameters().Length == 1);
		object? firstNode = indexer?.GetValue(nodeBuffer, [0]);
		if (firstNode is not null)
		{
			DumpMembers("node", firstNode);
		}
	}
	object? stringBuffer = blob.GetType().GetProperty("StringBuffer")?.GetValue(blob);
	if (stringBuffer is not null)
	{
		DumpMembers("string-buffer", stringBuffer);
	}
	WriteTpkData(blob, dataOutputPath, bytes.Length, Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(bytes)));
}

static void WriteTpkData(TpkTypeTreeBlob blob, string path, int rawLength, string rawSha256)
{
	string fullPath = Path.GetFullPath(path);
	Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
	using StreamWriter writer = new(fullPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
	writer.WriteLine($"TPK-DATA|1|{rawLength}|{rawSha256}|{blob.ClassInformation.Count}|{blob.NodeBuffer.Count}|{blob.StringBuffer.Count}");
	foreach (var version in blob.Versions)
	{
		writer.WriteLine($"VERSION|{version}|{version.GetBits():X16}|{version.Major}|{version.Minor}|{version.Build}|{Convert.ToInt32(version.Type)}|{version.TypeNumber}");
	}
	foreach (TpkClassInformation classInformation in blob.ClassInformation.OrderBy(item => item.ID))
	{
		writer.WriteLine($"CLASSSET|{classInformation.ID}|{classInformation.Classes.Count}");
		foreach (var entry in classInformation.Classes)
		{
			var version = entry.Key;
			if (entry.Value is null)
			{
				writer.WriteLine($"CLASS|{classInformation.ID}|{version}|{version.GetBits():X16}|{version.Major}|{version.Minor}|{version.Build}|{Convert.ToInt32(version.Type)}|{version.TypeNumber}|NULL");
				continue;
			}
			var unityClass = entry.Value;
			writer.WriteLine($"CLASS|{classInformation.ID}|{version}|{version.GetBits():X16}|{version.Major}|{version.Minor}|{version.Build}|{Convert.ToInt32(version.Type)}|{version.TypeNumber}|{unityClass.Name}|{unityClass.Base}|{(byte)unityClass.Flags}|{unityClass.ReleaseRootNode}|{unityClass.EditorRootNode}");
		}
	}
	for (int index = 0; index < blob.NodeBuffer.Count; index++)
	{
		TpkUnityNode node = blob.NodeBuffer[index];
		string children = string.Join(',', node.SubNodes);
		writer.WriteLine($"NODE|{index}|{node.TypeName}|{node.Name}|{node.ByteSize}|{node.Version}|{node.TypeFlags}|{node.MetaFlag}|{children}");
	}
	for (int index = 0; index < blob.StringBuffer.Count; index++)
	{
		string value = blob.StringBuffer[checked((ushort)index)];
		writer.WriteLine($"STRING|{index}|{Convert.ToBase64String(Encoding.UTF8.GetBytes(value))}");
	}
}

static object? First(object? value) => value is IEnumerable enumerable ? enumerable.Cast<object?>().FirstOrDefault() : null;

static void DumpMembers(string label, object value)
{
	Console.WriteLine($"SHAPE|{label}|{value.GetType().FullName}");
	foreach (PropertyInfo property in value.GetType().GetProperties(BindingFlags.Instance | BindingFlags.Public))
	{
		object? memberValue = property.GetIndexParameters().Length == 0 ? property.GetValue(value) : null;
		string count = memberValue is ICollection collection ? collection.Count.ToString() : "-";
		Console.WriteLine($"MEMBER|{label}|{property.Name}|{property.PropertyType.FullName}|{count}");
	}
}

static string OneLine(string value) => value.Replace('\r', ' ').Replace('\n', ' ');

// An empty serialized file at the requested version gives a real collection whose
// Version/Platform/EndianType facts the factory consults, without needing any payload.
static SerializedAssetCollection EmptyCollection(
	UnityVersion version,
	EndianType endianType = EndianType.LittleEndian,
	BuildTarget platform = (BuildTarget)19)
{
	SerializedFile file = new SerializedFileBuilder
	{
		Generation = FormatVersion.Unknown_10,
		Version = version,
		Platform = platform,
		EndianType = endianType,
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

static void DumpFactoryDispatchMatrix(string outputPath)
{
	TpkTypeTreeBlob blob = (TpkTypeTreeBlob)TpkFile.FromStream(SourceTpk.GetStream()).GetDataBlob();
	SortedSet<UnityVersion> queryVersions = new() { UnityVersion.Parse("0.0.0f0") };
	queryVersions.UnionWith(blob.Versions);
	// The package-wide version list omits some per-class boundary versions. The factory can
	// switch generated classes at those boundaries, so include them in the dispatch matrix too.
	queryVersions.UnionWith(blob.ClassInformation.SelectMany(static item => item.Classes.Select(static entry => entry.Key)));
	int[] classIds = Enum.GetValues<ClassIDType>()
		.Select(static value => (int)value)
		.Concat(blob.ClassInformation.Select(static item => item.ID))
		.Distinct()
		.OrderBy(static value => value)
		.ToArray();
	string fullPath = Path.GetFullPath(outputPath);
	Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
	using StreamWriter writer = new(fullPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
	writer.WriteLine($"FACTORY-DISPATCH|1|{classIds.Length}|{queryVersions.Count}|{(long)classIds.Length * queryVersions.Count}");
	foreach (int classId in classIds)
	{
		writer.WriteLine($"ID|{classId}");
	}
	foreach (UnityVersion version in queryVersions)
	{
		writer.WriteLine($"VERSION|{version}|{version.GetBits():X16}");
	}
	Dictionary<int, string> previous = new();
	int transitionCount = 0;
	foreach (UnityVersion version in queryVersions)
	{
		SerializedAssetCollection collection = EmptyCollection(version);
		foreach (int classId in classIds)
		{
			string name = GeneratedName(collection, classId, version).Replace('|', ' ');
			if (previous.TryGetValue(classId, out string? oldName) && oldName == name)
			{
				continue;
			}
			writer.WriteLine($"GEN|{classId}|{version}|{version.GetBits():X16}|{name}");
			previous[classId] = name;
			transitionCount++;
		}
	}
	writer.WriteLine($"TRANSITIONS|{transitionCount}");
}

static void DumpGeneratedPayloadMatrix(string outputPath)
{
	TpkTypeTreeBlob blob = (TpkTypeTreeBlob)TpkFile.FromStream(SourceTpk.GetStream()).GetDataBlob();
	string fullPath = Path.GetFullPath(outputPath);
	Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
	using StreamWriter writer = new(fullPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
	writer.WriteLine("# Release bytes written by the frozen generated AssetRipper classes at each non-null TPK class-version boundary.");
	writer.WriteLine("# PAYLOAD|class-id|version|factory-type|mode|byte-length|bytes-hex");
	Dictionary<string, SerializedAssetCollection> collections = new(StringComparer.Ordinal);
	int generated = 0;
	int skipped = 0;
	int failed = 0;
	long totalBytes = 0;
	foreach (TpkClassInformation classInformation in blob.ClassInformation.OrderBy(item => item.ID))
	{
		foreach (var entry in classInformation.Classes)
		{
			if (entry.Value is null)
			{
				continue;
			}
			UnityVersion version = entry.Key;
			string versionText = version.ToString();
			if (!collections.TryGetValue(versionText, out SerializedAssetCollection? collection))
			{
				collection = EmptyCollection(version);
				collections.Add(versionText, collection);
			}
			IUnityObjectBase? asset;
			try
			{
				asset = AssetFactory.CreateSerialized(new AssetInfo(collection, 42, classInformation.ID), version);
			}
			catch (Exception exception)
			{
				writer.WriteLine($"SKIP|{classInformation.ID}|{versionText}|THROW:{exception.GetType().Name}");
				skipped++;
				continue;
			}
			if (asset is null)
			{
				writer.WriteLine($"SKIP|{classInformation.ID}|{versionText}|MISSING");
				skipped++;
				continue;
			}
			foreach (string mode in new[] { "release", "editor" })
			{
				try
				{
					using MemoryStream stream = new();
					using (AssetWriter assetWriter = new(stream, asset.Collection))
					{
						if (mode == "release")
						{
							asset.WriteRelease(assetWriter);
						}
						else
						{
							asset.WriteEditor(assetWriter);
						}
					}
					byte[] bytes = stream.ToArray();
					writer.WriteLine($"PAYLOAD|{classInformation.ID}|{versionText}|{asset.GetType().Name}|{mode}|{bytes.Length}|{Convert.ToHexString(bytes)}");
					generated++;
					totalBytes += bytes.Length;
				}
				catch (Exception exception)
				{
					writer.WriteLine($"FAIL|{classInformation.ID}|{versionText}|{asset.GetType().Name}|{mode}|{exception.GetType().FullName}|{OneLine(exception.Message)}");
					failed++;
				}
			}
		}
	}
	Console.WriteLine($"PAYLOAD_MATRIX|generated={generated}|skipped={skipped}|failed={failed}|bytes={totalBytes}|versions={collections.Count}");
}

static void DumpGeneratedWorkspaceMatrix(string outputPath, int? maximumBoundaries)
{
	TpkTypeTreeBlob blob = (TpkTypeTreeBlob)TpkFile.FromStream(SourceTpk.GetStream()).GetDataBlob();
	string scratch = Path.GetFullPath(Path.Combine(Environment.CurrentDirectory, "target", "ser-factory-workspace-probe"));
	Directory.CreateDirectory(scratch);
	Dictionary<(string Version, bool Editor), List<(int ClassId, string WrittenType, byte[] Payload)>> groups = new();
	Dictionary<string, SerializedAssetCollection> collections = new(StringComparer.Ordinal);
	int boundaries = 0;
	int skipped = 0;
	int failed = 0;
	long payloadBytes = 0;
	List<string> skipRecords = [];

	foreach (TpkClassInformation classInformation in blob.ClassInformation.OrderBy(item => item.ID))
	{
		foreach (var entry in classInformation.Classes)
		{
			if (entry.Value is null)
			{
				continue;
			}
			if (maximumBoundaries.HasValue && boundaries >= maximumBoundaries.Value)
			{
				goto WriteGroups;
			}
			boundaries++;
			UnityVersion version = entry.Key;
			string versionText = version.ToString();
			foreach (bool editor in new[] { false, true })
			{
				BuildTarget platform = editor ? BuildTarget.NoTarget : (BuildTarget)19;
				SerializedAssetCollection collection = EmptyCollection(version, platform: platform);
				IUnityObjectBase? asset;
				try
				{
					asset = AssetFactory.CreateSerialized(new AssetInfo(collection, 42, classInformation.ID), version);
				}
				catch (Exception exception)
				{
					skipped++;
					skipRecords.Add($"SKIP|{classInformation.ID}|{versionText}|{(editor ? "editor" : "release")}|THROW:{exception.GetType().Name}|{OneLine(exception.Message)}");
					continue;
				}
				if (asset is null)
				{
					skipped++;
					skipRecords.Add($"SKIP|{classInformation.ID}|{versionText}|{(editor ? "editor" : "release")}|MISSING");
					continue;
				}

				try
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
					var key = (versionText, editor);
					if (!groups.TryGetValue(key, out List<(int ClassId, string WrittenType, byte[] Payload)>? objects))
					{
						objects = [];
						groups.Add(key, objects);
					}
					objects.Add((classInformation.ID, asset.GetType().Name, stream.ToArray()));
					payloadBytes += stream.Length;
			}
				catch (Exception exception)
				{
					failed++;
					throw new InvalidOperationException(
						$"Source writer failed for class {classInformation.ID} at {versionText} in {(editor ? "editor" : "release")} mode",
						exception);
				}
			}
		}
	}

WriteGroups:
	var orderedGroups = groups
		.OrderBy(pair => UnityVersion.Parse(pair.Key.Version))
		.ThenBy(pair => pair.Key.Editor)
		.ToArray();
	string fullPath = Path.GetFullPath(outputPath);
	Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
	long serializedFileBytes = 0;
	int materializedObjects = 0;
	using (FileStream fileStream = File.Create(fullPath))
	using (GZipStream gzip = new(fileStream, CompressionLevel.SmallestSize))
	using (BinaryWriter fixture = new(gzip, Encoding.UTF8, leaveOpen: false))
	{
		fixture.Write(Encoding.ASCII.GetBytes("ARWFAC01"));
		fixture.Write(orderedGroups.Length);
		for (int groupIndex = 0; groupIndex < orderedGroups.Length; groupIndex++)
		{
			var group = orderedGroups[groupIndex];
			string fileName = $"factory_{groupIndex:D4}_{(group.Key.Editor ? "editor" : "release")}{(group.Key.Editor ? ".unity" : ".assets")}";
			BuildTarget platform = group.Key.Editor ? BuildTarget.NoTarget : (BuildTarget)19;
			SerializedFileBuilder builder = new()
			{
				Generation = FormatVersion.Unknown_10,
				Version = UnityVersion.Parse(group.Key.Version),
				Platform = platform,
				EndianType = EndianType.LittleEndian,
				HasTypeTree = false,
			};
			for (int objectIndex = 0; objectIndex < group.Value.Count; objectIndex++)
			{
				var sourceObject = group.Value[objectIndex];
				SerializedType type = new() { RawTypeID = sourceObject.ClassId };
				builder.Types.Add(type);
				builder.Objects.Add(new ObjectInfo(type)
				{
					FileID = objectIndex + 1,
					TypeID = sourceObject.ClassId,
					ObjectData = sourceObject.Payload,
				});
			}
			SerializedFile generatedFile = builder.Build();
			generatedFile.FilePath = fileName;
			byte[] serializedFile = generatedFile.ToByteArray();
			serializedFileBytes += serializedFile.Length;
			string scratchPath = Path.Combine(scratch, fileName);
			File.WriteAllBytes(scratchPath, serializedFile);
			using FileBase loaded = SchemeReader.LoadFile(scratchPath, LocalFileSystem.Instance);
			if (loaded is not SerializedFile loadedSerializedFile)
			{
				throw new InvalidOperationException($"{fileName} reloaded as {loaded.GetType().Name}");
			}
			GameAssetFactory factory = new(new BaseManager(_ => { }));
			SerializedAssetCollection collection = new GameBundle().AddCollectionFromSerializedFile(loadedSerializedFile, factory);
			if (collection.Count != group.Value.Count)
			{
				throw new InvalidOperationException($"{fileName} source factory returned {collection.Count} assets for {group.Value.Count} objects");
			}
			Dictionary<long, IUnityObjectBase> assetsByPathId = collection.ToDictionary(asset => asset.PathID);

			WriteFixtureString(fixture, fileName);
			WriteFixtureString(fixture, group.Key.Version);
			fixture.Write((byte)(group.Key.Editor ? 1 : 0));
			fixture.Write(serializedFile.Length);
			fixture.Write(serializedFile);
			fixture.Write(group.Value.Count);
			for (int objectIndex = 0; objectIndex < group.Value.Count; objectIndex++)
			{
				var sourceObject = group.Value[objectIndex];
				long pathId = objectIndex + 1;
				if (!assetsByPathId.TryGetValue(pathId, out IUnityObjectBase? asset))
				{
					throw new InvalidOperationException($"{fileName} source factory omitted PathID {pathId}");
				}
				fixture.Write(sourceObject.ClassId);
				fixture.Write(pathId);
				WriteFixtureString(fixture, asset.GetType().Name);
				WriteFixtureString(fixture, NameOf(asset));
				object? structure = GetProperty(asset, "Structure");
				WriteFixtureString(fixture, structure?.GetType().Name ?? string.Empty);
		fixture.Write(sourceObject.Payload.Length);
				materializedObjects++;
			}
		}
	}
	File.WriteAllLines(fullPath + ".skips.txt", skipRecords, new UTF8Encoding(false));
	Console.WriteLine($"WORKSPACE_MATRIX|boundaries={boundaries}|payloads={materializedObjects}|groups={orderedGroups.Length}|serialized_bytes={serializedFileBytes}|payload_bytes={payloadBytes}|skipped={skipped}|failed={failed}");
}

static void WriteFixtureString(BinaryWriter writer, string value)
{
	byte[] bytes = Encoding.UTF8.GetBytes(value);
	writer.Write(bytes.Length);
	writer.Write(bytes);
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
		"textasset_notree_2022_3",
		"textasset_tree",
		"textasset_padding",
		"textasset_bad_tail",
		"unknown_class",
		"unknown_no_tree",
		"tpk_navmesh",
		"tpk_navmesh_2022_3",
		"player_settings_notree_2022_3",
		"unreadable_version",
		"unreadable_no_tree",
		"engine_default_resources",
		"mono_unloaded",
		"mono_unloaded_valid",
		"mono_with_structure",
		"monoscript_notree",
		"tpk_navmesh_editor",
		"gameobject_4_alpha_notree",
		"texture_chinese_tail_le",
		"texture_chinese_tail_be",
		"engine_default_resources_shader",
		"engine_default_resources_underscore_shader",
		"engine_builtin_extra_shader",
		"engine_builtin_extra_underscore_shader",
		"patch_retry_light",
		"patch_retry_light_failed",
		"patch_retry_graphics_settings_failed",
		"patch_retry_font_failed",
		"patch_retry_light_editor",
	];
	string scratch = Path.GetFullPath(Path.Combine(Environment.CurrentDirectory, "target", "ser-factory-probe"));
	Directory.CreateDirectory(scratch);
	Logger.Add(new CaptureLogger());
	using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false));
	rows.WriteLine("# Reference GameAssetFactory differential (probe tools/probes/ser-factory, .NET SDK 10.0.401).");
	rows.WriteLine("# In expected log text, `␠` encodes a trailing ASCII space so source messages remain whitespace-clean.");
	rows.WriteLine("# LOG rows omit runtime-specific exception stack frames; exception type/message and input context remain.");
	rows.WriteLine("# GEN rows: AssetFactory.CreateSerialized(new AssetInfo(collection,42,classId), 2019.4.40f1)");
	rows.WriteLine("# on an empty real SerializedAssetCollection, plus the TypeTreeNodeStruct.TryMakeFromTpk outcome.");
	rows.WriteLine("# TPK rows: the real TPK TypeTree nodes used by the CreateAsset fallback.");
	rows.WriteLine("# SF_HEX rows: the authored SerializedFiles; SF_READ rows: the shipped factory materialization");
	rows.WriteLine("# of those exact bytes reloaded through SchemeReader using each case's platform/path facts.");
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
	WriteTpk(rows, 194, UnityVersion.Parse("2022.3.22f1"));
	foreach (string name in cases)
	{
		(string fileName, SerializedFile file) = BuildCase(name);
		byte[] bytes = file.ToByteArray();
		rows.WriteLine($"SF_HEX|{name}|{Convert.ToHexString(bytes)}");
		string caseDirectory = Path.Combine(scratch, name);
		Directory.CreateDirectory(caseDirectory);
		string path = Path.Combine(caseDirectory, fileName);
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
		case "textasset_notree_2022_3":
			return ("textasset_notree_2022_3.assets", BuildTextAssetFile("2022.3.22f1", "Hero", "print(1)", null, []));
		case "textasset_tree":
			return ("textasset_tree.assets", BuildTextAssetFile("2019.4.40f1", "Hero", "print(1)", TextAssetReleaseTree(), []));
		case "textasset_padding":
			return ("textasset_padding.assets", BuildTextAssetFile("2019.4.40f1", "Hero", "print(1)", null, [0, 0, 0, 0]));
		case "textasset_bad_tail":
			return ("textasset_bad_tail.assets", BuildTextAssetFile("2019.4.40f1", "Hero", "print(1)", null, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE]));
		case "unknown_class":
			return ("unknown_class.assets", BuildFile("unknown_class.assets", "2019.4.40f1", 7, null, [1, 2, 3, 4, 5, 6, 7, 8]));
		case "unknown_no_tree":
			return ("unknown_no_tree.assets", BuildFile("unknown_no_tree.assets", "2019.4.40f1", 9999, null, [1, 2, 3, 4, 5, 6, 7, 8]));
		case "tpk_navmesh":
			return ("tpk_navmesh.assets", BuildFile("tpk_navmesh.assets", "2019.4.40f1", 194, null, StringPayload("NavMeshData")));
		case "tpk_navmesh_2022_3":
			return ("tpk_navmesh_2022_3.assets", BuildFile("tpk_navmesh_2022_3.assets", "2022.3.22f1", 194, null, StringPayload("NavMeshData")));
		case "player_settings_notree_2022_3":
			return ("player_settings_notree_2022_3.assets", BuildFile("player_settings_notree_2022_3.assets", "2022.3.22f1", 129, null, BuildDefaultTpkPayload(129, "2022.3.22f1")));
		case "tpk_navmesh_editor":
			return ("navmesh.unity", BuildNavMeshEditorFile());
		case "unreadable_version":
			return ("unreadable_version.assets", BuildTextAssetFile("3.4.0f1", "Hero", "print(1)", null, []));
		case "unreadable_no_tree":
			return ("unreadable_no_tree.assets", BuildFile("unreadable_no_tree.assets", "3.4.0f1", 9999, null, [1, 2, 3, 4, 5, 6, 7, 8]));
		case "engine_default_resources":
			return ("unity default resources", BuildTextAssetFile("2019.4.40f1", "Hero", "print(1)", null, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE]));
		case "mono_unloaded":
			return ("mono_unloaded.assets", BuildMonoBehaviourFile());
		case "mono_unloaded_valid":
			return ("mono_unloaded_valid.assets", BuildMonoBehaviourWithValidUnloadedStructureFile());
		case "mono_with_structure":
			return ("mono_with_structure.assets", BuildMonoBehaviourWithStructureFile());
		case "monoscript_notree":
			return ("monoscript_notree.assets", BuildMonoScriptFile());
		case "gameobject_4_alpha_notree":
		{
			UnityVersion version = UnityVersion.Parse("4.0.0a0");
			SerializedAssetCollection collection = EmptyCollection(version);
			IUnityObjectBase asset = AssetFactory.CreateSerialized(new AssetInfo(collection, 42, 1), version)
				?? throw new InvalidOperationException("GameObject factory returned null");
			using MemoryStream stream = new();
			using (AssetWriter writer = new(stream, asset.Collection))
			{
				asset.WriteRelease(writer);
			}
			return ("gameobject_4_alpha_notree.assets", BuildFile("gameobject_4_alpha_notree.assets", version.ToString(), 1, null, stream.ToArray()));
		}
		case "texture_chinese_tail_le":
			return ("texture_chinese_tail_le.assets", BuildChineseTextureFile("texture_chinese_tail_le.assets", EndianType.LittleEndian));
		case "texture_chinese_tail_be":
			return ("texture_chinese_tail_be.assets", BuildChineseTextureFile("texture_chinese_tail_be.assets", EndianType.BigEndian));
		case "engine_default_resources_shader":
			return ("unity default resources", BuildEngineShaderFile("unity default resources"));
		case "engine_default_resources_underscore_shader":
			return ("unity_default_resources", BuildEngineShaderFile("unity_default_resources"));
		case "engine_builtin_extra_shader":
			return ("unity builtin extra", BuildEngineShaderFile("unity builtin extra"));
		case "engine_builtin_extra_underscore_shader":
			return ("unity_builtin_extra", BuildEngineShaderFile("unity_builtin_extra"));
		case "patch_retry_light":
			return ("patch_retry_light.assets", BuildPatchRetryFile("patch_retry_light.assets", "2017.1.0p0", "2017.1.1f0", 108));
		case "patch_retry_light_failed":
			return ("patch_retry_light_failed.assets", BuildPatchRetryFile("patch_retry_light_failed.assets", "2017.1.0p0", "2017.1.1f0", 108, true));
		case "patch_retry_graphics_settings_failed":
			return ("patch_retry_graphics_settings_failed.assets", BuildPatchRetryFile("patch_retry_graphics_settings_failed.assets", "2017.1.0p0", "2017.1.1f0", 30, true));
		case "patch_retry_font_failed":
			return ("patch_retry_font_failed.assets", BuildPatchRetryFile("patch_retry_font_failed.assets", "2017.1.1p0", "2017.1.2f0", 128, true));
		case "patch_retry_light_editor":
			return ("patch_retry_light_editor.unity", BuildPatchRetryFile("patch_retry_light_editor.unity", "2017.1.0p0", "2017.1.1f0", 108, editorMode: true));
		default:
			throw new ArgumentException($"unknown case {name}");
	}
}

static SerializedFile BuildChineseTextureFile(string fileName, EndianType endianType)
{
	UnityVersion version = UnityVersion.Parse("2019.4.40f1");
	SerializedAssetCollection collection = EmptyCollection(version, endianType);
	IUnityObjectBase texture = AssetFactory.CreateSerialized(new AssetInfo(collection, 42, 28), version)
		?? throw new InvalidOperationException("Texture2D factory returned null");
	PropertyInfo nameProperty = texture.GetType().GetProperty("Name", BindingFlags.Public | BindingFlags.Instance)
		?? throw new InvalidOperationException("generated Texture2D has no Name property");
	nameProperty.SetValue(texture, new Utf8String("Logo"));
	using MemoryStream stream = new();
	using (AssetWriter writer = new(stream, texture.Collection))
	{
		texture.WriteRelease(writer);
	}
	List<byte> payload = [.. stream.ToArray()];
	WriteI32(payload, 2048, endianType);
	WriteI32(payload, 1024, endianType);
	WriteI32(payload, unchecked((int)0x11223344), endianType);
	WriteI32(payload, unchecked((int)0x55667788), endianType);
	WriteI32(payload, unchecked((int)0x90ABCDEF), endianType);
	WriteI32(payload, 0x01234567, endianType);
	return BuildFile(fileName, version.ToString(), 28, null, payload.ToArray(), endianType: endianType);
}

static SerializedFile BuildEngineShaderFile(string collectionName)
{
	UnityVersion version = UnityVersion.Parse("2019.4.40f1");
	SerializedAssetCollection collection = EmptyCollection(version);
	IUnityObjectBase shader = AssetFactory.CreateSerialized(new AssetInfo(collection, 42, 48), version)
		?? throw new InvalidOperationException("Shader factory returned null");
	using MemoryStream stream = new();
	using (AssetWriter writer = new(stream, shader.Collection))
	{
		shader.WriteRelease(writer);
	}
	List<byte> payload = [.. stream.ToArray(), 0xAA, 0xBB, 0xCC, 0xDD];
	return BuildFile(collectionName, version.ToString(), 48, null, payload.ToArray());
}

static SerializedFile BuildPatchRetryFile(string fileName, string serializedVersionText, string payloadVersionText, int classId, bool truncatePayload = false, bool editorMode = false)
{
	UnityVersion payloadVersion = UnityVersion.Parse(payloadVersionText);
	SerializedAssetCollection collection = EmptyCollection(payloadVersion);
	IUnityObjectBase asset = AssetFactory.CreateSerialized(new AssetInfo(collection, 42, classId), payloadVersion)
		?? throw new InvalidOperationException($"factory returned null for class {classId} at {payloadVersion}");
	using MemoryStream stream = new();
	using (AssetWriter writer = new(stream, asset.Collection))
	{
		if (editorMode)
		{
			asset.WriteEditor(writer);
		}
		else
		{
			asset.WriteRelease(writer);
		}
	}
	byte[] payload = stream.ToArray();
	if (truncatePayload)
	{
		Array.Resize(ref payload, payload.Length - 1);
	}
	return BuildFile(fileName, serializedVersionText, classId, null, payload, editorMode ? BuildTarget.NoTarget : (BuildTarget)19);
}

static SerializedFile BuildTextAssetFile(string versionText, string name, string script, TypeTree? tree, byte[] tail)
{
	List<byte> payload = [];
	WriteString(payload, name);
	WriteString(payload, script);
	payload.AddRange(tail);
	return BuildFile("x.assets", versionText, 49, tree, payload.ToArray());
}

static SerializedFile BuildNavMeshEditorFile()
{
	List<byte> payload = [];
	WriteI32(payload, 3); // m_ObjectHideFlags
	WriteI32(payload, 0); // m_PrefabParentObject.m_FileID
	WriteI64(payload, 0); // m_PrefabParentObject.m_PathID
	WriteI32(payload, 0); // m_PrefabInternal.m_FileID
	WriteI64(payload, 0); // m_PrefabInternal.m_PathID
	WriteString(payload, "SceneNavMesh");
	return BuildFile("navmesh.unity", "2019.4.40f1", 194, null, payload.ToArray(), BuildTarget.NoTarget);
}

static SerializedFile BuildFile(string fileName, string versionText, int classId, TypeTree? tree, byte[] payload, BuildTarget platform = (BuildTarget)19, EndianType endianType = EndianType.LittleEndian)
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
		Platform = platform,
		EndianType = endianType,
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

// Fixture-only writer for a zero-valued TPK object. Empty arrays/strings use a zero
// count, primitives use their zero representation, and every TypeTree AlignBytes flag
// is honored. This creates a complete source-readable PlayerSettings payload without
// relying on private Unity project data.
static byte[] BuildDefaultTpkPayload(int classId, string versionText)
{
	UnityVersion version = UnityVersion.Parse(versionText);
	if (!TypeTreeNodeStruct.TryMakeFromTpk((ClassIDType)classId, version, out TypeTreeNodeStruct release, out _))
	{
		throw new InvalidOperationException($"No release TPK tree for class {classId} at {version}");
	}
	List<byte> bytes = [];
	WriteDefaultTpkNode(release, bytes);
	return bytes.ToArray();
}

static void WriteDefaultTpkNode(TypeTreeNodeStruct node, List<byte> bytes)
{
	if (node.IsArray || node.IsTypelessData || node.IsString || node.IsVector || node.IsNamedVector || node.IsMap)
	{
		WriteI32(bytes, 0);
		if (node.AlignBytes || (node.SubNodes.Count > 0 && node[0].AlignBytes))
		{
			AlignDefaultTpkPayload(bytes);
		}
		return;
	}
	if (node.SubNodes.Count > 0)
	{
		foreach (TypeTreeNodeStruct child in node.SubNodes)
		{
			WriteDefaultTpkNode(child, bytes);
		}
	}
	else
	{
		int size = node.TypeName switch
		{
			"bool" or "char" or "UInt8" or "SInt8" => 1,
			"short" or "SInt16" or "ushort" or "UInt16" or "unsigned short" => 2,
			"int" or "SInt32" or "Type*" or "uint" or "UInt32" or "unsigned int" or "float" => 4,
			"SInt64" or "long long" or "UInt64" or "FileSize" or "unsigned long long" or "double" => 8,
			"None" or "void" => 0,
			_ => throw new InvalidOperationException($"No zero encoding for TPK leaf {node.TypeName}/{node.Name}"),
		};
		bytes.AddRange(new byte[size]);
	}
	if (node.AlignBytes)
	{
		AlignDefaultTpkPayload(bytes);
	}
}

static void AlignDefaultTpkPayload(List<byte> bytes)
{
	while (bytes.Count % 4 != 0)
	{
		bytes.Add(0);
	}
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
	return BuildMonoBehaviourFileWithRemainder("mono_unloaded.assets", [0xDE, 0xAD, 0xBE, 0xEF], "VRC.SDK");
}

// A source-authored UnloadedStructure whose bytes follow the fixture assembly's
// inherited long, int, and float fields exactly.
static SerializedFile BuildMonoBehaviourWithValidUnloadedStructureFile()
{
	List<byte> structure = [];
	WriteI64(structure, 0x0102030405060708);
	WriteI32(structure, 1234);
	WriteI32(structure, BitConverter.SingleToInt32Bits(1.5f));
	return BuildMonoBehaviourFileWithRemainder("mono_unloaded_valid.assets", structure.ToArray(), "Game");
}

static SerializedFile BuildMonoBehaviourFileWithRemainder(
	string fileName,
	byte[] unloadedStructure,
	string scriptNamespace)
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
	payload.AddRange(unloadedStructure);
	SerializedType monoBehaviourType = new() { RawTypeID = 114 };
	SerializedType monoScriptType = new() { RawTypeID = 115 };
	byte[] monoScriptPayload = BuildMonoScriptFileWithNamespace(scriptNamespace).Objects[0].ObjectData;
	SerializedFile file = new SerializedFileBuilder
	{
		Generation = FormatVersion.Unknown_10,
		Version = UnityVersion.Parse("2019.4.40f1"),
		Platform = (BuildTarget)19,
		EndianType = EndianType.LittleEndian,
		HasTypeTree = false,
		Types = { monoBehaviourType, monoScriptType },
		Objects =
		{
			new ObjectInfo(monoBehaviourType)
			{
				FileID = 42,
				TypeID = 114,
				ObjectData = payload.ToArray(),
			},
			new ObjectInfo(monoScriptType)
			{
				FileID = 7,
				TypeID = 115,
				ObjectData = monoScriptPayload,
			},
		},
	}.Build();
	file.FilePath = fileName;
	return file;
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
	return BuildMonoScriptFileWithNamespace("VRC.SDK");
}

static SerializedFile BuildMonoScriptFileWithNamespace(string scriptNamespace)
{
	List<byte> payload = [];
	WriteString(payload, "HeroController");
	WriteI32(payload, 200);
	for (int i = 0; i < 16; i++)
	{
		payload.Add((byte)i);
	}
	WriteString(payload, "HeroController");
	WriteString(payload, scriptNamespace);
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

static void WriteI32(List<byte> payload, int value, EndianType endianType = EndianType.LittleEndian)
{
	byte[] bytes = BitConverter.GetBytes(value);
	if (BitConverter.IsLittleEndian != (endianType == EndianType.LittleEndian))
	{
		Array.Reverse(bytes);
	}
	payload.AddRange(bytes);
}

static void WriteI64(List<byte> payload, long value, EndianType endianType = EndianType.LittleEndian)
{
	byte[] bytes = BitConverter.GetBytes(value);
	if (BitConverter.IsLittleEndian != (endianType == EndianType.LittleEndian))
	{
		Array.Reverse(bytes);
	}
	payload.AddRange(bytes);
}

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
			rows.WriteLine($"SF_READ|{name}|LOG|{PreserveTrailingSpaces(message)}");
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
			case (int)ClassIDType.Texture2D:
				DumpChineseTextureFields(rows, name, asset);
				break;
		}
		if (asset is TypeTreeObject)
		{
			using MemoryStream editorStream = new();
			using (AssetWriter writer = new(editorStream, asset.Collection))
			{
				asset.WriteEditor(writer);
			}
			rows.WriteLine($"SF_READ|{name}|EDITOR_HEX|{Convert.ToHexString(editorStream.ToArray())}");
			using MemoryStream releaseStream = new();
			using (AssetWriter writer = new(releaseStream, asset.Collection))
			{
				asset.WriteRelease(writer);
			}
			rows.WriteLine($"SF_READ|{name}|RELEASE_HEX|{Convert.ToHexString(releaseStream.ToArray())}");
		}
	}
	foreach (string message in CaptureLogger.Messages)
	{
		rows.WriteLine($"SF_READ|{name}|LOG|{PreserveTrailingSpaces(message)}");
	}
}

static string PreserveTrailingSpaces(string value)
{
	int end = value.Length;
	while (end > 0 && value[end - 1] == ' ')
	{
		end--;
	}
	return end == value.Length ? value : value[..end] + new string('␠', value.Length - end);
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

static void DumpChineseTextureFields(StreamWriter rows, string name, object asset)
{
	object? guid = GetProperty(asset, "OriginalAssetGuid_C28");
	rows.WriteLine($"SF_READ|{name}|CHINESE_TEXTURE|{GetProperty(asset, "OriginalWidth_C28")}|{GetProperty(asset, "OriginalHeight_C28")}|{GetProperty(guid, "Data_0_")},{GetProperty(guid, "Data_1_")},{GetProperty(guid, "Data_2_")},{GetProperty(guid, "Data_3_")}");
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
		string normalized = string.Join(' ', message.Replace("\r\n", "\n").Split('\n')
			.Where(static line => !line.TrimStart().StartsWith("at ", StringComparison.Ordinal)));
		Messages.Add($"{type}|{category}|{normalized.Replace('\r', ' ').Replace('\n', ' ')}");
	}

	public void BlankLine(int numLines)
	{
	}
}
