// Isolated reference probe for BuildSettings (class 141) layouts and values.
// Mode tpk <version...>: dump the real TPK TypeTree node lists (release+editor).
// Mode materialize <file> <rows>: load a SerializedFile through the shipped
// factory and dump the materialized BuildSettings scene paths plus a bounded
// reflection view of the object. Writes only the rows file; reads only inputs.
using AssetRipper.Assets;
using AssetRipper.Assets.Bundles;
using AssetRipper.Assets.Collections;
using AssetRipper.Import.AssetCreation;
using AssetRipper.Import.Structure.Assembly.Managers;
using AssetRipper.Import.Structure.Assembly.TypeTrees;
using AssetRipper.IO.Files;
using AssetRipper.IO.Files.SerializedFiles;
using AssetRipper.Primitives;
using AssetRipper.SourceGenerated;

if (args.Length == 0)
{
	Console.Error.WriteLine("usage: tpk <version...> | materialize <file> <rows>");
	return 2;
}

try
{
	if (args[0] == "tpk")
	{
		DumpTpk(args[1..]);
		return 0;
	}
	if (args[0] == "materialize" && args.Length == 3)
	{
		Materialize(args[1], args[2]);
		return 0;
	}
	Console.Error.WriteLine("usage: tpk <version...> | materialize <file> <rows>");
	return 2;
}
catch (Exception ex)
{
	Console.Error.WriteLine($"PROBE_FAIL|{ex.GetType().FullName}|{OneLine(ex.Message)}");
	return 1;
}

static string OneLine(string value) => value.Replace('\r', ' ').Replace('\n', ' ');

static void DumpTpk(string[] versions)
{
	foreach (string versionText in versions)
	{
		UnityVersion version = UnityVersion.Parse(versionText);
		if (TypeTreeNodeStruct.TryMakeFromTpk(ClassIDType.BuildSettings, version, out TypeTreeNodeStruct release, out TypeTreeNodeStruct editor))
		{
			Console.WriteLine($"TPK|{versionText}|release");
			DumpNodes(release, 0);
			Console.WriteLine($"TPK|{versionText}|editor");
			DumpNodes(editor, 0);
		}
		else
		{
			Console.WriteLine($"TPK|{versionText}|MISSING");
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

static void Materialize(string path, string rowsPath)
{
	GameAssetFactory factory = new GameAssetFactory(new BaseManager(s => { }));
	FileBase file = SchemeReader.LoadFile(path, LocalFileSystem.Instance);
	if (file is not SerializedFile serializedFile)
	{
		File.WriteAllText(rowsPath, $"KIND|{file.GetType().Name}\n");
		return;
	}
	GameBundle bundle = new();
	SerializedAssetCollection collection = bundle.AddCollectionFromSerializedFile(serializedFile, factory);
	bundle.InitializeAllDependencyLists();
	using StreamWriter rows = new StreamWriter(rowsPath, false, System.Text.Encoding.UTF8);
	rows.WriteLine($"FILE|{serializedFile.NameFixed}|version={serializedFile.Version}|assets={collection.Count}");
	foreach (IUnityObjectBase asset in collection)
	{
		rows.WriteLine($"ASSET|{asset.PathID}|class={asset.ClassID}|type={asset.GetType().Name}");
		if (asset.ClassID == (int)ClassIDType.BuildSettings)
		{
			DumpBuildSettingsFocused(rows, asset);
		}
	}
}

static void DumpBuildSettingsFocused(StreamWriter rows, object asset)
{
	// Scene paths: Scenes/Levels is an enumerable of Utf8String (exposes String).
	object? scenes = GetProperty(asset, "Scenes") ?? GetProperty(asset, "Levels");
	if (scenes is System.Collections.IEnumerable items)
	{
		foreach (object? item in items)
		{
			rows.WriteLine($"SCENE|{GetProperty(item, "String")}");
		}
	}
	else
	{
		rows.WriteLine("SCENE|MISSING");
	}
	// Version and auth token are Utf8String (exposes String/IsEmpty).
	foreach (string name in new[] { "Version", "AuthToken" })
	{
		object? text = GetProperty(asset, name);
		rows.WriteLine($"{name.ToUpperInvariant()}|{GetProperty(text, "String")}");
	}
	// Build GUID words (the generated property is BuildGUID_GUID).
	object? guid = GetProperty(asset, "BuildGUID_GUID") ?? GetProperty(asset, "BuildGUID");
	if (guid is null)
	{
		rows.WriteLine("GUID|ABSENT");
	}
	else
	{
		rows.WriteLine($"GUID|{GetProperty(guid, "Data_0_")},{GetProperty(guid, "Data_1_")},{GetProperty(guid, "Data_2_")},{GetProperty(guid, "Data_3_")}");
	}
	// A sample of scalar flags, proving value fidelity beyond the paths.
	foreach (string name in new[] { "HasShadows", "EnableDynamicBatching", "HasPROVersion", "HasPublishingRights" })
	{
		rows.WriteLine($"BOOL|{name}|{GetProperty(asset, name)}");
	}
}

static object? GetProperty(object? value, string name)
{
	return value?.GetType().GetProperty(name)?.GetValue(value);
}

static void DumpAssetReflective(StreamWriter rows, object value, int depth)
{
	if (depth > 3 || value is null)
	{
		return;
	}
	Type type = value.GetType();
	foreach (System.Reflection.PropertyInfo property in type.GetProperties())
	{
		if (property.GetIndexParameters().Length != 0 || !property.CanRead)
		{
			continue;
		}
		object? propertyValue;
		try
		{
			propertyValue = property.GetValue(value);
		}
		catch
		{
			continue;
		}
		DumpValue(rows, $"{new string('>', depth)}{property.Name}", propertyValue, depth);
	}
}

static void DumpValue(StreamWriter rows, string label, object? propertyValue, int depth)
{
	switch (propertyValue)
	{
		case null:
			rows.WriteLine($"{label}=null");
			break;
		case string text:
			rows.WriteLine($"{label}={text}");
			break;
		case bool flag:
			rows.WriteLine($"{label}={flag}");
			break;
		case int number:
			rows.WriteLine($"{label}={number}");
			break;
		case uint unumber:
			rows.WriteLine($"{label}={unumber}");
			break;
		case long lnumber:
			rows.WriteLine($"{label}={lnumber}");
			break;
		case System.Collections.IEnumerable items when propertyValue is not byte[]:
			int index = 0;
			foreach (object? item in items)
			{
				if (index >= 64)
				{
					rows.WriteLine($"{label}[…]=TRUNCATED");
					break;
				}
				DumpValue(rows, $"{label}[{index}]", item, depth + 1);
				index++;
			}
			if (index == 0)
			{
				rows.WriteLine($"{label}[]=EMPTY");
			}
			break;
		default:
			Type valueType = propertyValue.GetType();
			if (valueType.IsPrimitive || valueType.IsEnum)
			{
				rows.WriteLine($"{label}={propertyValue}");
			}
			else if (depth < 3)
			{
				DumpAssetReflective(rows, propertyValue, depth + 1);
			}
			else
			{
				rows.WriteLine($"{label}=<{valueType.Name}>");
			}
			break;
	}
}
