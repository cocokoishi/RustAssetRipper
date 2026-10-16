// Frozen ground truth for the web DependenciesTab port.
//
// Loads one bundle file into a GameBundle exactly like the frozen web surface does,
// then prints every asset's FetchDependencies pairs with the collection-scoped
// TryGetAsset verdict the DependenciesTab renders (a link when it resolves, the
// literal "Missing" text otherwise, silence for null PPtrs).
//
// Usage:
//   dotnet run --project tools/probes/asset-deps/AssetDepsProbe.csproj -c Release -- <bundle>
using AssetRipper.Assets;
using AssetRipper.Assets.Bundles;
using AssetRipper.Assets.Metadata;
using AssetRipper.Import.AssetCreation;
using AssetRipper.Import.Structure.Assembly.Managers;
using AssetRipper.IO.Files;

string? bundlePath = args.FirstOrDefault(File.Exists);
if (bundlePath is null)
{
	Console.Error.WriteLine("usage: asset-deps <bundle>");
	return 2;
}

using GameBundle bundle = GameBundle.FromPaths(
	[Path.GetFullPath(bundlePath)],
	new GameAssetFactory(new BaseManager(_ => { })),
	LocalFileSystem.Instance);

foreach (IUnityObjectBase asset in bundle.FetchAssets())
{
	Console.WriteLine($"ASSET|{asset.ClassID}|{asset.PathID}|{asset.ClassName}|{asset.GetBestName()}|{asset.Collection.Name}");
}

foreach (IUnityObjectBase asset in bundle.FetchAssets())
{
	bool enabled = false;
	foreach ((string field, PPtr pptr) in asset.FetchDependencies())
	{
		if (pptr.IsNull)
		{
			Console.WriteLine($"DEP|{asset.PathID}|{field}|{pptr.FileID}|{pptr.PathID}|NULL");
			continue;
		}
		enabled = true;
		if (asset.Collection.TryGetAsset(pptr, out IUnityObjectBase? dependency) && dependency is not null)
		{
			Console.WriteLine($"DEP|{asset.PathID}|{field}|{pptr.FileID}|{pptr.PathID}|LINK|{dependency.Collection.Name}|{dependency.ClassID}|{dependency.PathID}|{dependency.GetBestName()}");
		}
		else
		{
			Console.WriteLine($"DEP|{asset.PathID}|{field}|{pptr.FileID}|{pptr.PathID}|MISSING");
		}
	}
	Console.WriteLine($"ENABLED|{asset.PathID}|{enabled}");
}

return 0;
