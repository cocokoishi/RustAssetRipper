using System.IO.Compression;
using System.Text;
using AssetRipper.Assets.Bundles;
using AssetRipper.Export.Configuration;
using AssetRipper.Export.UnityProjects.Project;
using AssetRipper.Export.UnityProjects.Scripts;
using AssetRipper.Import.Configuration;
using AssetRipper.Import.Structure.Assembly.Managers;
using AssetRipper.Import.Structure.Platforms;
using AssetRipper.IO.Files;
using AssetRipper.Primitives;
using AssetRipper.Processing;

internal static partial class ManagedProjectProbe
{
    // Calls only frozen post-exporters on independent original input files.
    // No Unity install or user AssetBundle is needed or accessed.
    public static int ProbePostExports(string scratch, string assemblyPath, string rowsPath)
    {
        scratch = Path.GetFullPath(scratch);
        Directory.CreateDirectory(scratch);
        string input = Path.Combine(scratch, "original");
        Directory.CreateDirectory(Path.Combine(input, "empty", "nested"));
        Directory.CreateDirectory(Path.Combine(input, "nested"));
        File.WriteAllBytes(Path.Combine(input, "nested", "payload.bin"), [0, 1, 2, 0xFF, 7]);
        File.WriteAllText(Path.Combine(input, "readme.txt"), "Original streaming data\n", new UTF8Encoding(false));
        File.WriteAllBytes(Path.Combine(input, "numbered.bin.split0"), [3, 4]);
        File.WriteAllBytes(Path.Combine(input, "numbered.bin.split1"), [5, 6]);
        using (FileStream file = File.Create(Path.Combine(input, "wrapped.bytes")))
        using (GZipStream compressed = new(file, CompressionLevel.SmallestSize))
        {
            compressed.Write(Encoding.UTF8.GetBytes("Keep the wrapper bytes, not the decoded file."));
        }
        using StreamWriter rows = new(rowsPath, false, new UTF8Encoding(false));
        rows.WriteLine("# frozen StreamingAssetsPostExporter and DllPostExporter; Unity 2022.3.22f1");
        foreach (string directory in Directory.GetDirectories(input, "*", SearchOption.AllDirectories).Order(StringComparer.Ordinal))
            rows.WriteLine($"INPUT_DIR|{Path.GetRelativePath(input, directory).Replace('\\', '/')}");
        foreach (string file in Directory.GetFiles(input, "*", SearchOption.AllDirectories).Order(StringComparer.Ordinal))
            rows.WriteLine($"INPUT_FILE|{Path.GetRelativePath(input, file).Replace('\\', '/')}|{Convert.ToHexString(File.ReadAllBytes(file))}");
        foreach (string scenario in new[] { "extract", "ignore", "no-platform", "missing", "null-path" })
        {
            FullConfiguration settings = new() { ExportRootPath = Path.Combine(scratch, scenario) };
            settings.SetProjectSettings(new UnityVersion(2022, 3, 22, UnityVersionType.Final, 1));
            settings.ImportSettings.StreamingAssetsMode = scenario == "ignore" ? StreamingAssetsMode.Ignore : StreamingAssetsMode.Extract;
            MonoManager manager = new(_ => { });
            if (scenario == "extract")
            {
                manager.Load(Path.GetFullPath(assemblyPath), LocalFileSystem.Instance);
                manager.ClearStreamCache();
            }
            PlatformGameStructure? platform = scenario == "no-platform" ? null : new PostExportPlatform(
                scenario == "missing" ? Path.Combine(scratch, "absent") : scenario == "null-path" ? null : input);
            GameData data = new(new GameBundle(), new UnityVersion(2022, 3, 22, UnityVersionType.Final, 1), manager, platform);
            new StreamingAssetsPostExporter().DoPostExport(data, settings, LocalFileSystem.Instance);
            new DllPostExporter().DoPostExport(data, settings, LocalFileSystem.Instance);
            if (!Directory.Exists(settings.ExportRootPath)) continue;
            foreach (string directory in Directory.GetDirectories(settings.ExportRootPath, "*", SearchOption.AllDirectories).Order(StringComparer.Ordinal))
                rows.WriteLine($"SOURCE_DIR|{scenario}|{Path.GetRelativePath(settings.ExportRootPath, directory).Replace('\\', '/')}");
            foreach (string file in Directory.GetFiles(settings.ExportRootPath, "*", SearchOption.AllDirectories).Order(StringComparer.Ordinal))
                rows.WriteLine($"SOURCE_FILE|{scenario}|{Path.GetRelativePath(settings.ExportRootPath, file).Replace('\\', '/')}|{Convert.ToHexString(File.ReadAllBytes(file))}");
        }
        return 0;
    }

    private sealed class PostExportPlatform : PlatformGameStructure
    {
        // Protected property is the only platform fact the frozen copier reads.
        public PostExportPlatform(string? streamingPath) : base(LocalFileSystem.Instance)
        {
            StreamingAssetsPath = streamingPath;
        }
    }
}
