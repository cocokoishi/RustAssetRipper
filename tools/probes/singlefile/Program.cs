using AssetRipper.IO.Endian;
using AssetRipper.IO.Files;
using AssetRipper.IO.Files.ResourceFiles;
using AssetRipper.IO.Files.WebFiles;

var scratchRoot = Path.GetFullPath(Path.Combine(Environment.CurrentDirectory, "target", "singlefile-probe"));
Directory.CreateDirectory(scratchRoot);
var root = Path.Combine(scratchRoot, Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(root);
var fileSystem = LocalFileSystem.Instance;

byte[] BuildWebData(bool alignEntries)
{
	var web = new WebFile();
	web.AddResourceFile(new ResourceFile("alpha-payload"u8.ToArray(), "", "alpha.bin"));
	web.AddResourceFile(new ResourceFile("beta!"u8.ToArray(), "", "beta.bin"));
	using var output = new MemoryStream();
	if (alignEntries)
	{
		web.Write(output);
	}
	else
	{
		using var writer = new EndianWriter(output, EndianType.LittleEndian);
		web.Write(writer, false);
	}
	return output.ToArray();
}

var aligned = BuildWebData(true);
var noalign = BuildWebData(false);
Console.WriteLine($"FILE_HEX|aligned|{Convert.ToHexString(aligned)}");
Console.WriteLine($"FILE_HEX|noalign|{Convert.ToHexString(noalign)}");

foreach (var (label, bytes) in new[] { ("ALIGNED", aligned), ("NOALIGN", noalign) })
{
	var path = Path.Combine(root, $"{label.ToLowerInvariant()}.webdata");
	File.WriteAllBytes(path, bytes);
	using var file = SchemeReader.LoadFile(path, fileSystem);
	if (file is not WebFile web)
	{
		Console.WriteLine($"{label}|NOT_A_WEBFILE|{file.GetType().Name}");
	}
	else
	{
		foreach (var resource in web.ResourceFiles)
		{
			Console.WriteLine($"{label}_ENTRY|{resource.Name}|{resource.ToByteArray().Length}|{Convert.ToHexString(resource.ToByteArray())}");
		}
	}
}

// Truncated input: record exactly what the source produces instead of assuming
// a rejection. A silent partial container on either side would be a real gap.
var truncatedPath = Path.Combine(root, "truncated.webdata");
File.WriteAllBytes(truncatedPath, aligned[..8]);
try
{
	using (var file = SchemeReader.LoadFile(truncatedPath, fileSystem))
	{
		if (file is WebFile web)
		{
			var names = string.Join(",", web.ResourceFiles.Select(resource => resource.Name));
			Console.WriteLine($"TRUNCATED|ACCEPTED|WebFile|resources={web.ResourceFiles.Count}|serialized={web.SerializedFiles.Count}|names={names}");
		}
		else
		{
			Console.WriteLine($"TRUNCATED|ACCEPTED|{file.GetType().Name}|{file.Name}");
		}
	}
}
catch (Exception error)
{
	Console.WriteLine($"TRUNCATED|REJECTED|{error.GetType().Name}");
}

// Split-file single-path contract: base and part paths both resolve; a missing
// part is rejected instead of mounting a partial stream.
var splitBase = Path.Combine(root, "single.webdata");
var half = aligned.Length / 2;
File.WriteAllBytes(splitBase + ".split0", aligned[..half]);
File.WriteAllBytes(splitBase + ".split1", aligned[half..]);
foreach (var (label, path) in new[] { ("SPLIT_BASE", splitBase), ("SPLIT_PART", splitBase + ".split1") })
{
	using (var file = SchemeReader.LoadFile(path, fileSystem))
	{
		if (file is WebFile web && web.ResourceFiles.Count == 2)
		{
			Console.WriteLine($"{label}|OK|{web.ResourceFiles[0].Name}|{web.ResourceFiles[1].Name}");
		}
		else
		{
			Console.WriteLine($"{label}|UNEXPECTED|{file.GetType().Name}");
		}
	}
}
// A missing middle part is rejected; a fresh gap directory avoids deleting a
// file whose split readers the source still retains (Windows share lock).
var gapBase = Path.Combine(root, "gap.webdata");
File.WriteAllBytes(gapBase + ".split0", aligned[..half]);
File.WriteAllBytes(gapBase + ".split2", aligned[half..]);
try
{
	using (var file = SchemeReader.LoadFile(gapBase, fileSystem))
	{
		Console.WriteLine($"GAP|ACCEPTED|{file.Name}");
	}
}
catch (Exception error)
{
	Console.WriteLine($"GAP|REJECTED|{error.GetType().Name}");
}
