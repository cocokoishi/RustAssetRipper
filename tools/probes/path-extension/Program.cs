// What exactly does `Path.GetExtension` return at its edges?
//
// `VideoClipExtensions.TryGetExtensionFromPath` calls `Path.GetExtension(clip.OriginalPath_R)`, and
// `VideoClipExportCollection` uses the result as the export extension. The reference then tests
// `string.IsNullOrEmpty(extension)` and strips the leading character, so whether a trailing dot
// yields `"."` or `""` decides between an empty export extension and the `bytes` fallback - two
// different output file names for the same asset.
//
// The port's `asset_path::get_extension` returns `""` for a trailing dot. This probe measures the
// runtime's answer so the port can be judged against it rather than against a recollection.
//
// Run (from the RustAssetripper repository root):
//   dotnet run --project tools/probes/path-extension/PathExtensionProbe.csproj -c Release
public static class Program
{
    public static int Main()
    {
        string[] samples =
        {
            "video.mp4",
            "video",
            "video.",
            "a.b.c",
            ".bashrc",
            "",
            ".",
            "..",
            "dir.d/file",
            "dir/file.mp4",
            "dir/file.",
            "/abs/path/x.ogv",
            "C:\\dir\\x.mp4",
            "C:\\dir.d\\x",
            "x..y",
            "x. y",
            "sp ace.mp4",
            "trailing..",
        };
        foreach (string sample in samples)
        {
            string extension = Path.GetExtension(sample);
            // `Describe` renders the raw text so an empty result is distinguishable from a dot.
            Console.WriteLine($"EXT|{Describe(sample)}|{Describe(extension)}");
        }
        return 0;
    }

    private static string Describe(string value)
    {
        if (value.Length == 0)
        {
            return "<empty>";
        }
        var builder = new System.Text.StringBuilder();
        foreach (char c in value)
        {
            builder.Append(c < 0x20 || c > 0x7e ? $"\\u{(int)c:X4}" : c.ToString());
        }
        return builder.ToString();
    }
}
