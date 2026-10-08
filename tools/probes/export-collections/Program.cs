// Dumps the generated AssetRipper.SourceGenerated class-ID -> exporter-target interface map.
//
// The frozen snapshot ships no generated sources: the generated assembly is produced by
// AssetRipper.AssemblyDumper from the Unity reference assemblies. This probe reads the
// already-built AssetRipper.SourceGenerated.dll in the isolated reference copy and prints
// every (class ID, transitive whitelisted interface) pair the project exporter dispatch
// depends on. Reflection's GetInterfaces result is intentional: the registration stack uses
// assignability, which includes interfaces inherited from generated base classes.
//
// Usage:
//   dotnet run --project tools/probes/export-collections/ExportCollectionsProbe.csproj -c Release \
//     [path-to-AssetRipper.SourceGenerated.dll]
using System.Reflection;
using System.Runtime.Loader;

string defaultPath = Path.Combine(
    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
    "Temp",
    "opencode",
    "ar-ref",
    "Source",
    "0Bins",
    "AssetRipper.Tools.DependenceGrapher",
    "Release",
    "AssetRipper.SourceGenerated.dll");
string path = args.Length > 0 && !args[0].StartsWith("--", StringComparison.Ordinal) ? args[0] : defaultPath;
bool dumpAll = args.Contains("--all");
bool detail = args.Contains("--detail");

// The interfaces named by `ProjectExporter` registrations plus the claim predicates.
HashSet<string> wanted = new(StringComparer.Ordinal)
{
    "IAnimationClip",
    "IAnimatorController",
    "IAnimatorOverrideController",
    "IAnimatorState",
    "IAnimatorStateMachine",
    "IAnimatorTransition",
    "IAssetBundle",
    "IAssetBundleManifest",
    "IAudioClip",
    "IAudioMixer",
    "IAudioMixerEffectController",
    "IAudioMixerGroup",
    "IAudioMixerSnapshot",
    "IAvatarMask",
    "IBlendTree",
    "IBuildSettings",
    "IComponent",
    "ICubemap",
    "ICubemapArray",
    "IEditorBuildSettings",
    "IFlare",
    "IFont",
    "IGameObject",
    "IGlobalGameManager",
    "IImageTexture",
    "ILevelGameManager",
    "ILightingSettings",
    "ILightmapParameters",
    "IMaterial",
    "IMesh",
    "IMonoBehaviour",
    "IMonoManager",
    "IMonoScript",
    "IMovieTexture",
    "IPhysicsMaterial",
    "IPhysicsMaterial2D",
    "IPrefabInstance",
    "IPreloadData",
    "IRenderTexture",
    "IResourceManager",
    "ISceneAsset",
    "IShader",
    "IShaderNameRegistry",
    "IShaderVariantCollection",
    "ISprite",
    "ISpriteAtlas",
    "ITerrainData",
    "ITerrainLayer",
    "ITextAsset",
    "ITexture",
    "ITexture2D",
    "ITexture2DArray",
    "ITexture3D",
    "IUnityObjectBase",
    "IVideoClip",
    "IWebCamTexture",
};

const string prefix = "AssetRipper.SourceGenerated.Classes.ClassID_";

string fullPath = Path.GetFullPath(path);
string assemblyDirectory = Path.GetDirectoryName(fullPath)!;
AssemblyLoadContext.Default.Resolving += (_, name) =>
{
    string dependency = Path.Combine(assemblyDirectory, $"{name.Name}.dll");
    return File.Exists(dependency)
        ? AssemblyLoadContext.Default.LoadFromAssemblyPath(dependency)
        : null;
};
Assembly assembly = AssemblyLoadContext.Default.LoadFromAssemblyPath(fullPath);

SortedDictionary<int, SortedSet<string>> rows = new();

foreach (Type type in assembly.GetTypes())
{
    string ns = type.Namespace ?? string.Empty;
    if (!ns.StartsWith(prefix, StringComparison.Ordinal))
    {
        continue;
    }
    string classIdText = ns[prefix.Length..];
    if (!int.TryParse(classIdText, out int classId) || classId == 0)
    {
        continue;
    }
    Type[] interfaces = type.GetInterfaces();
    if (detail && classId is 48 or 109)
    {
        Console.WriteLine($"TYPE\t{classId}\t{type.FullName}\t{string.Join(",", interfaces.Select(i => i.Name).Order(StringComparer.Ordinal))}");
    }
    foreach (Type implementedInterface in interfaces)
    {
        string name = implementedInterface.Name;
        if (!dumpAll && !wanted.Contains(name))
        {
            continue;
        }
        if (dumpAll && !name.StartsWith('I'))
        {
            continue;
        }
        if (!rows.TryGetValue(classId, out SortedSet<string>? names))
        {
            names = new SortedSet<string>(StringComparer.Ordinal);
            rows[classId] = names;
        }
        names.Add(name);
    }
}

foreach (KeyValuePair<int, SortedSet<string>> row in rows)
{
    Console.WriteLine($"{row.Key}\t{string.Join(",", row.Value)}");
}
