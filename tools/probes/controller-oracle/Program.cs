// Differential oracle (not product code): loads one bundle through the frozen
// AssetRipper load+process pipeline and reports AnimatorController hierarchy
// membership plus MainAsset assignment per sub-asset, before and after processing.
//
// Usage:
//   dotnet run --project tools/probes/controller-oracle/controller-oracle.csproj \
//     --configuration Release -p:ReferenceRoot=<isolated build> -- <bundle> <report.tsv>
//
// Report sections (tab-separated):
//   PRE-MEMBER|classID|pathID|name|collection|mainClass|mainPathID|mainName
//   CONTROLLER|pathID|name|collection|hierarchyTotal|withDupHistogram|distinctTotal|distinctHistogram
//   POST-MEMBER|classID|pathID|name|collection|mainClass|mainPathID|mainName
// MEMBER covers classes 114/206/1101/1102/1107/1109 across every collection.
using AssetRipper.Assets;
using AssetRipper.Export.Configuration;
using AssetRipper.Export.UnityProjects;
using AssetRipper.IO.Files;
using AssetRipper.Processing;
using AssetRipper.SourceGenerated.Classes.ClassID_91;
using AssetRipper.SourceGenerated.Extensions;

static class Oracle
{
	static readonly int[] MemberClasses = [114, 206, 1101, 1102, 1107, 1109];

	static int Run(string[] args)
	{
		if (args.Length != 2)
		{
			Console.Error.WriteLine("usage: controller-oracle <bundle> <report.tsv>");
			return 2;
		}
		ExportHandler handler = new(new FullConfiguration());
		GameData gameData = handler.Load([args[0]], LocalFileSystem.Instance);
		using StreamWriter writer = new(args[1], false, new System.Text.UTF8Encoding(false));
		DumpMembers(gameData, writer, "PRE");
		handler.Process(gameData);
		DumpControllers(gameData, writer);
		DumpMembers(gameData, writer, "POST");
		DumpConstants(gameData, writer);
		return 0;
	}

	static void DumpConstants(GameData gameData, StreamWriter writer)
	{
		List<IAnimatorController> controllers = [];
		foreach (IUnityObjectBase asset in gameData.GameBundle.FetchAssets())
		{
			if (asset is IAnimatorController controller)
			{
				controllers.Add(controller);
			}
		}
		controllers.Sort(static (a, b) => a.PathID.CompareTo(b.PathID));
		foreach (IAnimatorController controller in controllers)
		{
			foreach (var tosPair in controller.TOS)
			{
				writer.WriteLine($"TOS\t{controller.PathID}\t{tosPair.Key}\t{Escape(tosPair.Value.String)}");
			}
			var controllerConstant = controller.Controller;
			for (int mi = 0; mi < controllerConstant.StateMachineArray.Count; mi++)
			{
				var machine = controllerConstant.StateMachineArray[mi].Data;
				writer.WriteLine($"MCONST\t{controller.PathID}\t{mi}\t{machine.StateConstantArray.Count}\t{(machine.Has_SelectorStateConstantArray() ? machine.SelectorStateConstantArray.Count : -1)}");
				for (int si = 0; si < machine.StateConstantArray.Count; si++)
				{
					var stateConstant = machine.StateConstantArray[si].Data;
					uint fullPathID = stateConstant.GetId();
					string fullPath = controller.TOS.TryGetValue(fullPathID, out var pathName) ? pathName.String : "<missing>";
					writer.WriteLine($"SCONST\t{controller.PathID}\t{mi}\t{si}\t{fullPathID}\t{Escape(fullPath)}");
				}
				if (machine.Has_SelectorStateConstantArray())
				{
					for (int qi = 0; qi < machine.SelectorStateConstantArray.Count; qi++)
					{
						var selector = machine.SelectorStateConstantArray[qi].Data;
						writer.WriteLine($"SEL\t{controller.PathID}\t{mi}\t{qi}\t{selector.FullPathID}\t{(selector.IsEntry ? 1 : 0)}");
					}
				}
				for (int si = 0; si < machine.StateConstantArray.Count; si++)
				{
					var stateConstant = machine.StateConstantArray[si].Data;
					if (stateConstant.BlendTreeConstantArray.Count == 0)
					{
						continue;
					}
					var treeConstant = stateConstant.BlendTreeConstantArray[0].Data;
					for (int ni = 0; ni < treeConstant.NodeArray.Count; ni++)
					{
						var node = treeConstant.NodeArray[ni].Data;
						string children = string.Join(",", node.ChildIndices.Select(id => id.ToString()));
						writer.WriteLine($"BNODE\t{controller.PathID}\t{mi}\t{si}\t{ni}\t{node.BlendType}\t{children}\t{node.ClipID}\t{node.Duration}\t{(node.Mirror ? 1 : 0)}\t{node.CycleOffset}");
					}
				}
			}
		}
	}

	static void DumpControllers(GameData gameData, StreamWriter writer)
	{
		List<IAnimatorController> controllers = [];
		foreach (IUnityObjectBase asset in gameData.GameBundle.FetchAssets())
		{
			if (asset is IAnimatorController controller)
			{
				controllers.Add(controller);
			}
		}
		controllers.Sort(static (a, b) => a.PathID.CompareTo(b.PathID));
		foreach (IAnimatorController controller in controllers)
		{
			Dictionary<int, int> counts = [];
			HashSet<IUnityObjectBase> distinct = [];
			int total = 0;
			foreach (IUnityObjectBase member in controller.FetchEditorHierarchy())
			{
				total++;
				distinct.Add(member);
				counts.TryGetValue(member.ClassID, out int count);
				counts[member.ClassID] = count + 1;
			}
			Dictionary<int, int> distinctCounts = [];
			foreach (IUnityObjectBase member in distinct)
			{
				distinctCounts.TryGetValue(member.ClassID, out int count);
				distinctCounts[member.ClassID] = count + 1;
			}
			string histogram = string.Join(",", counts.OrderBy(static pair => pair.Key).Select(static pair => $"{pair.Key}:{pair.Value}"));
			string distinctHistogram = string.Join(",", distinctCounts.OrderBy(static pair => pair.Key).Select(static pair => $"{pair.Key}:{pair.Value}"));
			writer.WriteLine($"CONTROLLER\t{controller.PathID}\t{Escape(controller.GetBestName())}\t{Escape(controller.Collection.Name)}\t{total}\t{histogram}\t{distinct.Count}\t{distinctHistogram}");
			DumpAttachment(controller, writer);
		}
	}

	static void DumpAttachment(IAnimatorController controller, StreamWriter writer)
	{
		HashSet<IUnityObjectBase> seen = [];
		foreach (IUnityObjectBase member in controller.FetchEditorHierarchy())
		{
			if (member is not AssetRipper.SourceGenerated.Classes.ClassID_1107.IAnimatorStateMachine machine || !seen.Add(machine))
			{
				continue;
			}
			List<string> childStates = [];
			if (machine.Has_ChildStates())
			{
				foreach (var child in machine.ChildStates)
				{
					childStates.Add(child.State.TryGetAsset(machine.Collection, out var state) && state is not null ? state.PathID.ToString() : "null");
				}
			}
			else
			{
				foreach (var state in machine.StatesP)
				{
					childStates.Add(state is not null ? state.PathID.ToString() : "null");
				}
			}
			List<string> childMachines = [];
			if (machine.Has_ChildStateMachines())
			{
				foreach (var child in machine.ChildStateMachines)
				{
					childMachines.Add(child.StateMachine.TryGetAsset(machine.Collection, out var sub) && sub is not null ? sub.PathID.ToString() : "null");
				}
			}
			writer.WriteLine($"ATTACH\t{controller.PathID}\t{machine.PathID}\t{Escape(machine.GetBestName())}\t{string.Join(",", childStates)}\t{string.Join(",", childMachines)}");
		}
	}

	static void DumpMembers(GameData gameData, StreamWriter writer, string phase)
	{
		List<IUnityObjectBase> members = [];
		foreach (IUnityObjectBase asset in gameData.GameBundle.FetchAssets())
		{
			if (MemberClasses.Contains(asset.ClassID))
			{
				members.Add(asset);
			}
		}
		members.Sort(static (a, b) => a.ClassID != b.ClassID ? a.ClassID.CompareTo(b.ClassID) : a.PathID.CompareTo(b.PathID));
		foreach (IUnityObjectBase asset in members)
		{
			IUnityObjectBase? main = asset.MainAsset;
			writer.WriteLine($"{phase}-MEMBER\t{asset.ClassID}\t{asset.PathID}\t{Escape(asset.GetBestName())}\t{Escape(asset.Collection.Name)}\t{(main is null ? "null" : main.ClassID.ToString())}\t{(main is null ? "null" : main.PathID.ToString())}\t{(main is null ? "null" : Escape(main.GetBestName()))}");
		}
	}

	static string Escape(string? value) => (value ?? string.Empty).Replace('\t', ' ').Replace('\r', ' ').Replace('\n', ' ');

	static int Main(string[] args)
	{
		try
		{
			return Run(args);
		}
		catch (Exception exception)
		{
			Console.Error.WriteLine(exception);
			return 1;
		}
	}
}
