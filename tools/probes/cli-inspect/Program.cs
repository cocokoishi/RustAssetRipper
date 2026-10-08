using AssetRipper.Export.Configuration;
using AssetRipper.Export.UnityProjects;
using AssetRipper.IO.Files;
using AssetRipper.Processing;

if (args.Length == 3 && args[0] == "--platform-mixed")
{
	RunPlatformMixed(args[1], args[2]);
	return;
}

if (args.Length != 1)
{
	throw new ArgumentException("Expected one empty directory path or --platform-mixed <fixture> <scratch-root>.");
}

string inputDirectory = Path.GetFullPath(args[0]);
Directory.CreateDirectory(inputDirectory);
if (Directory.EnumerateFileSystemEntries(inputDirectory).Any())
{
	throw new InvalidOperationException("The probe input directory must be empty.");
}

CountingExportHandler handler = new(new FullConfiguration());
GameData gameData = handler.LoadAndProcess([inputDirectory], LocalFileSystem.Instance);
int collectionCount = gameData.GameBundle.FetchAssetCollections().Count();
Console.WriteLine($"EMPTY_DIRECTORY|collections={collectionCount}|processors_requested={handler.ProcessorSourceCalls}");

void RunPlatformMixed(string fixturePath, string scratchRoot)
{
	string root = Path.GetFullPath(scratchRoot);
	Directory.CreateDirectory(root);
	if (Directory.EnumerateFileSystemEntries(root).Any())
	{
		throw new InvalidOperationException("The probe scratch root must be empty.");
	}

	string fixtureHex = File.ReadLines(fixturePath)
		.Select(line => line.Trim())
		.First(line => line.StartsWith("SFHEX|", StringComparison.Ordinal))
		.Split('|')[1];
	byte[] serializedFile = Convert.FromHexString(fixtureHex);
	string platformRoot = Path.Combine(root, "platform");
	string dataPath = Path.Combine(platformRoot, "game_Data");
	string mixedRoot = Path.Combine(root, "mixed");
	Directory.CreateDirectory(dataPath);
	Directory.CreateDirectory(mixedRoot);
	File.WriteAllBytes(Path.Combine(platformRoot, "game.exe"), [0x4D, 0x5A]);
	File.WriteAllBytes(Path.Combine(dataPath, "globalgamemanagers"), serializedFile);
	File.WriteAllBytes(Path.Combine(mixedRoot, "main.assets"), serializedFile);

	CountingExportHandler mixedHandler = new(new FullConfiguration());
	GameData mixedData = mixedHandler.LoadAndProcess([platformRoot, mixedRoot], LocalFileSystem.Instance);
	string[] inputCollectionNames = mixedData.GameBundle.FetchAssetCollections()
		.Select(collection => collection.Name)
		.Where(name => name is "globalgamemanagers" or "main.assets")
		.Order(StringComparer.Ordinal)
		.ToArray();
	Console.WriteLine($"PLATFORM_MIXED|input_names={string.Join(',', inputCollectionNames)}|processors_requested={mixedHandler.ProcessorSourceCalls}");
}

sealed class CountingExportHandler(FullConfiguration settings) : ExportHandler(settings)
{
	public int ProcessorSourceCalls { get; private set; }

	protected override IEnumerable<IAssetProcessor> GetProcessors()
	{
		ProcessorSourceCalls++;
		return [];
	}
}
