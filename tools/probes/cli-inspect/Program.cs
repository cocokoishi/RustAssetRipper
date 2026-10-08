using AssetRipper.Export.Configuration;
using AssetRipper.Export.UnityProjects;
using AssetRipper.IO.Files;
using AssetRipper.Processing;

if (args.Length != 1)
{
	throw new ArgumentException("Expected one empty directory path.");
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

sealed class CountingExportHandler(FullConfiguration settings) : ExportHandler(settings)
{
	public int ProcessorSourceCalls { get; private set; }

	protected override IEnumerable<IAssetProcessor> GetProcessors()
	{
		ProcessorSourceCalls++;
		return [];
	}
}
