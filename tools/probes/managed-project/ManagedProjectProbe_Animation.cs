using System.Security.Cryptography;
using AssetRipper.Export.Configuration;
using AssetRipper.Export.UnityProjects;
using AssetRipper.Import.Logging;
using AssetRipper.IO.Files;

internal static partial class ManagedProjectProbe
{
    internal static int ExportAnimationProject(string inputRootArgument, string outputRootArgument)
    {
        string inputRoot = Path.GetFullPath(inputRootArgument);
        string outputRoot = Path.GetFullPath(outputRootArgument);
        if (!Directory.Exists(inputRoot))
        {
            throw new DirectoryNotFoundException($"animation fixture input is missing: {inputRoot}");
        }
        EnsureEmptyOrMissingDirectory(outputRoot, "output");

        Logger.Add(new ConsoleLogger());
        ExportHandler handler = new(new FullConfiguration());
        var gameData = handler.LoadAndProcess([inputRoot], LocalFileSystem.Instance);
        int animationClipCount = gameData.GameBundle.FetchAssets()
            .Count(asset => asset.ClassID == 74);
        if (animationClipCount != 1)
        {
            throw new InvalidDataException($"expected one source AnimationClip, found {animationClipCount}");
        }
        handler.Export(gameData, outputRoot, LocalFileSystem.Instance);

        string assetsRoot = Path.Combine(outputRoot, "ExportedProject", "Assets");
        string[] animationPaths = Directory.GetFiles(assetsRoot, "*.anim", SearchOption.AllDirectories);
        if (animationPaths.Length != 1)
        {
            throw new InvalidDataException($"expected one exported AnimationClip YAML, found {animationPaths.Length}");
        }
        byte[] animationBytes = File.ReadAllBytes(animationPaths[0]);
        string animationText = System.Text.Encoding.UTF8.GetString(animationBytes);
        if (!animationText.Contains("health", StringComparison.Ordinal))
        {
            throw new InvalidDataException("source animation export did not resolve the MonoScript health field path");
        }
        string relativePath = Path.GetRelativePath(outputRoot, animationPaths[0]).Replace('\\', '/');
        Console.WriteLine($"animation={relativePath}");
        Console.WriteLine($"animation_sha256={Convert.ToHexString(SHA256.HashData(animationBytes))}");
        Console.WriteLine($"animation_bytes={animationBytes.Length}");
        return 0;
    }
}
