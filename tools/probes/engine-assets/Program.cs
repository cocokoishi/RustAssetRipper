// Isolated reference probe for the engine-assets TPK version bands and the
// generated class (OriginalName) table.
//
// Modes:
//   bands <out>
//       Dumps the ordered TpkEngineAssetsBlob band keys as `index|version` rows.
//   bandjson <version> <out>
//       Applies the exact frozen `EngineAssetsExporter.CreateFromTpkBlob` band
//       selection for <version> and writes `KEY|<selected band key>` followed
//       by the raw band JSON. Reads only the embedded TPK; writes only <out>.
//   classnames <out>
//       Dumps the generated `ClassID_*` class names as `classID|OriginalName`
//       rows sorted by class ID. These are the frozen `Pass110` ClassName
//       overrides (unversioned Unity names), read via reflection over the
//       SourceGenerated assembly. Reads nothing else; writes only <out>.
using System.Reflection;
using System.Text.Json;
using AssetRipper.Primitives;
using AssetRipper.SourceGenerated;
using AssetRipper.Tpk;
using AssetRipper.Tpk.EngineAssets;

if (args.Length == 0)
{
	Console.Error.WriteLine("usage: bands <out> | bandjson <version> <out> | tsv <version> <materialsOut> <shadersOut> | spritetextsv <version> <spritesOut> <texturesOut> | classnames <out>");
	return 2;
}

try
{
	switch (args[0])
	{
		case "bands":
			DumpBands(args[1]);
			return 0;
		case "bandjson":
			DumpBandJson(args[1], args[2]);
			return 0;
		case "tsv":
			DumpTsv(args[1], args[2], args[3]);
			return 0;
		case "spritetextsv":
			DumpSpriteTextureTsv(args[1], args[2], args[3]);
			return 0;
		case "classnames":
			DumpClassNames(args[1]);
			return 0;
		default:
			Console.Error.WriteLine($"unknown mode {args[0]}");
			return 2;
	}
}
catch (Exception ex)
{
	Console.Error.WriteLine($"FAILED: {ex}");
	return 1;
}

static TpkEngineAssetsBlob LoadBlob()
{
	using Stream stream = EngineAssetsTpk.GetStream();
	return (TpkEngineAssetsBlob)TpkFile.FromStream(stream).GetDataBlob();
}

static void DumpBands(string output)
{
	TpkEngineAssetsBlob blob = LoadBlob();
	List<string> lines = new();
	for (int i = 0; i < blob.Data.Count; i++)
	{
		lines.Add($"{i}|{blob.Data[i].Key}");
	}
	File.WriteAllLines(output, lines);
}

static void DumpBandJson(string versionText, string output)
{
	TpkEngineAssetsBlob blob = LoadBlob();
	UnityVersion version = UnityVersion.Parse(versionText);
	// Exact copy of EngineAssetsExporter.CreateFromTpkBlob band selection.
	int index;
	if (version <= blob.Data[0].Key)
	{
		index = 0;
	}
	else
	{
		index = blob.Data.Count - 1;
		for (int i = 1; i < blob.Data.Count; i++)
		{
			if (version <= blob.Data[i].Key)
			{
				index = i - 1;
				break;
			}
		}
	}
	File.WriteAllText(output, $"KEY|{blob.Data[index].Key}\n{blob.Data[index].Value}");
}

static void DumpTsv(string versionText, string materialsOut, string shadersOut)
{
	TpkEngineAssetsBlob blob = LoadBlob();
	UnityVersion version = UnityVersion.Parse(versionText);
	// Same band selection as DumpBandJson.
	int index;
	if (version <= blob.Data[0].Key)
	{
		index = 0;
	}
	else
	{
		index = blob.Data.Count - 1;
		for (int i = 1; i < blob.Data.Count; i++)
		{
			if (version <= blob.Data[i].Key)
			{
				index = i - 1;
				break;
			}
		}
	}
	using JsonDocument document = JsonDocument.Parse(blob.Data[index].Value);
	List<string> materials = new();
	List<string> shaders = new();
	foreach (string section in new[] { "DefaultResources", "ExtraResources" })
	{
		string kind = section == "DefaultResources" ? "D" : "F";
		if (!document.RootElement.TryGetProperty(section, out JsonElement resources))
		{
			continue;
		}
		foreach (JsonProperty entry in resources.EnumerateObject())
		{
			JsonElement value = entry.Value;
			string type = value.GetProperty("$type").GetString() ?? "";
			string name = value.TryGetProperty("Name", out JsonElement nameElement)
				? nameElement.GetString() ?? ""
				: "";
			if (type == "Material")
			{
				string shader = value.TryGetProperty("Shader", out JsonElement shaderElement)
					? shaderElement.GetString() ?? ""
					: "";
				materials.Add($"{entry.Name}|{kind}|{name}|{shader}");
			}
			else if (type == "Shader")
			{
				List<string> properties = new();
				if (value.TryGetProperty("PropertyNames", out JsonElement props))
				{
					foreach (JsonElement prop in props.EnumerateArray())
					{
						properties.Add(prop.GetString() ?? "");
					}
				}
				shaders.Add($"{entry.Name}|{kind}|{name}|{string.Join(" ", properties)}");
			}
		}
	}
	File.WriteAllLines(materialsOut, materials);
	File.WriteAllLines(shadersOut, shaders);
}

static void DumpSpriteTextureTsv(string versionText, string spritesOut, string texturesOut)
{
	TpkEngineAssetsBlob blob = LoadBlob();
	UnityVersion version = UnityVersion.Parse(versionText);
	// Same band selection as DumpBandJson.
	int index;
	if (version <= blob.Data[0].Key)
	{
		index = 0;
	}
	else
	{
		index = blob.Data.Count - 1;
		for (int i = 1; i < blob.Data.Count; i++)
		{
			if (version <= blob.Data[i].Key)
			{
				index = i - 1;
				break;
			}
		}
	}
	using JsonDocument document = JsonDocument.Parse(blob.Data[index].Value);
	List<string> sprites = new();
	List<string> textures = new();
	foreach (string section in new[] { "DefaultResources", "ExtraResources" })
	{
		string kind = section == "DefaultResources" ? "D" : "F";
		if (!document.RootElement.TryGetProperty(section, out JsonElement resources))
		{
			continue;
		}
		foreach (JsonProperty entry in resources.EnumerateObject())
		{
			JsonElement value = entry.Value;
			string type = value.GetProperty("$type").GetString() ?? "";
			string name = value.TryGetProperty("Name", out JsonElement nameElement)
				? nameElement.GetString() ?? ""
				: "";
			if (type == "Sprite")
			{
				string texture = value.TryGetProperty("Texture", out JsonElement textureElement)
					? textureElement.GetString() ?? ""
					: "";
				sprites.Add($"{entry.Name}|{kind}|{name}|{texture}");
			}
			else if (type == "Texture2D")
			{
				string width = value.TryGetProperty("Width", out JsonElement widthElement)
					? widthElement.GetInt32().ToString()
					: "";
				string height = value.TryGetProperty("Height", out JsonElement heightElement)
					? heightElement.GetInt32().ToString()
					: "";
				textures.Add($"{entry.Name}|{kind}|{name}|{width}|{height}");
			}
		}
	}
	File.WriteAllLines(spritesOut, sprites);
	File.WriteAllLines(texturesOut, textures);
}

static void DumpClassNames(string output)
{
	Assembly assembly = typeof(AssetRipper.SourceGenerated.Classes.ClassID_1.GameObject).Assembly;
	List<string> lines = new();
	foreach (Type type in assembly.GetTypes())
	{
		string? ns = type.Namespace;
		if (ns is null || !ns.StartsWith("AssetRipper.SourceGenerated.Classes.ClassID_", StringComparison.Ordinal))
		{
			continue;
		}
		string rest = ns.Substring("AssetRipper.SourceGenerated.Classes.ClassID_".Length);
		if (rest.Contains('.') || !int.TryParse(rest, out int classId))
		{
			continue;
		}
		if (!type.IsClass || type.IsNested)
		{
			continue;
		}
		lines.Add($"{classId}|{type.Name}");
	}
	lines.Sort(StringComparer.Ordinal);
	File.WriteAllLines(output, lines);
}
