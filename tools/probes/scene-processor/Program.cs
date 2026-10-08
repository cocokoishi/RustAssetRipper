// Isolated reference probe for the Scene/Prefab processor sequence.
//
// Mode tpk <version> <classId...>: dump the real TPK TypeTree node lists (release+editor) so the
//   Rust fixture builder can reproduce the frozen layouts byte-exactly.
// Mode process <rows> <file...>: load each SerializedFile through the shipped factory into one
//   GameBundle, then run the real SceneDefinitionProcessor and PrefabProcessor in source order
//   and dump the observable outcome (collections, scene definitions, generated hierarchies,
//   generated Transform objects). Writes only the rows file; reads only inputs.
using System.Collections;
using System.Reflection;
using AssetRipper.Assets;
using AssetRipper.Assets.Bundles;
using AssetRipper.Assets.Collections;
using AssetRipper.Import.AssetCreation;
using AssetRipper.Import.Logging;
using AssetRipper.Import.Structure.Assembly.Managers;
using AssetRipper.Import.Structure.Assembly.TypeTrees;
using AssetRipper.IO.Files;
using AssetRipper.IO.Files.SerializedFiles;
using AssetRipper.Primitives;
using AssetRipper.Processing;
using AssetRipper.Processing.Prefabs;
using AssetRipper.Processing.Scenes;
using AssetRipper.SourceGenerated;
using AssetRipper.SourceGenerated.Classes.ClassID_142;
using AssetRipper.SourceGenerated.Extensions;

if (args.Length == 0)
{
	Console.Error.WriteLine("usage: tpk <version> <classId...> | naming <version> <name...> | process <rows> <file...>");
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
	if (args[0] == "process" && args.Length >= 3)
	{
		Process(args[1], args[2..]);
		return 0;
	}
	Console.Error.WriteLine("usage: tpk <version> <classId...> | naming <version> <name...> | process <rows> <file...>");
	return 2;
}
catch (Exception ex)
{
	Console.Error.WriteLine($"PROBE_FAIL|{ex.GetType().FullName}|{OneLine(ex.Message)}");
	return 1;
}

static string OneLine(string value) => value.Replace('\r', ' ').Replace('\n', ' ');

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
