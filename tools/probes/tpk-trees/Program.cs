// Dumps a class's release TypeTrees from the reference TPK database in the same textual
// shape as the committed Mesh fixture, so a class with no in-repo TypeTree can be given a
// real release-ordered tree for a synthetic SerializedFile fixture.
//
// Usage:
//   dotnet run --project tools/probes/tpk-trees/TpkTreesProbe.csproj -c Release \
//     -p:ReferenceRoot='<workspace>/AssetRipper' -- Texture2D 2019.4.40f1
//
// Format (matches src/resources/mesh_typetree_fixture.txt):
//   TREE|<version>|releaseRoot=<index>
//   NODE|<version>|<level>|<type>|<name>|<byteSize>|<versionField>|<typeFlags>|<metaFlag>|<nodeIndex>|<subNodeCount>
using System.Collections;
using System.Reflection;

if (args.Length < 1)
{
	Console.Error.WriteLine("usage: TpkTreesProbe <ClassName|ClassID> [unityVersion]");
	return 2;
}

string target = args[0];
string versionText = args.Length > 1 ? args[1] : "2019.4.40f1";

AppDomain.CurrentDomain.AssemblyResolve += (_, eventArgs) =>
{
	string simple = new AssemblyName(eventArgs.Name).Name + ".dll";
	string beside = Path.Combine(AppContext.BaseDirectory, simple);
	if (File.Exists(beside))
	{
		return Assembly.LoadFrom(beside);
	}
	string inRef = Path.Combine(AppContext.BaseDirectory, "ref", simple);
	return File.Exists(inRef) ? Assembly.LoadFrom(inRef) : null;
};

string refDirectory = Path.Combine(AppContext.BaseDirectory, "ref");
Assembly tpk = Assembly.LoadFrom(Path.Combine(refDirectory, "AssetRipper.Tpk.dll"));
Assembly sourceGenerated = Assembly.LoadFrom(Path.Combine(refDirectory, "AssetRipper.SourceGenerated.dll"));
Assembly primitives = Assembly.LoadFrom(Path.Combine(refDirectory, "AssetRipper.Primitives.dll"));

Type sourceTpkType = sourceGenerated.GetType("AssetRipper.SourceGenerated.SourceTpk")
	?? throw new InvalidOperationException("SourceTpk type not found");
using Stream stream = (Stream)sourceTpkType
	.GetMethod("GetStream", BindingFlags.Public | BindingFlags.Static)!
	.Invoke(null, null)!;

Type tpkFileType = tpk.GetType("AssetRipper.Tpk.TpkFile")!;
object tpkFile = tpkFileType.GetMethod("FromStream", BindingFlags.Public | BindingFlags.Static)!
	.Invoke(null, [stream])!;
object blob = tpkFileType.GetMethod("GetDataBlob", BindingFlags.Public | BindingFlags.Instance)!
	.Invoke(tpkFile, null)!;

// Resolve the requested class ID: accept a number directly, else look the name up in the
// TypeTree blob's class-information table.
int classId;
if (int.TryParse(target, out int parsed))
{
	classId = parsed;
}
else
{
	classId = -1;
	{
		// `Classes` is a map from Unity version to the class record for that release.
		foreach (object info in (IEnumerable)Get(blob, "ClassInformation")!)
		{
			object? classes = Get(info, "Classes");
			if (classes is not IEnumerable enumerable)
			{
				continue;
			}
			foreach (object? pair in enumerable)
			{
				if (pair is null)
				{
					continue;
				}
				object? record = Get(pair, "Value");
				if (record is null)
				{
					continue;
				}
				string? name = Prop<string>(record, "Name");
				if (Environment.GetEnvironmentVariable("TPK_LIST_CLASSES") is not null)
				{
					Console.WriteLine($"CLASS|{Get(info, "ID")}|{name}|{Get(record, "ReleaseRootNode")}");
					continue;
				}
				if (string.Equals(name, target, StringComparison.OrdinalIgnoreCase))
				{
					classId = Convert.ToInt32(Get(info, "ID"));
					break;
				}			}
			if (classId >= 0)
			{
				break;
			}
		}
	}
	if (classId < 0)
	{
		Console.Error.WriteLine($"class {target} not found in the TPK class table");
		return 3;
	}
}

// The version table gives the index that the release-tree lookup keys on.
Type unityVersionType = primitives.GetType("AssetRipper.Primitives.UnityVersion")!;
object version = unityVersionType
	.GetMethod("Parse", BindingFlags.Public | BindingFlags.Static, [typeof(string)])!
	.Invoke(null, [versionText])!;

object versions = Get(blob, "Versions")!;
int versionIndex = -1;
int index = 0;
foreach (object? candidate in (IEnumerable)versions)
{
	if (candidate is not null && candidate.Equals(version))
	{
		versionIndex = index;
		break;
	}
	index++;
}
if (versionIndex < 0)
{
	Console.Error.WriteLine($"version {versionText} not found in the TPK version table");
	return 4;
}

// Find the release-root node list for (class, version) and walk it recursively.
object? root = null;
if (Environment.GetEnvironmentVariable("TPK_LIST_METHODS") is not null)
{
	// Walk the class record's release root through the node buffer directly.
	object? record = null;
	foreach (object info in (IEnumerable)Get(blob, "ClassInformation")!)
	{
		if (Convert.ToInt32(Get(info, "ID")) != classId)
		{
			continue;
		}
		object? classes = Get(info, "Classes");
		if (classes is not IEnumerable entries)
		{
			continue;
		}
		foreach (object? pair in entries)
		{
			if (pair is null)
			{
				continue;
			}
			object? candidate = Get(pair, "Value");
			if (candidate is not null)
			{
				record = candidate;
			}
		}
	}
	if (record is null)
	{
		Console.Error.WriteLine("class record not found");
		return 11;
	}
	object nodeBuffer = Get(blob, "NodeBuffer")!;
	object stringBuffer = Get(blob, "StringBuffer")!;
	PropertyInfo nodeItem = nodeBuffer.GetType().GetProperty("Item")!;
	PropertyInfo stringItem = stringBuffer.GetType().GetProperty("Item")!;
	int rootIndex = Convert.ToInt32(Get(record, "ReleaseRootNode"));
	Console.WriteLine($"TREE|{versionText}|releaseRoot={rootIndex}");
	void WalkDirect(int index, int level)
	{
		object node = nodeItem.GetValue(nodeBuffer, [index])!;
		string type = stringItem
			.GetValue(stringBuffer, [Convert.ToInt32(Get(node, "TypeName"))])?.ToString() ?? string.Empty;
		string name = stringItem
			.GetValue(stringBuffer, [Convert.ToInt32(Get(node, "Name"))])?.ToString() ?? string.Empty;
		int byteSize = Convert.ToInt32(Get(node, "ByteSize"));
		int versionField = Convert.ToInt32(Get(node, "Version"));
		uint typeFlags = Convert.ToUInt32(Get(node, "TypeFlags"));
		uint metaFlag = Convert.ToUInt32(Get(node, "MetaFlag"));
		ushort[] subNodes = (ushort[])(Get(node, "SubNodes") ?? Array.Empty<ushort>());
		Console.WriteLine(
			$"NODE|{versionText}|{level}|{type}|{name}|{byteSize}|{versionField}|{typeFlags}|{metaFlag}|{index}|{subNodes.Length}");
		foreach (ushort child in subNodes)
		{
			WalkDirect(child, level + 1);
		}
	}
	WalkDirect(rootIndex, 0);
	return 10;
}
foreach (MethodInfo method in blob.GetType().GetMethods(BindingFlags.Public | BindingFlags.Instance))
{
	if (!method.Name.Contains("TypeTree", StringComparison.Ordinal)
		&& !method.Name.Contains("Structure", StringComparison.Ordinal)
		&& !method.Name.Contains("Node", StringComparison.Ordinal))
	{
		continue;
	}
	ParameterInfo[] parameters = method.GetParameters();
	if (parameters.Length != 2
		|| parameters[0].ParameterType != typeof(int)
		|| parameters[1].ParameterType != typeof(int))
	{
		continue;
	}
	if (Environment.GetEnvironmentVariable("TPK_LIST_METHODS") is not null)
	{
		Console.WriteLine($"METHOD|{method.Name}|{parameters[0].ParameterType.Name}|{parameters[1].ParameterType.Name}");
		continue;
	}
	try
	{
		object? candidate = method.Invoke(blob, [classId, versionIndex]);
		if (candidate is IEnumerable enumerable && enumerable.Cast<object?>().Any())
		{
			Console.WriteLine($"# lookup={method.Name} classId={classId} versionIndex={versionIndex}");
			root = enumerable;
			break;
		}
	}
	catch (TargetInvocationException)
	{
	}
}

if (root is null)
{
	Console.Error.WriteLine(
		$"no release TypeTree found for class {classId} ({target}) at {versionText}; "
		+ "the class may be an engine-only or script class in this revision");
	return 5;
}

Console.WriteLine($"TREE|{versionText}|releaseRoot=class{classId}");
int nodeIndex = 0;
void Walk(IEnumerable nodes, int level)
{
	foreach (object? node in nodes)
	{
		if (node is null)
		{
			continue;
		}
		string type = Prop<string>(node, "TypeName") ?? Prop<string>(node, "Type") ?? string.Empty;
		string name = Prop<string>(node, "Name") ?? string.Empty;
		int byteSize = Prop<int?>(node, "ByteSize") ?? Prop<int?>(node, "ByteCount") ?? -1;
		ushort versionField = Prop<ushort?>(node, "Version") ?? 0;
		uint typeFlags = Prop<uint?>(node, "TypeFlags") ?? 0;
		uint metaFlag = Prop<uint?>(node, "MetaFlag") ?? 0;
		object? subNodes = Get(node, "SubNodes") ?? Get(node, "Children");
		int subCount = subNodes is ICollection collection ? collection.Count : 0;
		int current = nodeIndex++;
		Console.WriteLine(
			$"NODE|{versionText}|{level}|{type}|{name}|{byteSize}|{versionField}|{typeFlags}|{metaFlag}|{current}|{subCount}");
		if (subNodes is IEnumerable children)
		{
			Walk(children, level + 1);
		}
	}
}
Walk((IEnumerable)root, 0);
return 0;

static object? Get(object instance, string name) =>
	instance.GetType().GetProperty(name, BindingFlags.Public | BindingFlags.Instance)?.GetValue(instance)
	?? instance.GetType().GetField(name, BindingFlags.Public | BindingFlags.Instance)?.GetValue(instance);

static T? Prop<T>(object instance, string name)
{
	object? raw = Get(instance, name);
	return raw is null ? default : (T)Convert.ChangeType(raw, Nullable.GetUnderlyingType(typeof(T)) ?? typeof(T));
}
