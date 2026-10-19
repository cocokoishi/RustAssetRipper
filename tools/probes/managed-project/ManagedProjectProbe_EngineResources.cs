using System.Security.Cryptography;
using System.Text;
using AssetRipper.Export.Configuration;
using AssetRipper.Export.UnityProjects;
using AssetRipper.Import.Logging;
using AssetRipper.IO.Files;
using AssetRipper.Mining.PredefinedAssets;

internal static partial class ManagedProjectProbe
{
    internal static int ExportCachedGameObject(
        string fixturePathArgument,
        string inputRootArgument,
        string outputRootArgument,
        string rowsArgument,
        string inputFileName)
    {
        string fixturePath = Path.GetFullPath(fixturePathArgument);
        string inputRoot = Path.GetFullPath(inputRootArgument);
        string outputRoot = Path.GetFullPath(outputRootArgument);
        string rowsPath = Path.GetFullPath(rowsArgument);
        EnsureEmptyOrMissingDirectory(inputRoot, "input");
        EnsureEmptyOrMissingDirectory(outputRoot, "output");
        if (File.Exists(rowsPath) || Directory.Exists(rowsPath))
        {
            throw new IOException($"refusing to overwrite probe rows {rowsPath}");
        }
        if (Path.GetFileName(inputFileName) != inputFileName || string.IsNullOrWhiteSpace(inputFileName))
        {
            throw new ArgumentException("input file name must be one safe relative component", nameof(inputFileName));
        }

        Directory.CreateDirectory(inputRoot);
        Directory.CreateDirectory(outputRoot);
        Directory.CreateDirectory(Path.GetDirectoryName(rowsPath)!);

        byte[] inputBytes = File.ReadAllBytes(fixturePath);
        string inputPath = Path.Combine(inputRoot, inputFileName);
        File.WriteAllBytes(inputPath, inputBytes);

        const string engineResourceJson = "{\"DefaultResources\":{\"900\":{\"$type\":\"GameObject\",\"Name\":\"LegacyRoot\",\"Components\":[4],\"Layer\":0}},\"ExtraResources\":{}}";
        FullConfiguration settings = new();
        settings.SingletonData.SetStoredValue<EngineResourceData?>(
            nameof(EngineResourceData),
            EngineResourceData.FromJson(engineResourceJson));

        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(settings);
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        int gameObjectCount = gameData.GameBundle.FetchAssets().Count(asset => asset.ClassID == 1);
        int sceneCount = gameData.GameBundle.Scenes.Count();
        int prefabCount = gameData.GameBundle.FetchAssets().Count(asset => asset.ClassID == 1001);

        string? exportError = null;
        try
        {
            handler.Export(gameData, outputRoot, LocalFileSystem.Instance);
        }
        catch (ArgumentException error) when (
            error.Message.Contains("already added by", StringComparison.Ordinal)
            || error.Message.Contains("same key", StringComparison.OrdinalIgnoreCase))
        {
            exportError = error.GetType().Name;
        }

        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false)) { NewLine = "\n" };
#if DEBUG
        rows.WriteLine("BUILD|Debug");
#else
        rows.WriteLine("BUILD|Release");
#endif
        rows.WriteLine("VERSION|2017.4.40f1|format=17|platform=NoTarget");
        rows.WriteLine($"INPUT|{inputFileName}|{Convert.ToHexString(SHA256.HashData(inputBytes))}");
        rows.WriteLine("CACHE|DefaultResources|900|GameObject|LegacyRoot|Layer=0|ComponentCount=1");
        rows.WriteLine($"OBJECTS|gameObjects={gameObjectCount}|scenes={sceneCount}|prefabInstances={prefabCount}");
        rows.WriteLine(exportError is null ? "EXPORT|success" : $"EXPORT|duplicate-membership|{exportError}");

        string projectAssets = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string[] assetFiles = Directory.Exists(projectAssets)
            ? Directory.GetFiles(projectAssets, "*", SearchOption.AllDirectories)
                .Where(path => new[] { ".asset", ".prefab", ".meta" }.Contains(Path.GetExtension(path), StringComparer.OrdinalIgnoreCase))
                .OrderBy(path => Path.GetRelativePath(outputRoot, path), StringComparer.Ordinal)
                .ToArray()
            : [];
        foreach (string file in assetFiles)
        {
            string relative = Path.GetRelativePath(outputRoot, file).Replace('\\', '/');
            byte[] bytes = File.ReadAllBytes(file);
            rows.WriteLine($"FILE|{relative}|{bytes.Length}|{Convert.ToHexString(SHA256.HashData(bytes))}");
            rows.WriteLine($"CONTENT|{relative}|{Convert.ToBase64String(bytes)}");
        }

        Console.WriteLine($"gameobjects={gameObjectCount} scenes={sceneCount} prefab_instances={prefabCount}");
        Console.WriteLine(exportError is null ? "export=success" : "export=duplicate-membership");
        Console.WriteLine($"project_asset_files={assetFiles.Length}");
        return 0;
    }
}
