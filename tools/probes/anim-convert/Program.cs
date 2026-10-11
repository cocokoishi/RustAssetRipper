using System.Reflection;
using System.Text;
using AssetRipper.Assets;
using AssetRipper.Assets.Bundles;
using AssetRipper.Assets.Collections;
using AssetRipper.Assets.IO.Writing;
using AssetRipper.Import.AssetCreation;
using AssetRipper.Import.Structure.Assembly.Managers;
using AssetRipper.IO.Endian;
using AssetRipper.IO.Files;
using AssetRipper.IO.Files.SerializedFiles;
using AssetRipper.IO.Files.SerializedFiles.Parser;
using AssetRipper.SourceGenerated.Extensions;

// Reference-runtime probe for the animation processing workstream.
//
// Modes:
//   fieldhashes <out>   compact dump of the generated FieldHashes tables
//   props <TypeName>    list public members of a shipped type
//   types <fragment>    list shipped type names containing a fragment
//   convert <out>       build a real AnimationClip graph, run the shipped
//                       AnimationClipConverter, and dump the complete result
//   workspace <out> <serialized-file>
//                       write the same generated non-default graph as a no-TypeTree
//                       SerializedFile before conversion
//   workspace-existing-curves <out> <serialized-file>
//                       include pre-existing vector, quaternion, float, and PPtr curves
//   controller-nested-state-machine <out>
//                       reconstruct a compiled controller with an empty intermediate machine,
//                       nested/sibling machines, and entry/exit/any/state transitions
public static class Program
{
    private static Assembly Sg = null!;
    private static Assembly AssetsAsm = null!;
    private static Assembly ProcessingAsm = null!;

    public static int Main(string[] args)
    {
        Sg = typeof(AssetRipper.SourceGenerated.Classes.ClassID_74.AnimationClip).Assembly;
        AssetsAsm = typeof(AssetRipper.Assets.Metadata.AssetInfo).Assembly;
        ProcessingAsm = typeof(AssetRipper.Processing.AnimationClips.AnimationClipConverter).Assembly;
        string mode = args.Length > 0 ? args[0] : "fieldhashes";
        switch (mode)
        {
            case "fieldhashes":
                return DumpFieldHashes(args[1]);
            case "props":
                return DumpProps(args[1]);
            case "enum":
                Type enumType = Resolve(args[1]);
                foreach (object value in Enum.GetValues(enumType))
                {
                    Console.WriteLine($"{enumType.FullName}|{value}|{Convert.ToInt64(value)}");
                }
                return 0;
            case "types":
                return DumpTypes(args.Length > 1 ? args[1] : "AnimationClip");
            case "convert":
                try
                {
                    return RunConvert(args[1]);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine(ex.ToString());
                    return 3;
                }
            case "workspace":
                try
                {
                    return RunConvert(args[1], args[2]);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine(ex.ToString());
                    return 3;
                }
            case "workspace-existing-curves":
                try
                {
                    return RunConvert(args[1], args[2], includeExistingCurves: true);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine(ex.ToString());
                    return 3;
                }
            case "workspace-external-script":
                if (args.Length != 4)
                {
                    Console.Error.WriteLine("usage: anim-convert workspace-external-script <animation-file> <script-serialized-file> <assembly-file>");
                    return 2;
                }
                try
                {
                    return RunConvert(
                        string.Empty,
                        args[1],
                        includeExistingCurves: true,
                        externalScriptSerializedFile: args[2],
                        externalScriptAssemblyFile: args[3]);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine(ex.ToString());
                    return 3;
                }
            case "controller-blend-tree":
                try
                {
                    return DumpControllerBlendTree(args[1]);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine(ex.ToString());
                    return 3;
                }
            case "controller-nested-blend-tree":
                try
                {
                    return DumpControllerBlendTree(args[1], nested: true);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine(ex.ToString());
                    return 3;
                }
            case "controller-nested-state-machine":
                if (args.Length is not (2 or 3))
                {
                    Console.Error.WriteLine("usage: anim-convert controller-nested-state-machine <out> [editor-input-hex]");
                    return 2;
                }
                try
                {
                    return DumpControllerNestedStateMachine(args[1], args.Length == 3 ? args[2] : null);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine(ex.ToString());
                    return 3;
                }
            case "controller-empty-state-machine":
                if (args.Length != 2)
                {
                    Console.Error.WriteLine("usage: anim-convert controller-empty-state-machine <out>");
                    return 2;
                }
                try
                {
                    return DumpControllerNestedStateMachine(args[1], emptyMachines: true);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine(ex.ToString());
                    return 3;
                }
            case "controller-behaviour-state-machine":
                if (args.Length is not (2 or 3))
                {
                    Console.Error.WriteLine("usage: anim-convert controller-behaviour-state-machine <out> [editor-input-hex]");
                    return 2;
                }
                try
                {
                    return DumpControllerNestedStateMachine(args[1], args.Length == 3 ? args[2] : null, behaviourBindings: true);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine(ex.ToString());
                    return 3;
                }
            case "controller-shared-editor-state-machine":
                if (args.Length != 3)
                {
                    Console.Error.WriteLine("usage: anim-convert controller-shared-editor-state-machine <out> <editor-input-hex>");
                    return 2;
                }
                try
                {
                    return DumpControllerNestedStateMachine(args[1], args[2], behaviourBindings: true, sharedEditorInput: true);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine(ex.ToString());
                    return 3;
                }
            case "controller-dual-release-clone":
                if (args.Length != 2)
                {
                    Console.Error.WriteLine("usage: anim-convert controller-dual-release-clone <out>");
                    return 2;
                }
                try
                {
                    return DumpControllerNestedStateMachine(
                        args[1], behaviourBindings: true, dualReleaseControllers: true);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine(ex.ToString());
                    return 3;
                }
            default:
                Console.Error.WriteLine($"unknown mode {mode}");
                return 2;
        }
    }

    private static Type Resolve(string name)
    {
        foreach (Assembly asm in new[] { Sg, AssetsAsm, ProcessingAsm })
        {
            Type? type = asm.GetType(name);
            if (type is not null)
            {
                return type;
            }
        }
        foreach (Assembly asm in new[] { Sg, AssetsAsm, ProcessingAsm })
        {
            Type? type = asm.GetTypes().FirstOrDefault(t => t.Name == name);
            if (type is not null)
            {
                return type;
            }
        }
        throw new InvalidOperationException($"type {name} not found");
    }

    private static int DumpTypes(string fragment)
    {
        foreach (Type type in Sg.GetTypes().Where(t => (t.FullName ?? t.Name).Contains(fragment, StringComparison.Ordinal)).OrderBy(t => t.FullName))
        {
            Console.WriteLine(type.FullName);
        }
        return 0;
    }

    private static int DumpControllerBlendTree(string output, bool nested = false)
    {
        var bundle = new AssetRipper.Assets.Bundles.GameBundle();
        var collection = new AssetRipper.Assets.Collections.ProcessedAssetCollection(bundle);
        object version = AssetRipper.Primitives.UnityVersion.Parse("2022.3.0f1");
        collection.SetLayout((AssetRipper.Primitives.UnityVersion)version);
        collection.FilePath = "source_blend_tree.assets";

        object controller = NewAsset(
            typeof(AssetRipper.SourceGenerated.Classes.ClassID_91.AnimatorController),
            collection,
            10000,
            91,
            version);
        object firstClip = NewAsset(
            typeof(AssetRipper.SourceGenerated.Classes.ClassID_74.AnimationClip),
            collection,
            10010,
            74,
            version);
        object secondClip = NewAsset(
            typeof(AssetRipper.SourceGenerated.Classes.ClassID_74.AnimationClip),
            collection,
            10011,
            74,
            version);
        object? thirdClip = nested
            ? NewAsset(
                typeof(AssetRipper.SourceGenerated.Classes.ClassID_74.AnimationClip),
                collection,
                10012,
                74,
                version)
            : null;
        SetAsset(AddNew(Get(controller, "AnimationClips")!), firstClip);
        SetAsset(AddNew(Get(controller, "AnimationClips")!), secondClip);
        if (thirdClip is not null)
        {
            SetAsset(AddNew(Get(controller, "AnimationClips")!), thirdClip);
        }

        Type stateType = Resolve("AssetRipper.SourceGenerated.Subclasses.StateConstant.StateConstant_5");
        object state = Activator.CreateInstance(stateType)!;
        object treeOffset = AddNew(Get(state, "BlendTreeConstantArray")!);
        object treeConstant = Get(treeOffset, "Data")!;
        object nodes = Get(treeConstant, "NodeArray")!;
        object rootOffset = AddNew(nodes);
        object root = Get(rootOffset, "Data")!;
        Set(root, "BlendType", 0u);
        uint speedParameter = Crc32("Speed");
        Set(root, "BlendEventID", speedParameter);
        Set(root, "BlendEventYID", uint.MaxValue);
        object tos = Get(controller, "TOS")!;
        MethodInfo addTos = tos.GetType().GetMethods().First(method =>
            method.Name == "Add" && method.GetParameters().Length == 2);
        addTos.Invoke(tos, new object[] { speedParameter, new AssetRipper.Primitives.Utf8String("Speed") });
        AddItem(Get(root, "ChildIndices")!, 1u);
        AddItem(Get(root, "ChildIndices")!, 2u);
        object blend1d = Get(Get(root, "Blend1dData")!, "Data")!;
        AddItem(Get(blend1d, "ChildThresholdArray")!, -0.5f);
        AddItem(Get(blend1d, "ChildThresholdArray")!, 0.75f);

        if (nested)
        {
            object nestedOffset = AddNew(nodes);
            object nestedNode = Get(nestedOffset, "Data")!;
            Set(nestedNode, "BlendType", 0u);
            Set(nestedNode, "CycleOffset", 0.0f);
            uint nestedSpeedParameter = Crc32("NestedSpeed");
            Set(nestedNode, "BlendEventID", nestedSpeedParameter);
            Set(nestedNode, "BlendEventYID", uint.MaxValue);
            MethodInfo addNestedTos = tos.GetType().GetMethods().First(method =>
                method.Name == "Add" && method.GetParameters().Length == 2);
            addNestedTos.Invoke(tos, new object[] { nestedSpeedParameter, new AssetRipper.Primitives.Utf8String("NestedSpeed") });
            AddItem(Get(nestedNode, "ChildIndices")!, 3u);
            AddItem(Get(nestedNode, "ChildIndices")!, 4u);
            object nestedBlend1d = Get(Get(nestedNode, "Blend1dData")!, "Data")!;
            AddItem(Get(nestedBlend1d, "ChildThresholdArray")!, 0.25f);
            AddItem(Get(nestedBlend1d, "ChildThresholdArray")!, 1.25f);
        }

        object firstOffset = AddNew(nodes);
        object firstNode = Get(firstOffset, "Data")!;
        Set(firstNode, "ClipID", 0u);
        Set(firstNode, "Duration", 2.0f);
        Set(firstNode, "CycleOffset", 0.125f);
        Set(firstNode, "Mirror", false);
        object secondOffset = AddNew(nodes);
        object secondNode = Get(secondOffset, "Data")!;
        Set(secondNode, "ClipID", 1u);
        Set(secondNode, "Duration", 4.0f);
        Set(secondNode, "CycleOffset", 0.25f);
        Set(secondNode, "Mirror", true);
        if (nested)
        {
            object thirdOffset = AddNew(nodes);
            object thirdNode = Get(thirdOffset, "Data")!;
            Set(thirdNode, "ClipID", 2u);
            Set(thirdNode, "Duration", 4.0f);
            Set(thirdNode, "CycleOffset", 0.375f);
            Set(thirdNode, "Mirror", false);
        }

        Type factory = ProcessingAsm.GetType("AssetRipper.Processing.AnimatorControllers.VirtualAnimationFactory")!;
        object sourceTree = factory.GetMethod(
                "CreateBlendTree",
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)!
            .Invoke(null, new[] { collection, controller, state, 0 })!;

        var rows = new StringBuilder();
        AppendBlendTreeProjection(rows, nested ? "ROOT" : "TREE", sourceTree);
        if (nested)
        {
            object nestedSourceTree = factory.GetMethod(
                    "CreateBlendTree",
                    BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)!
                .Invoke(null, new[] { collection, controller, state, 1 })!;
            AppendBlendTreeProjection(rows, "NESTED", nestedSourceTree);
        }

        string fullPath = Path.GetFullPath(output);
        Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
        File.WriteAllText(fullPath, rows.ToString());
        Console.WriteLine($"wrote source AnimatorController BlendTree rows={rows.ToString().Split('\n', StringSplitOptions.RemoveEmptyEntries).Length} path={fullPath}");
        return 0;
    }

    private static void AppendBlendTreeProjection(StringBuilder rows, string record, object sourceTree)
    {
        AppendSourceRow(rows, record, "class", Get(sourceTree, "ClassID"));
        AppendSourceRow(rows, record, "serializedVersion", Get(sourceTree, "SerializedVersion"));
        AppendSourceRow(rows, record, "hideFlags", Get(sourceTree, "HideFlags"));
        AppendSourceRow(rows, record, "name", Get(sourceTree, "Name"));
        AppendSourceRow(rows, record, "blendParameter", Get(sourceTree, "BlendParameter"));
        AppendSourceRow(rows, record, "blendParameterY", Get(sourceTree, "BlendParameterY"));
        AppendSourceRow(rows, record, "blendType", Get(sourceTree, "BlendType_Int32") ?? Get(sourceTree, "BlendType_UInt32"));
        AppendSourceRow(rows, record, "minThreshold", Hex(Convert.ToSingle(Get(sourceTree, "MinThreshold"))));
        AppendSourceRow(rows, record, "maxThreshold", Hex(Convert.ToSingle(Get(sourceTree, "MaxThreshold"))));
        AppendSourceRow(rows, record, "useAutomaticThresholds", Get(sourceTree, "UseAutomaticThresholds"));
        AppendSourceRow(rows, record, "normalizedBlendValues", Get(sourceTree, "NormalizedBlendValues"));

        object children = Get(sourceTree, "Childs")!;
        for (int i = 0; i < Count(children); i++)
        {
            object child = At(children, i);
            object position = Get(child, "Position")!;
            var fields = new[]
            {
                record,
                i.ToString(),
                Convert.ToString(Get(child, "SerializedVersion"), System.Globalization.CultureInfo.InvariantCulture) ?? "missing",
                Hex(Convert.ToSingle(Get(child, "Threshold"))),
                Hex(Convert.ToSingle(Get(position, "X") ?? Get(position, "x"))),
                Hex(Convert.ToSingle(Get(position, "Y") ?? Get(position, "y"))),
                Hex(Convert.ToSingle(Get(child, "TimeScale"))),
                Hex(Convert.ToSingle(Get(child, "CycleOffset"))),
                Convert.ToString(Get(child, "DirectBlendParameter"), System.Globalization.CultureInfo.InvariantCulture) ?? string.Empty,
                Convert.ToBoolean(Get(child, "Mirror")) ? "1" : "0",
                Convert.ToBoolean(Get(child, "IsAnim")) ? "1" : "0",
            };
            rows.Append("CHILD|").Append(string.Join('|', fields)).Append('\n');
        }
    }

    private static void AppendSourceRow(StringBuilder builder, string record, string name, object? value)
    {
        string text = Convert.ToString(value, System.Globalization.CultureInfo.InvariantCulture) ?? string.Empty;
        builder.Append(record).Append('|').Append(name).Append('|').Append(text).Append('\n');
    }

    private static int DumpControllerNestedStateMachine(string output, string? editorInputOutput = null, bool emptyMachines = false, bool behaviourBindings = false, bool sharedEditorInput = false, bool dualReleaseControllers = false)
    {
        var bundle = new AssetRipper.Assets.Bundles.GameBundle();
        var collection = new ProcessedAssetCollection(bundle);
        var version = AssetRipper.Primitives.UnityVersion.Parse("2022.3.0f1");
        collection.SetLayout(version);
        collection.FilePath = "source_nested_state_machine.assets";
        var controller = (AssetRipper.SourceGenerated.Classes.ClassID_91.IAnimatorController)NewAsset(
            typeof(AssetRipper.SourceGenerated.Classes.ClassID_91.AnimatorController), collection,
            10000, 91, version);
        controller.Name_R = new AssetRipper.Primitives.Utf8String("Nested State Machine Source");
        controller.TOS.Add(0, AssetRipper.Primitives.Utf8String.Empty);
        var sourceControllers = new List<AssetRipper.SourceGenerated.Classes.ClassID_91.IAnimatorController> { controller };

        uint Register(string text)
        {
            uint hash = Crc32(text);
            controller.TOS.TryAdd(hash, new AssetRipper.Primitives.Utf8String(text));
            return hash;
        }

        var constant = controller.Controller;
        var layer = constant.LayerArray.AddNew().Data;
        string rootName = emptyMachines ? "主层" : "Base";
        layer.Binding = Register(rootName);
        layer.StateMachineIndex = 0;
        layer.DefaultWeight = 1.0f;
        layer.IKPass = true;
        foreach (var (name, type, index) in new (string, uint, uint)[]
        {
            ("Speed", 1, 0), ("Moving", 4, 0), ("Mode", 3, 0), ("Trigger", 9, 1),
        })
        {
            var parameter = constant.Values.Data.ValueArray.AddNew();
            parameter.ID = Register(name);
            parameter.Type = type;
            parameter.Index = index;
        }
        constant.DefaultValues.Data.FloatValues.Add(0.25f);
        constant.DefaultValues.Data.BoolValues.Add(true);
        constant.DefaultValues.Data.BoolValues.Add(false);
        constant.DefaultValues.Data.IntValues.Add(2);
        var compiled = constant.StateMachineArray.AddNew().Data;
        compiled.DefaultState = 0;

        string[] statePaths = { "Base.Idle", "Base.Locomotion.Deep.Walk", "Base.Locomotion.Deep.Run", "Base.Combat.Ready" };
        if (emptyMachines)
        {
            statePaths = statePaths.Select(path => rootName + path[4..]).ToArray();
        }
        for (int i = 0; i < statePaths.Length; i++)
        {
            var state = compiled.StateConstantArray.AddNew().Data;
            state.FullPathID = Register(statePaths[i]);
            state.NameID = Register(statePaths[i][(statePaths[i].LastIndexOf('.') + 1)..]);
            state.Speed = 1.0f + i * 0.25f;
            state.CycleOffset = i * 0.125f;
            state.WriteDefaultValues = true;
        }

        string[] machinePaths = { "Base", "Base.Locomotion", "Base.Locomotion.Deep", "Base.Combat" };
        if (emptyMachines)
        {
            machinePaths = machinePaths.Concat(new[]
            {
                "Base.Locomotion.Deep.失名", "Base.Locomotion.Deep.失名.子",
                "Base.Locomotion.Deep.DirectExit", "Base.Locomotion.Deep.Linked",
                "Base.Locomotion.Restricted", "Base.Disconnected", "Base.Orphan",
            }).Select(path => rootName + path[4..]).ToArray();
        }
        foreach (string path in machinePaths)
        {
            // Nested machine path text is deliberately not inserted into TOS: the source must recover
            // the empty intermediate machine's path from its descendant state's full path.
            foreach (bool isEntry in new[] { true, false })
            {
                var selector = compiled.SelectorStateConstantArray!.AddNew().Data;
                selector.FullPathID = Crc32(path);
                selector.IsEntry = isEntry;
            }
        }

        void Condition(object transition, string parameter, uint mode, float threshold)
        {
            object condition = Get(AddNew(Get(transition, "ConditionConstantArray")!), "Data")!;
            Set(condition, "EventID", Register(parameter));
            Set(condition, "ConditionMode", mode);
            Set(condition, "EventThreshold", threshold);
        }

        void Selector(int index, uint destination, string? parameter = null, uint mode = 1, float threshold = 0)
        {
            var transition = compiled.SelectorStateConstantArray![index].Data.TransitionConstantArray.AddNew().Data;
            transition.Destination = destination;
            if (parameter is not null)
            {
                Condition(transition, parameter, mode, threshold);
            }
        }

        Selector(0, 30002, "Moving");
        Selector(0, 0);
        Selector(2, 30004, "Mode", 6, 2.0f);
        Selector(2, 1);
        Selector(3, 0); // unconditional return to Base's default: omitted by the source
        Selector(4, 2, "Speed", 3, 0.75f);
        Selector(4, 1);
        Selector(5, 3, "Speed", 3, 0.75f);
        Selector(5, 30003); // exit from Deep through Locomotion's exit selector
        Selector(6, 30004, "Trigger");
        Selector(6, 3);
        Selector(7, 30002, "Moving");
        Selector(7, 30001); // exit from Combat through Base's exit selector

        void StateTransition(int stateIndex, uint destination, string name, string parameter, uint mode = 1, float threshold = 0)
        {
            var transition = compiled.StateConstantArray[stateIndex].Data.TransitionConstantArray.AddNew().Data;
            transition.DestinationState = destination;
            transition.UserID = Register(name);
            transition.TransitionDuration = 0.25f;
            transition.TransitionOffset = 0.125f;
            transition.ExitTime = 0.75f;
            transition.HasExitTime = stateIndex == 2;
            transition.HasFixedDuration = true;
            transition.OrderedInterruption = true;
            transition.CanTransitionToSelf = false;
            Condition(transition, parameter, mode, threshold);
        }

        StateTransition(0, 30002, "EnterLocomotion", "Moving");
        StateTransition(1, 2, "WalkToRun", "Speed", 3, 0.75f);
        StateTransition(2, 30006, "EnterCombat", "Mode", 6, 2.0f);
        StateTransition(2, 30005, "ExitDeep", "Moving", 2);
        StateTransition(3, 30004, "ReturnToDeep", "Trigger");
        foreach (uint destination in new uint[] { 2, 30006 })
        {
            var transition = compiled.AnyStateTransitionConstantArray.AddNew().Data;
            transition.DestinationState = destination;
            transition.UserID = Register(destination == 2 ? "AnyToRun" : "AnyToCombat");
            transition.TransitionDuration = 0.5f;
            transition.CanTransitionToSelf = true;
            Condition(transition, "Trigger", 1, 0);
        }

        if (emptyMachines)
        {
            // Empty paths never enter TOS. These connections independently exercise
            // definitive parents, flagged chains, deeper proposals, sharing, parent
            // exclusions, a disconnected node, and the source's extra-machine rule.
            StateTransition(0, 30008, "OfferRootParent", "Moving");
            StateTransition(2, 30010, "OfferDeeperParentThroughChild", "Trigger");
            StateTransition(2, 30016, "OfferExcludedParent", "Moving");
            Selector(8, 30014, "Moving");
            Selector(8, uint.MaxValue);
            Selector(11, 30009); // definitive child -> unknown parent's exit
            Selector(13, 30005); // definitive child -> known parent's exit
            Selector(17, 30004); // cannot directly parent Restricted under Deep
            Selector(1, 30020, "Moving"); // Orphan cannot be under the root path
            Selector(21, 30000); // Orphan cannot directly parent under the root either
        }

        var behaviourAssets = new List<AssetRipper.SourceGenerated.Classes.ClassID_114.IMonoBehaviour>();
        if (behaviourBindings)
        {
            foreach (string name in new[] { "Root Behaviour", "State Behaviour" })
            {
                var behaviour = (AssetRipper.SourceGenerated.Classes.ClassID_114.IMonoBehaviour)NewAsset(
                    typeof(AssetRipper.SourceGenerated.Classes.ClassID_114.MonoBehaviour), collection,
                    20000 + behaviourAssets.Count, 114, version);
                behaviour.Name_R = new AssetRipper.Primitives.Utf8String(name);
                behaviour.Enabled = 1;
                behaviourAssets.Add(behaviour);
                controller.StateMachineBehavioursP.Add(behaviour);
            }
            controller.StateMachineBehavioursP.Add(null);
            var description = controller.StateMachineBehaviourVectorDescription
                ?? throw new InvalidOperationException("source controller has no behaviour description");
            foreach (uint slot in new uint[] { 1, 0, 2, 0, 1, 2 })
            {
                description.StateMachineBehaviourIndices.Add(slot);
            }
            void Range(uint path, uint start, uint count)
            {
                object pair = AddNew(description.StateMachineBehaviourRanges);
                object key = Get(pair, "Key")!;
                object range = Get(pair, "Value")!;
                Set(key, "LayerIndex", 0);
                Set(key, "StateID", path);
                Set(range, "StartIndex", start);
                Set(range, "Count", count);
            }
            Range(Crc32(statePaths[0]), 0, 3); // states retain explicit null slot
            Range(Crc32(machinePaths[0]), 3, 3); // machines omit the null slot
            Range(Crc32(machinePaths[2]), 0, 1); // same behaviour bound on a child machine
        }
        if (dualReleaseControllers)
        {
            var second = (AssetRipper.SourceGenerated.Classes.ClassID_91.IAnimatorController)NewAsset(
                typeof(AssetRipper.SourceGenerated.Classes.ClassID_91.AnimatorController), collection,
                10001, 91, version);
            second.CopyValues(controller, new AssetRipper.Assets.Cloning.PPtrConverter(collection, collection));
            second.Name_R = new AssetRipper.Primitives.Utf8String("Nested State Machine Source Second");
            sourceControllers.Add(second);
        }

        byte[] releasePayload;
        using (var payload = new MemoryStream())
        {
            using (var writer = new AssetWriter(payload, collection))
            {
                controller.WriteRelease(writer);
            }
            releasePayload = payload.ToArray();
        }
        var sourceBuilder = new SerializedFileBuilder
        {
            Generation = FormatVersion.Unknown_10,
            Version = version,
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = false,
        };
        var controllerType = new SerializedType { RawTypeID = 91 };
        sourceBuilder.Types.Add(controllerType);
        foreach (var sourceController in sourceControllers)
        {
            byte[] controllerPayload;
            if (sourceController == controller)
            {
                controllerPayload = releasePayload;
            }
            else
            {
                using var payload = new MemoryStream();
                using (var writer = new AssetWriter(payload, collection))
                {
                    sourceController.WriteRelease(writer);
                }
                controllerPayload = payload.ToArray();
            }
            sourceBuilder.Objects.Add(new ObjectInfo(controllerType)
            {
                FileID = sourceController.PathID,
                TypeID = 91,
                ObjectData = controllerPayload,
            });
        }
        if (behaviourBindings)
        {
            var behaviourType = new SerializedType { RawTypeID = 114 };
            sourceBuilder.Types.Add(behaviourType);
            foreach (var behaviour in behaviourAssets)
            {
                using var payload = new MemoryStream();
                using (var writer = new AssetWriter(payload, collection)) { behaviour.WriteRelease(writer); }
                sourceBuilder.Objects.Add(new ObjectInfo(behaviourType)
                {
                    FileID = behaviour.PathID, TypeID = 114, ObjectData = payload.ToArray(),
                });
            }
        }
        var sourceFile = sourceBuilder.Build();
        sourceFile.FilePath = collection.FilePath;
        byte[] serializedBytes = sourceFile.ToByteArray();
        if (dualReleaseControllers)
        {
            return DumpDualReleaseControllerClone(output, sourceFile, serializedBytes, version);
        }

        // Invoke the frozen processor's per-controller path, so layer construction and graph
        // reconstruction run exactly as the production reference, without launching Unity.
        Type processor = ProcessingAsm.GetType("AssetRipper.Processing.AnimatorControllers.AnimatorControllerProcessor")!;
        processor.GetMethod("Process", BindingFlags.NonPublic | BindingFlags.Static, null,
            new[] { typeof(AssetRipper.SourceGenerated.Classes.ClassID_91.IAnimatorController), typeof(ProcessedAssetCollection) }, null)!
            .Invoke(null, new object[] { controller, collection });

        var root = controller.AnimatorLayers[0].StateMachine.TryGetAsset(collection)!;
        var identities = new Dictionary<IUnityObjectBase, string>();
        identities.Add(controller, "controller");
        void IndexMachine(AssetRipper.SourceGenerated.Classes.ClassID_1107.IAnimatorStateMachine machine, string path)
        {
            identities.Add(machine, path);
            foreach (var child in machine.ChildStates!)
            {
                var state = child.State.TryGetAsset(collection)!;
                identities.Add(state, path + "." + state.Name);
            }
            foreach (var child in machine.ChildStateMachines!)
            {
                var nestedMachine = child.StateMachine.TryGetAsset(collection)!;
                IndexMachine(nestedMachine, path + "." + nestedMachine.Name);
            }
        }
        IndexMachine(root, root.Name.String);
        foreach (var behaviour in behaviourAssets) { identities.Add(behaviour, "BEHAVIOUR:" + behaviour.Name); }
        string Identity(IUnityObjectBase? asset) => asset is null ? string.Empty : identities[asset];

        var rows = new StringBuilder();
        rows.Append("# Unity 2022.3.0f1 source-runtime AnimatorController hierarchy projection\n");
        rows.Append("# AnimatorControllerProcessor.Process -> VirtualAnimationFactory.CreateRootAnimatorStateMachine\n");
        rows.Append("# MACHINE/STATE/TRANSITION rows carry owner identity, field, value; CONDITION rows carry owner, index, mode, parameter, float bits.\n");
        rows.Append("# SOURCE_SERIALIZED_FILE_HEX contains a generated no-TypeTree SerializedFile before processing; SOURCE_YAML uses frozen YamlWalker with raw PPtr identities.\n");
        rows.Append("SOURCE_SERIALIZED_FILE_HEX|").Append(Convert.ToHexString(serializedBytes)).Append('\n');
        for (int i = 0; i < statePaths.Length; i++)
        {
            rows.Append("INPUT_STATE|").Append(i).Append('|').Append(statePaths[i]).Append('|').Append(Hex(Crc32(statePaths[i]))).Append('\n');
        }
        for (int i = 0; i < compiled.SelectorStateConstantArray!.Count; i++)
        {
            var selector = compiled.SelectorStateConstantArray[i].Data;
            rows.Append("INPUT_SELECTOR|").Append(i).Append('|').Append(machinePaths[i / 2]).Append('|').Append(selector.IsEntry ? "entry" : "exit").Append('\n');
            for (int j = 0; j < selector.TransitionConstantArray.Count; j++)
            {
                rows.Append("INPUT_SELECTOR_DESTINATION|").Append(i).Append('|').Append(j).Append('|').Append(selector.TransitionConstantArray[j].Data.Destination).Append('\n');
            }
        }

        void Transition(string owner, object transition)
        {
            identities.Add((IUnityObjectBase)transition, owner);
            AppendSourceRow(rows, "TRANSITION|" + owner, "class", Get(transition, "ClassID"));
            AppendSourceRow(rows, "TRANSITION|" + owner, "serializedVersion", Get(transition, "SerializedVersion"));
            AppendSourceRow(rows, "TRANSITION|" + owner, "name", Get(transition, "Name"));
            AppendSourceRow(rows, "TRANSITION|" + owner, "destinationState", Identity(Get(transition, "DestinationStateP") as IUnityObjectBase));
            AppendSourceRow(rows, "TRANSITION|" + owner, "destinationStateMachine", Identity(Get(transition, "DestinationStateMachineP") as IUnityObjectBase));
            AppendSourceRow(rows, "TRANSITION|" + owner, "isExit", Get(transition, "IsExit"));
            object conditions = Get(transition, "Conditions")!;
            for (int i = 0; i < Count(conditions); i++)
            {
                object condition = At(conditions, i);
                rows.Append("CONDITION|").Append(owner).Append('|').Append(i).Append('|')
                    .Append(Get(condition, "ConditionMode")).Append('|').Append(Get(condition, "ConditionEvent")).Append('|')
                    .Append(Hex(Convert.ToSingle(Get(condition, "EventTreshold")))).Append('\n');
            }
            foreach (string field in new[] { "TransitionDuration", "TransitionOffset", "ExitTime" })
            {
                if (Get(transition, field) is object value)
                {
                    AppendSourceRow(rows, "TRANSITION|" + owner, field, Hex(Convert.ToSingle(value)));
                }
            }
            foreach (string field in new[] { "HasExitTime", "HasFixedDuration", "InterruptionSource", "OrderedInterruption", "CanTransitionToSelf" })
            {
                if (Get(transition, field) is object value)
                {
                    AppendSourceRow(rows, "TRANSITION|" + owner, field, value);
                }
            }
        }

        void Machine(AssetRipper.SourceGenerated.Classes.ClassID_1107.IAnimatorStateMachine machine, string parent)
        {
            string path = Identity(machine);
            AppendSourceRow(rows, "MACHINE|" + path, "name", machine.Name);
            AppendSourceRow(rows, "MACHINE|" + path, "parent", parent);
            AppendSourceRow(rows, "MACHINE|" + path, "serializedVersion", machine.SerializedVersion);
            AppendSourceRow(rows, "MACHINE|" + path, "default", Identity(machine.DefaultStateP));
            AppendSourceRow(rows, "MACHINE|" + path, "stateCount", machine.ChildStates!.Count);
            AppendSourceRow(rows, "MACHINE|" + path, "machineCount", machine.ChildStateMachines!.Count);
            AppendSourceRow(rows, "MACHINE|" + path, "entryCount", machine.EntryTransitions!.Count);
            AppendSourceRow(rows, "MACHINE|" + path, "anyCount", machine.AnyStateTransitions!.Count);
            AppendSourceRow(rows, "MACHINE|" + path, "exitGroupCount", machine.StateMachineTransitions!.Count);
            foreach (var child in machine.ChildStates)
            {
                var state = child.State.TryGetAsset(collection)!;
                string statePath = Identity(state);
                rows.Append("CHILD_STATE|").Append(path).Append('|').Append(statePath);
                AppendVec(rows, child.Position, new[] { "X", "Y", "Z" });
                rows.Append('\n');
                AppendSourceRow(rows, "STATE|" + statePath, "speed", Hex(state.Speed));
                AppendSourceRow(rows, "STATE|" + statePath, "cycleOffset", Hex(state.CycleOffset));
                for (int i = 0; i < state.TransitionsP.Count; i++)
                {
                    Transition("STATE:" + statePath + ":" + i, state.TransitionsP[i]!);
                }
            }
            for (int i = 0; i < machine.EntryTransitionsP.Count; i++)
            {
                Transition("ENTRY:" + path + ":" + i, machine.EntryTransitionsP[i]!);
            }
            for (int i = 0; i < machine.AnyStateTransitionsP.Count; i++)
            {
                Transition("ANY:" + path + ":" + i, machine.AnyStateTransitionsP[i]!);
            }
            foreach (var pair in machine.StateMachineTransitions)
            {
                var childMachine = pair.Key.TryGetAsset(collection)!;
                rows.Append("EXIT_GROUP|").Append(path).Append('|').Append(Identity(childMachine)).Append('|').Append(pair.Value.Count).Append('\n');
                for (int i = 0; i < pair.Value.Count; i++)
                {
                    Transition("EXIT:" + path + ":" + Identity(childMachine) + ":" + i, pair.Value[i].TryGetAsset(collection)!);
                }
            }
            foreach (var child in machine.ChildStateMachines)
            {
                var childMachine = child.StateMachine.TryGetAsset(collection)!;
                rows.Append("CHILD_MACHINE|").Append(path).Append('|').Append(Identity(childMachine));
                AppendVec(rows, child.Position, new[] { "X", "Y", "Z" });
                rows.Append('\n');
                Machine(childMachine, path);
            }
        }
        Machine(root, string.Empty);
        AppendControllerSourceYaml(rows, collection, version, identities);
        if (sharedEditorInput)
        {
            var shared = (AssetRipper.SourceGenerated.Classes.ClassID_91.IAnimatorController)NewAsset(
                typeof(AssetRipper.SourceGenerated.Classes.ClassID_91.AnimatorController), collection,
                10001, 91, version);
            shared.CopyValues(controller, new AssetRipper.Assets.Cloning.PPtrConverter(collection, collection));
            shared.Name_R = new AssetRipper.Primitives.Utf8String("Shared State Machine Source");
            foreach (var parameter in shared.AnimatorParameters)
            {
                SetAsset(parameter.Controller ?? throw new InvalidOperationException("2022.3 parameter has no controller pointer"), shared);
            }
            foreach (var sharedLayer in shared.AnimatorLayers)
            {
                SetAsset(sharedLayer.Controller ?? throw new InvalidOperationException("2022.3 layer has no controller pointer"), shared);
            }
            identities.Add(shared, "shared-controller");
        }
        if (editorInputOutput is not null)
        {
            var editorBuilder = new SerializedFileBuilder
            {
                Generation = FormatVersion.Unknown_10,
                Version = version,
                Platform = BuildTarget.NoTarget,
                EndianType = EndianType.LittleEndian,
                HasTypeTree = false,
            };
            var editorTypes = new Dictionary<int, SerializedType>();
            foreach (IUnityObjectBase asset in identities.Keys.OrderBy(asset => asset.PathID))
            {
                if (!editorTypes.TryGetValue(asset.ClassID, out SerializedType? type))
                {
                    type = new SerializedType { RawTypeID = asset.ClassID };
                    editorTypes.Add(asset.ClassID, type);
                    editorBuilder.Types.Add(type);
                }
                using var payload = new MemoryStream();
                using (var writer = new AssetWriter(payload, collection))
                {
                    asset.WriteEditor(writer);
                }
                editorBuilder.Objects.Add(new ObjectInfo(type)
                {
                    FileID = asset.PathID,
                    TypeID = asset.ClassID,
                    ObjectData = payload.ToArray(),
                });
            }
            var editorFile = editorBuilder.Build();
            editorFile.FilePath = "source_nested_state_machine_editor.assets";
            string editorPath = Path.GetFullPath(editorInputOutput);
            Directory.CreateDirectory(Path.GetDirectoryName(editorPath)!);
            File.WriteAllText(editorPath,
                $"# Frozen AssetRipper WriteEditor; Unity 2022.3.0f1, NoTarget, no TypeTree, {identities.Count} objects.\n"
                + Convert.ToHexString(editorFile.ToByteArray()) + "\n", new UTF8Encoding(false));
        }
        string fullPath = Path.GetFullPath(output);
        Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
        File.WriteAllText(fullPath, rows.ToString(), new UTF8Encoding(false));
        Console.WriteLine($"wrote source nested AnimatorController machines={machinePaths.Length} states={statePaths.Length} path={fullPath}");
        return 0;
    }

    private static int DumpDualReleaseControllerClone(
        string output, SerializedFile sourceFile, byte[] serializedBytes,
        AssetRipper.Primitives.UnityVersion version)
    {
        var bundle = new GameBundle();
        var factory = new GameAssetFactory(new BaseManager(_ => { }));
        var inputCollection = bundle.AddCollectionFromSerializedFile(sourceFile, factory);
        inputCollection.FilePath = sourceFile.FilePath;
        object gameData = Activator.CreateInstance(
            ProcessingAsm.GetType("AssetRipper.Processing.GameData")!,
            bundle, version, null, null)!;
        Type processorType = ProcessingAsm.GetType("AssetRipper.Processing.AnimatorControllers.AnimatorControllerProcessor")!;
        object processor = Activator.CreateInstance(processorType)!;
        Type gameDataType = ProcessingAsm.GetType("AssetRipper.Processing.GameData")!;
        processorType.GetMethod("Process", BindingFlags.Public | BindingFlags.Instance, null,
            new[] { gameDataType }, null)!.Invoke(processor, new[] { gameData });

        var controllers = inputCollection
            .OfType<AssetRipper.SourceGenerated.Classes.ClassID_91.IAnimatorController>()
            .OrderBy(controller => controller.PathID)
            .ToArray();
        if (controllers.Length != 2)
        {
            throw new InvalidOperationException($"expected two release controllers, found {controllers.Length}");
        }
        ProcessedAssetCollection generated = bundle.FetchAssetCollections()
            .OfType<ProcessedAssetCollection>()
            .Single(collection => collection.Name == "Generated AnimatorController Dependencies");
        var clonedBehaviours = generated
            .OfType<AssetRipper.SourceGenerated.Classes.ClassID_114.IMonoBehaviour>()
            .OrderBy(behaviour => behaviour.PathID)
            .ToArray();
        if (clonedBehaviours.Length != 2)
        {
            throw new InvalidOperationException($"expected two source controller behaviour clones, found {clonedBehaviours.Length}");
        }

        var rows = new StringBuilder();
        rows.Append("# Frozen AssetRipper 2022.3.0f1 dual release AnimatorController clone replay.\n");
        rows.Append("# SOURCE_SERIALIZED_FILE_HEX is the two-controller release input before processing.\n");
        rows.Append("SOURCE_SERIALIZED_FILE_HEX|").Append(Convert.ToHexString(serializedBytes)).Append('\n');
        foreach (var controller in controllers)
        {
            string name = controller.Name_R.String;
            var hierarchy = controller.FetchEditorHierarchy()
                .Where(asset => asset is not null)
                .Select(asset => asset!)
                .Distinct()
                .OrderBy(asset => asset.PathID)
                .ToArray();
            var identities = BuildControllerSourceIdentities(controller, hierarchy);
            rows.Append("SOURCE_CONTROLLER|").Append(controller.PathID).Append('|').Append(name)
                .Append('|').Append(hierarchy.Length).Append('\n');
            foreach (IUnityObjectBase asset in hierarchy)
            {
                rows.Append("SOURCE_CONTROLLER_OBJECT|").Append(controller.PathID).Append('|')
                    .Append(asset.PathID).Append('|').Append(asset.ClassID).Append('|')
                    .Append(identities[asset]).Append('\n');
            }
            var section = new StringBuilder();
            AppendControllerSourceYaml(section, hierarchy, version, identities);
            string sectionText = section.ToString();
            int begin = sectionText.IndexOf("SOURCE_YAML_BEGIN\n", StringComparison.Ordinal);
            int end = sectionText.IndexOf("SOURCE_YAML_END", begin, StringComparison.Ordinal);
            if (begin < 0 || end < begin)
            {
                throw new InvalidOperationException("source YAML section markers were not emitted");
            }
            string yaml = sectionText[(begin + "SOURCE_YAML_BEGIN\n".Length)..end];
            rows.Append("SOURCE_CONTROLLER_YAML_BEGIN|").Append(controller.PathID).Append('\n')
                .Append(yaml);
            if (!yaml.EndsWith('\n')) { rows.Append('\n'); }
            rows.Append("SOURCE_CONTROLLER_YAML_END|").Append(controller.PathID).Append('\n');
        }
        var originalBehaviours = inputCollection
            .OfType<AssetRipper.SourceGenerated.Classes.ClassID_114.IMonoBehaviour>()
            .OrderBy(behaviour => behaviour.PathID)
            .ToArray();
        if (originalBehaviours.Length != clonedBehaviours.Length)
        {
            throw new InvalidOperationException("source clone set does not match the shared release behaviours");
        }
        for (int i = 0; i < clonedBehaviours.Length; i++)
        {
            rows.Append("SOURCE_CLONE|").Append(controllers[1].PathID).Append('|')
                .Append(clonedBehaviours[i].PathID).Append('|').Append(originalBehaviours[i].PathID).Append('|')
                .Append(clonedBehaviours[i].Name_R.String).Append('\n');
        }
        string fullPath = Path.GetFullPath(output);
        Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
        File.WriteAllText(fullPath, rows.ToString(), new UTF8Encoding(false));
        Console.WriteLine($"wrote dual release controller source clone replay path={fullPath}");
        return 0;
    }

    private static Dictionary<IUnityObjectBase, string> BuildControllerSourceIdentities(
        AssetRipper.SourceGenerated.Classes.ClassID_91.IAnimatorController controller,
        IReadOnlyList<IUnityObjectBase> hierarchy)
    {
        var identities = new Dictionary<IUnityObjectBase, string> { [controller] = "controller" };
        void AddTransition(string identity, IUnityObjectBase? transition)
        {
            if (transition is not null) { identities.Add(transition, identity); }
        }
        void IndexMachine(
            AssetRipper.SourceGenerated.Classes.ClassID_1107.IAnimatorStateMachine machine,
            string parent)
        {
            string path = parent.Length == 0 ? machine.Name.String : parent + "." + machine.Name.String;
            identities.Add(machine, path);
            foreach (var child in machine.ChildStates)
            {
                var state = child.State.TryGetAsset(controller.Collection)
                    ?? throw new InvalidOperationException("controller child state is missing");
                string statePath = path + "." + state.Name.String;
                identities.Add(state, statePath);
                for (int i = 0; i < state.TransitionsP.Count; i++)
                {
                    AddTransition("STATE:" + statePath + ":" + i, state.TransitionsP[i]);
                }
            }
            for (int i = 0; i < machine.EntryTransitionsP.Count; i++)
            {
                AddTransition("ENTRY:" + path + ":" + i, machine.EntryTransitionsP[i]);
            }
            for (int i = 0; i < machine.AnyStateTransitionsP.Count; i++)
            {
                AddTransition("ANY:" + path + ":" + i, machine.AnyStateTransitionsP[i]);
            }
            foreach (var pair in machine.StateMachineTransitions)
            {
                var childMachine = pair.Key.TryGetAsset(controller.Collection)
                    ?? throw new InvalidOperationException("exit transition child machine is missing");
                for (int i = 0; i < pair.Value.Count; i++)
                {
                    var transition = pair.Value[i].TryGetAsset(controller.Collection);
                    AddTransition("EXIT:" + path + ":" + childMachine.Name.String + ":" + i, transition);
                }
            }
            foreach (var child in machine.ChildStateMachines)
            {
                var nested = child.StateMachine.TryGetAsset(controller.Collection)
                    ?? throw new InvalidOperationException("controller child machine is missing");
                IndexMachine(nested, path);
            }
            foreach (var behaviour in machine.StateMachineBehavioursP)
            {
                var asset = behaviour.TryGetAsset(controller.Collection);
                if (asset is not null)
                {
                    identities.TryAdd(asset, "BEHAVIOUR:" + asset.Name_R.String);
                }
            }
        }
        foreach (var layer in controller.AnimatorLayers)
        {
            var root = layer.StateMachine.TryGetAsset(controller.Collection)
                ?? throw new InvalidOperationException("controller layer root machine is missing");
            IndexMachine(root, string.Empty);
        }
        foreach (IUnityObjectBase asset in hierarchy)
        {
            if (!identities.ContainsKey(asset))
            {
                throw new InvalidOperationException($"source hierarchy object has no stable identity: class={asset.ClassID} pathID={asset.PathID}");
            }
        }
        return identities;
    }

    private static void AppendControllerSourceYaml(
        StringBuilder rows, IEnumerable<IUnityObjectBase> assets,
        AssetRipper.Primitives.UnityVersion version,
        IReadOnlyDictionary<IUnityObjectBase, string> identities)
    {
        // The export assembly is already copied by this probe's ReferenceRoot infrastructure;
        // load it only for this mode, keeping existing conversion modes unchanged.
        string directory = Path.Combine(AppContext.BaseDirectory, "ref");
        Assembly? ResolveExportAssembly(System.Runtime.Loader.AssemblyLoadContext context, AssemblyName name)
        {
            string path = Path.Combine(directory, name.Name + ".dll");
            return File.Exists(path) ? context.LoadFromAssemblyPath(path) : null;
        }
        System.Runtime.Loader.AssemblyLoadContext.Default.Resolving += ResolveExportAssembly;
        try
        {
            var exporter = Assembly.LoadFrom(Path.Combine(directory, "AssetRipper.Export.UnityProjects.dll"));
            var yaml = Assembly.LoadFrom(Path.Combine(directory, "AssetRipper.Yaml.dll"));
            Type walkerType = exporter.GetType("AssetRipper.Export.UnityProjects.YamlWalker")!;
            object walker = Activator.CreateInstance(walkerType)!;
            walkerType.GetMethod("WithUnityVersion")!.Invoke(walker, new object[] { version });
            Type writerType = yaml.GetType("AssetRipper.Yaml.YamlWriter")!;
            object yamlWriter = Activator.CreateInstance(writerType)!;
            foreach (IUnityObjectBase asset in assets.OrderBy(asset => asset.PathID))
            {
                string identity = identities[asset];
                rows.Append("SOURCE_OBJECT|").Append(asset.PathID).Append('|').Append(asset.ClassID).Append('|').Append(identity).Append('\n');
                object document = walkerType.GetMethod("ExportYamlDocument")!
                    .Invoke(walker, new object[] { asset, asset.PathID })!;
                writerType.GetMethod("AddDocument")!.Invoke(yamlWriter, new[] { document });
            }
            using var textWriter = new StringWriter(System.Globalization.CultureInfo.InvariantCulture);
            writerType.GetMethod("Write")!.Invoke(yamlWriter, new object[] { textWriter });
            rows.Append("SOURCE_YAML_BEGIN\n").Append(textWriter.ToString().Replace("\r\n", "\n")).Append("SOURCE_YAML_END\n");
        }
        finally
        {
            System.Runtime.Loader.AssemblyLoadContext.Default.Resolving -= ResolveExportAssembly;
        }
    }

    private static int DumpProps(string name)
    {
        Type type = Resolve(name);
        Console.WriteLine($"# {type.FullName} (base {type.BaseType?.FullName})");
        foreach (PropertyInfo prop in type.GetProperties(BindingFlags.Public | BindingFlags.Instance).OrderBy(p => p.Name))
        {
            Console.WriteLine($"P|{prop.Name}|{prop.PropertyType.FullName ?? prop.PropertyType.Name}|{(prop.CanWrite ? "rw" : "r")}");
        }
        foreach (MethodInfo method in type.GetMethods(BindingFlags.Public | BindingFlags.Instance).Where(m => m.Name.StartsWith("Has_")).OrderBy(m => m.Name))
        {
            Console.WriteLine($"H|{method.Name}");
        }
        return 0;
    }

    private static int DumpFieldHashes(string output)
    {
        var pool = new List<string>();
        var poolIndex = new Dictionary<string, int>();
        var classes = new List<(int ClassId, List<(uint Hash, int Index)> Rows)>();
        int entries = 0;
        foreach (Type type in Sg.GetTypes())
        {
            string? ns = type.Namespace;
            if (ns is null || !ns.StartsWith("AssetRipper.SourceGenerated.Classes.ClassID_"))
            {
                continue;
            }
            string suffix = ns["AssetRipper.SourceGenerated.Classes.ClassID_".Length..];
            if (!int.TryParse(suffix, out int classId))
            {
                continue;
            }
            FieldInfo? field = type.GetField("s_FieldPathDictionary", BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.DeclaredOnly);
            if (field?.GetValue(null) is not System.Collections.IDictionary dict)
            {
                continue;
            }
            var rows = new List<(uint Hash, int Index)>();
            foreach (System.Collections.DictionaryEntry entry in dict)
            {
                string path = (string)entry.Value!;
                if (!poolIndex.TryGetValue(path, out int index))
                {
                    index = pool.Count;
                    poolIndex.Add(path, index);
                    pool.Add(path);
                }
                rows.Add(((uint)entry.Key!, index));
            }
            rows.Sort((a, b) => a.Hash.CompareTo(b.Hash));
            entries += rows.Count;
            classes.Add((classId, rows));
        }
        classes.Sort((a, b) => a.ClassId.CompareTo(b.ClassId));
        var builder = new StringBuilder();
        builder.Append("# FieldHashes table dumped from the shipped AssetRipper.SourceGenerated.dll.\n");
        builder.Append("# Pool lines: P|<index>|<path>\n");
        builder.Append("# Class lines: C|<classId>|<hash>:<poolIndex>,... (hash ascending)\n");
        for (int i = 0; i < pool.Count; i++)
        {
            builder.Append("P|").Append(i).Append('|').Append(pool[i]).Append('\n');
        }
        foreach ((int classId, List<(uint Hash, int Index)> rows) in classes)
        {
            builder.Append("C|").Append(classId).Append('|');
            for (int i = 0; i < rows.Count; i++)
            {
                if (i > 0)
                {
                    builder.Append(',');
                }
                builder.Append(rows[i].Hash).Append(':').Append(rows[i].Index);
            }
            builder.Append('\n');
        }
        File.WriteAllText(output, builder.ToString());
        Console.WriteLine($"wrote {output} classes={classes.Count} entries={entries} pool={pool.Count}");
        return 0;
    }

    // ---------------------------------------------------------------------------------
    // Reflection helpers
    // ---------------------------------------------------------------------------------

    private static object? Get(object target, string name) =>
        target.GetType().GetProperty(name)?.GetValue(target);

    private static bool Gate(object target, string name)
    {
        MethodInfo? method = target.GetType().GetMethod(name, BindingFlags.Public | BindingFlags.Instance);
        return method is not null && (bool)method.Invoke(target, null)!;
    }

    private static object? HasValue(object target, string name)
    {
        MethodInfo? method = target.GetType().GetMethod("Has_" + name, BindingFlags.Public | BindingFlags.Instance);
        if (method is not null && !(bool)method.Invoke(target, null)!)
        {
            return null;
        }
        return Get(target, name);
    }

    private static void Set(object target, string name, object? value)
    {
        PropertyInfo? prop = target.GetType().GetProperty(name);
        if (prop is not null && prop.CanWrite)
        {
            prop.SetValue(target, ConvertValue(prop.PropertyType, value));
            return;
        }
        for (Type? bt = target.GetType(); bt is not null; bt = bt.BaseType)
        {
            FieldInfo? field = bt.GetField(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly);
            if (field is not null)
            {
                field.SetValue(target, ConvertValue(field.FieldType, value));
                return;
            }
        }
        if (prop is not null)
        {
            prop.GetSetMethod(true)?.Invoke(target, new[] { ConvertValue(prop.PropertyType, value) });
            return;
        }
        throw new InvalidOperationException($"no settable {name} on {target.GetType().FullName}");
    }

    private static object? ConvertValue(Type type, object? value)
    {
        if (value is null)
        {
            return null;
        }
        if (type.IsInstanceOfType(value))
        {
            return value;
        }
        if (type.IsEnum)
        {
            return Enum.ToObject(type, value);
        }
        if (type == typeof(AssetRipper.Primitives.Utf8String))
        {
            return new AssetRipper.Primitives.Utf8String((string)value);
        }
        return Convert.ChangeType(value, type);
    }

    private static void SetUtf8(object target, string name, string value) =>
        Set(target, name, value);

    private static object AddNew(object list) =>
        list.GetType().GetMethod("AddNew")!.Invoke(list, null)!;

    private static void AddItem(object list, object value) =>
        list.GetType().GetMethod("Add")!.Invoke(list, new[] { value });

    private static void Clear(object list) =>
        list.GetType().GetMethod("Clear")!.Invoke(list, null);

    private static int Count(object list) =>
        (int)list.GetType().GetProperty("Count")!.GetValue(list)!;

    private static object At(object list, int index)
    {
        PropertyInfo? indexer = list.GetType().GetProperty("Item");
        return indexer!.GetValue(list, new object[] { index })!;
    }

    private static void SetAsset(object pptr, object? asset)
    {
        MethodInfo? method = pptr.GetType().GetMethod("SetAsset", new[] { typeof(AssetRipper.Assets.Collections.AssetCollection), typeof(AssetRipper.Assets.IUnityObjectBase) });
        if (method is not null)
        {
            method.Invoke(pptr, new[] { asset is null ? null : GetCollection(asset), asset });
            return;
        }
        long pathId = asset is null ? 0 : Convert.ToInt64(asset.GetType().GetProperty("PathID")!.GetValue(asset)!);
        TrySetInt(pptr, "FileID_", 0);
        TrySetInt(pptr, "m_FileID_", 0);
        if (TrySetPathId(pptr, pathId))
        {
            return;
        }
        throw new InvalidOperationException($"no PathID slot on {pptr.GetType().FullName}");
    }

    private static void TrySetInt(object target, string name, int value)
    {
        try
        {
            Set(target, name, value);
        }
        catch (InvalidOperationException)
        {
        }
    }

    private static bool TrySetPathId(object pptr, long pathId)
    {
        foreach (string slot in new[] { "PathID", "PathID__Int32", "PathID__Int64", "m_PathID_", "m_PathID" })
        {
            try
            {
                Set(pptr, slot, slot == "PathID__Int32" ? (int)pathId : pathId);
                foreach (string read in new[] { "PathID", "PathID__Int32", "PathID__Int64" })
                {
                    object? value = Get(pptr, read);
                    if (value is not null && Convert.ToInt64(value) == pathId)
                    {
                        return true;
                    }
                }
            }
            catch (Exception)
            {
            }
        }
        return false;
    }

    private static object GetCollection(object asset) =>
        asset.GetType().GetProperty("Collection")!.GetValue(asset)!;

    private static object NewAsset(Type factoryType, object collection, long pathId, int classId, object version)
    {
        object asset = factoryType.GetMethod("Create", new[] { typeof(AssetRipper.Assets.Metadata.AssetInfo), typeof(AssetRipper.Primitives.UnityVersion) })!
            .Invoke(null, new object[] { new AssetRipper.Assets.Metadata.AssetInfo((AssetRipper.Assets.Collections.AssetCollection)collection, pathId, classId), version })!;
        collection.GetType().GetMethod("AddAsset", BindingFlags.NonPublic | BindingFlags.Instance)!
            .Invoke(collection, new object[] { asset });
        return asset;
    }

    private static uint Bits(float value) => BitConverter.SingleToUInt32Bits(value);

    private static string Hex(float value) => Bits(value).ToString("X8");

    private static string Hex(uint value) => value.ToString("X8");

    private static void AppendVec(StringBuilder builder, object vec, string[] names)
    {
        foreach (string name in names)
        {
            builder.Append('|').Append(Hex(Convert.ToSingle(Get(vec, name))));
        }
    }

    private static uint Crc32(string text)
    {
        Type checksum = Type.GetType("AssetRipper.Checksum.Crc32Algorithm, AssetRipper.Checksum")!;
        return (uint)checksum.GetMethod("HashUTF8", BindingFlags.Public | BindingFlags.Static, null, new[] { typeof(string) }, null)!.Invoke(null, new object[] { text })!;
    }

    private static byte[] StreamedFrame(float time, (int Index, float Cx, float Cy, float Cz, float Limit)[] keys)
    {
        using var stream = new MemoryStream();
        using var writer = new BinaryWriter(stream);
        writer.Write(time);
        writer.Write(keys.Length);
        foreach ((int index, float cx, float cy, float cz, float limit) in keys)
        {
            writer.Write(index);
            writer.Write(cx);
            writer.Write(cy);
            writer.Write(cz);
            writer.Write(limit);
        }
        return stream.ToArray();
    }

    // ---------------------------------------------------------------------------------
    // Conversion differential
    // ---------------------------------------------------------------------------------

    private static int RunConvert(
        string output,
        string? serializedWorkspace = null,
        bool includeExistingCurves = false,
        string? externalScriptSerializedFile = null,
        string? externalScriptAssemblyFile = null)
    {
        var bundle = new AssetRipper.Assets.Bundles.GameBundle();
        var collection = new AssetRipper.Assets.Collections.ProcessedAssetCollection(bundle);
        object version = AssetRipper.Primitives.UnityVersion.Parse("2022.3.0f1");
        collection.SetLayout(AssetRipper.Primitives.UnityVersion.Parse("2022.3.0f1"));
        collection.FilePath = "animation_workspace.assets";

        // ---- materialized scene graph -------------------------------------------------
        object rootGo = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_1.GameObject), collection, 1010, 1, version);
        object armGo = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_1.GameObject), collection, 1011, 1, version);
        object legGo = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_1.GameObject), collection, 1012, 1, version);
        object rootTr = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_4.Transform), collection, 1020, 4, version);
        object armTr = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_4.Transform), collection, 1021, 4, version);
        object legTr = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_4.Transform), collection, 1022, 4, version);
        object skin = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_137.SkinnedMeshRenderer), collection, 1030, 137, version);
        object mesh = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_43.Mesh), collection, 1040, 43, version);
        object renderer = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_23.MeshRenderer), collection, 1050, 23, version);
        object material = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_21.Material), collection, 1060, 21, version);
        object controller = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_91.AnimatorController), collection, 1080, 91, version);
        object animator = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_95.Animator), collection, 1090, 95, version);

        SetUtf8(rootGo, "Name_R", "Root");
        SetUtf8(armGo, "Name_R", "Arm");
        SetUtf8(legGo, "Name_R", "Leg");

        Set(rootTr, "GameObject_C4P", rootGo);
        Set(armTr, "GameObject_C4P", armGo);
        Set(legTr, "GameObject_C4P", legGo);
        Set(armTr, "Father_C4P", rootTr);
        Set(legTr, "Father_C4P", rootTr);
        AddComponent(rootGo, rootTr, 4);
        AddComponent(armGo, armTr, 4);
        AddComponent(legGo, legTr, 4);
        object rootChildren = Get(rootTr, "Children_C4")!;
        SetAsset(AddNew(rootChildren), armTr);
        SetAsset(AddNew(rootChildren), legTr);

        // Arm carries the skinned mesh (whose blend-shape lookup misses), Leg the
        // mesh renderer (whose material-property lookup misses). Both walks therefore
        // reach the component and fall through to the ReverseAscii fallback.
        object armComponents = Get(armGo, "Components")!;
        object armPair = AddNew(armComponents);
        Set(armPair, "ClassID", 137);
        SetAsset(Get(armPair, "Component")!, skin);
        Set(skin, "GameObjectP", armGo);
        Set(skin, "MeshP", mesh);

        object legComponents = Get(legGo, "Components")!;
        object legPair = AddNew(legComponents);
        Set(legPair, "ClassID", 23);
        SetAsset(Get(legPair, "Component")!, renderer);
        Set(renderer, "GameObjectP", legGo);
        SetAsset(AddNew(Get(renderer, "Materials")!), material);

        // ---- the clip -----------------------------------------------------------------
        object clip = NewAsset(typeof(AssetRipper.SourceGenerated.Classes.ClassID_74.AnimationClip), collection, 1000, 74, version);
        Set(clip, "SampleRate_C74", 30.0f);

        string[] paths =
        {
            "Root", "Root", "Root", "Arm", "Root", "Leg", "Root", "Root",
            "Arm", "Leg", "Leg", "Leg", "Root", "Root", "Root", "Root",
            "Root", "Root", "Root", "Root", "Leg", "Leg", "Root",
        };
        (int ClassId, uint Attribute, byte CustomType, byte IsPPtr)[] bindingSpecs =
        {
            (4, 1, 4, 0),                          // 0 translation
            (4, 2, 4, 0),                          // 1 rotation
            (4, 3, 4, 0),                          // 2 scaling
            (4, 4, 2, 0),                          // 3 euler (rotation order 2)
            (1, Crc32("m_IsActive"), 0, 0),        // 4 GameObject default hit
            (23, Crc32("m_Enabled"), 0, 0),        // 5 engine default hit
            (23, 0xDEADBEEFu, 0, 0),               // 6 engine default miss
            (114, Crc32("myField"), 0, 0),         // 7 script default, checksum miss
            (137, 0xC34B2C0Bu, 20, 0),             // 8 blend shape (scene walk misses)
            (23, 0x8686C589u, 22, 0),              // 9 renderer material (scene walk misses)
            (23, 3, 21, 0),                        // 10 renderer array
            (212, 0, 23, 0),                       // 11 sprite renderer
            (114, 0x12345678u, 40, 0),             // 12 user defined
            (181, 4, 31, 0),                       // 13 position constraint
            (95, 42, 8, 0),                        // 14 animator muscle
            (198, 0x924284C0u, 27, 0),             // 15 particle system
            (20, 0x9B5A57ECu, 0, 1),               // 16 PPtr engine curve
            (224, 1, 4, 0),                        // 17 rect-transform translation
            (4, 1, 4, 0),                          // 18 dense translation
            (23, Crc32("m_ReceiveShadows"), 0, 0), // 19 spacer
            (23, Crc32("m_LightmapIndex"), 0, 0),  // 20 spacer
            (23, Crc32("m_CastShadows"), 0, 0),    // 21 constant engine default
            (114, 0x9299AC34u, 24, 0),             // 22 constant MonoBehaviour hit
        };

        object bindingConstant = Get(clip, "ClipBindingConstant_C74")!;
        object genericBindings = Get(bindingConstant, "GenericBindings")!;
        Clear(genericBindings);
        for (int i = 0; i < bindingSpecs.Length; i++)
        {
            object binding = AddNew(genericBindings);
            Set(binding, "Path", Crc32(paths[i]));
            Set(binding, "Attribute", bindingSpecs[i].Attribute);
            if (Gate(binding, "Has_ClassID_UInt16"))
            {
                Set(binding, "ClassID_UInt16", (ushort)bindingSpecs[i].ClassId);
            }
            else
            {
                Set(binding, "ClassID_Int32", bindingSpecs[i].ClassId);
            }
            Set(binding, "CustomType", bindingSpecs[i].CustomType);
            Set(binding, "IsPPtrCurve", bindingSpecs[i].IsPPtr);
            if (externalScriptSerializedFile is not null && i == 7)
            {
                SetExternalPPtr(Get(binding, "Script")!, 1, 7);
                Set(binding, "Attribute", Crc32("health"));
            }
        }
        object mapping = Get(bindingConstant, "PptrCurveMapping")!;
        Clear(mapping);
        SetAsset(AddNew(mapping), rootGo);
        SetAsset(AddNew(mapping), null);

        // ---- streamed data ------------------------------------------------------------
        // Binding-cache positions: 0-2 translation, 3-6 rotation, 7-9 scaling, 10-12 euler,
        // 13 GameObject, 14 engine hit, 15 engine miss, 16 script, 17 blend shape,
        // 18 material, 19 renderer, 20 sprite, 21 user defined, 22 constraint, 23 muscle,
        // 24 particle system, 25 PPtr, 26 rect-transform translation, 27-29 dense
        // translation, 30/31 spacers, 32 constant engine, 33 constant MonoBehaviour.
        var frame0 = new (int, float, float, float, float)[] { (25, 0f, 0f, 0f, 0f) };
        var frame1 = new List<(int, float, float, float, float)>
        {
            (0, 0.1f, 0.2f, 0.3f, 1.0f),
            (1, 0f, 0f, 0f, 2.0f),
            (2, 0f, 0f, 0f, 3.0f),
            (3, 0.1f, 0.2f, 0.3f, 0.5f),
        };
        for (int i = 4; i <= 24; i++)
        {
            frame1.Add((i, 0f, 0f, 0f, 10.0f + i));
        }
        for (int i = 26; i <= 28; i++)
        {
            frame1.Add((i, 0f, 0f, 0f, 40.0f + i));
        }
        var frame2 = new List<(int, float, float, float, float)>
        {
            // Consecutive duplicate: the collapse keeps the last key.
            (13, 0f, 0f, 0f, 99.0f),
            (13, 0f, 0f, 0f, 8.0f),
            (0, 0f, 0f, 0f, 1.5f),
            (1, 0f, 0f, 0f, 2.0f),
            (2, 0f, 0f, 0f, 4.0f),
        };
        for (int i = 3; i <= 24; i++)
        {
            frame2.Add((i, 0f, 0f, 0f, 20.0f + i));
        }
        for (int i = 26; i <= 28; i++)
        {
            frame2.Add((i, 0f, 0f, 0f, 50.0f + i));
        }
        var streamed = new List<byte>();
        streamed.AddRange(StreamedFrame(float.MinValue, frame0));
        streamed.AddRange(StreamedFrame(1.0f / 30.0f, frame1.ToArray()));
        streamed.AddRange(StreamedFrame(2.0f / 30.0f, frame2.ToArray()));
        streamed.AddRange(StreamedFrame(float.PositiveInfinity, Array.Empty<(int, float, float, float, float)>()));

        object muscle = Get(clip, "MuscleClip_C74")!;
        object clipData = Get(muscle, "Clip")!;
        object clipValue = Get(clipData, "Data")!;
        object streamedClip = Get(clipValue, "StreamedClip")!;
        object streamedData = Get(streamedClip, "Data")!;
        Clear(streamedData);
        byte[] streamedBytes = streamed.ToArray();
        for (int i = 0; i < streamedBytes.Length; i += 4)
        {
            AddItem(streamedData, BitConverter.ToUInt32(streamedBytes, i));
        }
        Set(streamedClip, "CurveCount_UInt32", 29u);

        object dense = Get(clipValue, "DenseClip")!;
        Set(dense, "FrameCount", 2);
        Set(dense, "CurveCount", 3u);
        Set(dense, "SampleRate", 30.0f);
        Set(dense, "BeginTime", 0.5f);
        object denseSamples = Get(dense, "SampleArray")!;
        Clear(denseSamples);
        for (int i = 0; i < 6; i++)
        {
            AddItem(denseSamples, 100.0f + i);
        }

        object constant = Get(clipValue, "ConstantClip")!;
        object constantData = Get(constant, "Data")!;
        Clear(constantData);
        AddItem(constantData, 777.0f);
        AddItem(constantData, 888.0f);

        Set(muscle, "StopTime", 2.0f);
        Set(muscle, "StartTime", 0.0f);
        Set(muscle, "LoopTime", true);
        Set(muscle, "Level", 0.25f);

        // The clip must be reachable from an animator so FindRoots sees the scene.
        SetAsset(AddNew(Get(controller, "AnimationClips")!), clip);
        AddComponent(rootGo, animator, 95);
        Set(animator, "GameObjectP", rootGo);
        Set(animator, "Controller_PPtr_RuntimeAnimatorController_5P", controller);
        if (Get(animator, "HasTransformHierarchy") is not null)
        {
            Set(animator, "HasTransformHierarchy", true);
        }

        // Events must survive conversion untouched.
        object events = Get(clip, "Events_C74")!;
        object animationEvent = AddNew(events);
        SetUtf8(animationEvent, "FunctionName", "OnThing");
        SetUtf8(animationEvent, "Data", "payload");
        Set(animationEvent, "FloatParameter", 1.5f);
        Set(animationEvent, "IntParameter", 7);
        Set(animationEvent, "Time", 0.75f);

        if (includeExistingCurves)
        {
            SeedExistingCurves(clip, rootGo);
        }

        if (serializedWorkspace is not null)
        {
            WriteSerializedWorkspace(
                serializedWorkspace,
                collection,
                version,
                [rootGo, armGo, legGo, rootTr, armTr, legTr, skin, mesh, renderer, material, controller, animator, clip],
                externalScriptSerializedFile is null ? null : "scripts.assets");
        }

        if (externalScriptSerializedFile is not null)
        {
            if (externalScriptAssemblyFile is null)
            {
                throw new ArgumentException("external-script workspace requires its managed assembly");
            }
            string inputDirectory = Path.GetDirectoryName(Path.GetFullPath(serializedWorkspace!))!;
            File.Copy(
                Path.GetFullPath(externalScriptSerializedFile),
                Path.Combine(inputDirectory, "scripts.assets"));
            File.Copy(
                Path.GetFullPath(externalScriptAssemblyFile),
                Path.Combine(inputDirectory, "ComplexFixtureScripts.dll"));
            Console.WriteLine($"wrote source-generated external-script workspace {Path.Combine(inputDirectory, "animation_workspace.assets")}");
            return 0;
        }

        var checksumType = Type.GetType("AssetRipper.Processing.AnimationClips.PathChecksumCache, AssetRipper.Processing")!;
        var gameDataType = Type.GetType("AssetRipper.Processing.GameData, AssetRipper.Processing")!;
        object gameData = Activator.CreateInstance(gameDataType, bundle, version, null!, null!)!;
        object checksumCache = Activator.CreateInstance(checksumType, gameData)!;
        typeof(AssetRipper.Processing.AnimationClips.AnimationClipConverter)
            .GetMethod("Process", BindingFlags.Public | BindingFlags.Static)!
            .Invoke(null, new[] { clip, checksumCache });

        DumpClip(clip, output);
        return 0;
    }

    private static void SeedExistingCurves(object clip, object referencedObject)
    {
        object position = AddNew(Get(clip, "PositionCurves_C74")!);
        SetUtf8(position, "Path", "preserved/vector");
        object positionCurve = Get(position, "Curve")!;
        Set(positionCurve, "PreInfinity", 3);
        Set(positionCurve, "PostInfinity", 4);
        if (Gate(positionCurve, "Has_RotationOrder"))
        {
            Set(positionCurve, "RotationOrder", 2);
        }
        object positionKeys = Get(positionCurve, "Curve")!;
        object positionKey = AddNew(positionKeys);
        Set(positionKey, "Time", 0.125f);
        SetVector(positionKey, "Value", new[] { 1.25f, -2.5f, 3.75f });
        SetVector(positionKey, "InSlope", new[] { 4.5f, 5.5f, 6.5f });
        SetVector(positionKey, "OutSlope", new[] { 7.5f, 8.5f, 9.5f });
        SetVector(positionKey, "InWeight", new[] { 0.1f, 0.2f, 0.3f });
        SetVector(positionKey, "OutWeight", new[] { 0.4f, 0.5f, 0.6f });
        Set(positionKey, "TangentMode", 0);
        if (Gate(positionKey, "Has_WeightedMode"))
        {
            Set(positionKey, "WeightedMode", 2);
        }

        object rotation = AddNew(Get(clip, "RotationCurves_C74")!);
        SetUtf8(rotation, "Path", "preserved/rotation");
        object rotationCurve = Get(rotation, "Curve")!;
        Set(rotationCurve, "PreInfinity", 1);
        Set(rotationCurve, "PostInfinity", 2);
        if (Gate(rotationCurve, "Has_RotationOrder"))
        {
            Set(rotationCurve, "RotationOrder", 5);
        }
        object rotationKey = AddNew(Get(rotationCurve, "Curve")!);
        Set(rotationKey, "Time", 0.375f);
        SetVector(rotationKey, "Value", new[] { 0.125f, -0.25f, 0.5f, 0.75f });
        SetVector(rotationKey, "InSlope", new[] { 1.25f, 1.5f, 1.75f, 2.0f });
        SetVector(rotationKey, "OutSlope", new[] { 2.25f, 2.5f, 2.75f, 3.0f });
        SetVector(rotationKey, "InWeight", new[] { 0.15f, 0.25f, 0.35f, 0.45f });
        SetVector(rotationKey, "OutWeight", new[] { 0.55f, 0.65f, 0.75f, 0.85f });
        Set(rotationKey, "TangentMode", 0);
        if (Gate(rotationKey, "Has_WeightedMode"))
        {
            Set(rotationKey, "WeightedMode", 1);
        }

        object floating = AddNew(Get(clip, "FloatCurves_C74")!);
        SetUtf8(floating, "Path", "preserved/float");
        SetUtf8(floating, "Attribute", "m_PreservedValue");
        Set(floating, "ClassID", 114);
        if (Gate(floating, "Has_Flags"))
        {
            Set(floating, "Flags", 17);
        }
        object script = Get(floating, "Script")!;
        SetAsset(script, null);
        object floatCurve = Get(floating, "Curve")!;
        Set(floatCurve, "PreInfinity", 2);
        Set(floatCurve, "PostInfinity", 3);
        if (Gate(floatCurve, "Has_RotationOrder"))
        {
            Set(floatCurve, "RotationOrder", 4);
        }
        object floatKey = AddNew(Get(floatCurve, "Curve")!);
        Set(floatKey, "Time", 0.625f);
        Set(floatKey, "Value", -12.75f);
        Set(floatKey, "InSlope", 3.25f);
        Set(floatKey, "OutSlope", -4.5f);
        if (Gate(floatKey, "Has_InWeight"))
        {
            Set(floatKey, "InWeight", 0.125f);
        }
        if (Gate(floatKey, "Has_OutWeight"))
        {
            Set(floatKey, "OutWeight", 0.875f);
        }
        Set(floatKey, "TangentMode", 0);
        if (Gate(floatKey, "Has_WeightedMode"))
        {
            Set(floatKey, "WeightedMode", 3);
        }

        object pointerCurve = AddNew(Get(clip, "PPtrCurves_C74")!);
        SetUtf8(pointerCurve, "Path", "preserved/pointer");
        SetUtf8(pointerCurve, "Attribute", "m_PreservedObject");
        Set(pointerCurve, "ClassID", 114);
        if (Gate(pointerCurve, "Has_Flags"))
        {
            Set(pointerCurve, "Flags", 23);
        }
        object pointerKey = AddNew(Get(pointerCurve, "Curve")!);
        Set(pointerKey, "Time", 0.875f);
        SetAsset(Get(pointerKey, "Value")!, referencedObject);
    }

    private static void SetVector(object target, string field, float[] values)
    {
        object vector = Get(target, field)!;
        string[] names = values.Length switch
        {
            3 => new[] { "X", "Y", "Z" },
            4 => new[] { "X", "Y", "Z", "W" },
            _ => throw new ArgumentOutOfRangeException(nameof(values)),
        };
        for (int index = 0; index < values.Length; index++)
        {
            Set(vector, names[index], values[index]);
        }
    }

    private static void WriteSerializedWorkspace(
        string output,
        object collection,
        object version,
        IReadOnlyList<object> assets,
        string? externalDependencyPath = null)
    {
        var builder = new SerializedFileBuilder
        {
            Generation = FormatVersion.Unknown_10,
            Version = (AssetRipper.Primitives.UnityVersion)version,
            Platform = (BuildTarget)19,
            EndianType = EndianType.LittleEndian,
            HasTypeTree = false,
        };
        if (externalDependencyPath is not null)
        {
            builder.Dependencies.Add(new FileIdentifier
            {
                PathName = externalDependencyPath,
                PathNameOrigin = externalDependencyPath,
                AssetPath = AssetRipper.Primitives.Utf8String.Empty,
            });
        }

        Dictionary<int, SerializedType> typesByClassId = new();
        foreach (object asset in assets.OrderBy(asset => Convert.ToInt64(Get(asset, "PathID"))))
        {
            int classId = Convert.ToInt32(Get(asset, "ClassID"));
            long pathId = Convert.ToInt64(Get(asset, "PathID"));
            if (!typesByClassId.TryGetValue(classId, out SerializedType? type))
            {
                type = new SerializedType { RawTypeID = classId };
                typesByClassId.Add(classId, type);
                builder.Types.Add(type);
            }

            using MemoryStream payload = new();
            using (AssetWriter writer = new(payload, (AssetCollection)collection))
            {
                ((IUnityObjectBase)asset).WriteRelease(writer);
            }

            builder.Objects.Add(new ObjectInfo(type)
            {
                FileID = pathId,
                TypeID = classId,
                ObjectData = payload.ToArray(),
            });
        }

        AssetRipper.IO.Files.SerializedFiles.SerializedFile file = builder.Build();
        file.FilePath = "animation_workspace.assets";
        string fullPath = Path.GetFullPath(output);
        Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
        File.WriteAllBytes(fullPath, file.ToByteArray());
        Console.WriteLine($"wrote source-generated workspace {fullPath} objects={assets.Count} bytes={new FileInfo(fullPath).Length}");
    }

    private static void SetExternalPPtr(object pointer, int fileId, long pathId)
    {
        TrySetInt(pointer, "FileID_", fileId);
        TrySetInt(pointer, "m_FileID_", fileId);
        if (!TrySetPathId(pointer, pathId))
        {
            throw new InvalidOperationException($"could not set external PPtr PathID {pathId} on {pointer.GetType().FullName}");
        }
        if (Convert.ToInt32(Get(pointer, "FileID_")) != fileId
            || Convert.ToInt64(Get(pointer, "PathID__Int64")) != pathId)
        {
            throw new InvalidOperationException($"external PPtr identity did not retain ({fileId}, {pathId})");
        }
    }

    private static void AddComponent(object gameObject, object component, int classId)
    {
        object pair = AddNew(Get(gameObject, "Components")!);
        Set(pair, "ClassID", classId);
        SetAsset(Get(pair, "Component")!, component);
    }

    private static void DumpClip(object clip, string output)
    {
        var builder = new StringBuilder();
        builder.Append("# AnimationClipConverter.Process output over a real generated clip.\n");
        builder.Append("# Probe tools/probes/anim-convert (convert mode), shipped AssetRipper.Processing.\n");
        builder.Append("# All floats are bit patterns; envelope rotation order -1 means the field is absent.\n");

        builder.Append("INFO|sample_rate|").Append(Hex(Convert.ToSingle(Get(clip, "SampleRate_C74")))).Append('\n');
        object info = Get(clip, "MuscleClipInfo_C74")!;
        builder.Append("MINFO");
        builder.Append('|').Append(Hex(Convert.ToSingle(Get(info, "StartTime"))));
        builder.Append('|').Append(Hex(Convert.ToSingle(Get(info, "StopTime"))));
        builder.Append('|').Append(Hex(Convert.ToSingle(Get(info, "OrientationOffsetY"))));
        builder.Append('|').Append(Hex(Convert.ToSingle(Get(info, "Level"))));
        builder.Append('|').Append(Hex(Convert.ToSingle(Get(info, "CycleOffset"))));
        builder.Append('|').Append(Hex(Convert.ToSingle(Get(info, "AdditiveReferencePoseTime"))));
        foreach (string flag in new[]
        {
            "LoopTime", "LoopBlend", "LoopBlendOrientation", "LoopBlendPositionY",
            "LoopBlendPositionXZ", "KeepOriginalOrientation", "KeepOriginalPositionY",
            "KeepOriginalPositionXZ", "HeightFromFeet", "HasAdditiveReferencePose", "Mirror",
        })
        {
            object? value = HasValue(info, flag);
            builder.Append('|').Append(value is bool b && b ? "1" : "0");
        }
        builder.Append('\n');

        object events = Get(clip, "Events_C74")!;
        for (int i = 0; i < Count(events); i++)
        {
            object e = At(events, i);
            builder.Append("EVENT|").Append(i)
                .Append('|').Append(Get(e, "FunctionName"))
                .Append('|').Append(Get(e, "Data"))
                .Append('|').Append(Hex(Convert.ToSingle(Get(e, "FloatParameter"))))
                .Append('|').Append(Get(e, "IntParameter"))
                .Append('|').Append(Hex(Convert.ToSingle(Get(e, "Time"))))
                .Append('\n');
        }

        DumpVector3Curves(builder, clip, "POS", "PositionCurves_C74");
        DumpQuaternionCurves(builder, clip, "ROT", "RotationCurves_C74");
        DumpVector3Curves(builder, clip, "SCL", "ScaleCurves_C74");
        DumpVector3Curves(builder, clip, "EUL", "EulerCurves_C74");
        DumpFloatCurves(builder, clip);
        DumpPPtrCurves(builder, clip);

        File.WriteAllText(output, builder.ToString());
        Console.WriteLine($"wrote {output}");
    }

    private static void DumpVector3Curves(StringBuilder builder, object clip, string tag, string prop)
    {
        object list = Get(clip, prop)!;
        for (int i = 0; i < Count(list); i++)
        {
            object curve = At(list, i);
            object envelope = Get(curve, "Curve")!;
            builder.Append(tag).Append('|').Append(i)
                .Append('|').Append(Get(curve, "Path"))
                .Append('|').Append(Convert.ToInt32(Get(envelope, "PreInfinity")))
                .Append('|').Append(Convert.ToInt32(Get(envelope, "PostInfinity")))
                .Append('|').Append(Gate(envelope, "Has_RotationOrder") ? Convert.ToInt32(Get(envelope, "RotationOrder")) : -1)
                .Append('\n');
            object keys = Get(envelope, "Curve")!;
            for (int k = 0; k < Count(keys); k++)
            {
                object key = At(keys, k);
                builder.Append(tag).Append("K|").Append(i)
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "Time"))));
                AppendVec(builder, Get(key, "Value")!, new[] { "X", "Y", "Z" });
                AppendVec(builder, Get(key, "InSlope")!, new[] { "X", "Y", "Z" });
                AppendVec(builder, Get(key, "OutSlope")!, new[] { "X", "Y", "Z" });
                AppendVec(builder, HasValue(key, "InWeight") ?? Get(key, "InSlope")!, new[] { "X", "Y", "Z" });
                AppendVec(builder, HasValue(key, "OutWeight") ?? Get(key, "OutSlope")!, new[] { "X", "Y", "Z" });
                builder.Append('|').Append(Convert.ToInt32(Get(key, "TangentMode")));
                object? weighted = HasValue(key, "WeightedMode");
                builder.Append('|').Append(weighted is null ? -1 : Convert.ToInt32(weighted));
                builder.Append('\n');
            }
        }
    }

    private static void DumpQuaternionCurves(StringBuilder builder, object clip, string tag, string prop)
    {
        object list = Get(clip, prop)!;
        for (int i = 0; i < Count(list); i++)
        {
            object curve = At(list, i);
            object envelope = Get(curve, "Curve")!;
            builder.Append(tag).Append('|').Append(i)
                .Append('|').Append(Get(curve, "Path"))
                .Append('|').Append(Convert.ToInt32(Get(envelope, "PreInfinity")))
                .Append('|').Append(Convert.ToInt32(Get(envelope, "PostInfinity")))
                .Append('|').Append(Gate(envelope, "Has_RotationOrder") ? Convert.ToInt32(Get(envelope, "RotationOrder")) : -1)
                .Append('\n');
            object keys = Get(envelope, "Curve")!;
            for (int k = 0; k < Count(keys); k++)
            {
                object key = At(keys, k);
                builder.Append(tag).Append("K|").Append(i)
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "Time"))));
                AppendVec(builder, Get(key, "Value")!, new[] { "X", "Y", "Z", "W" });
                AppendVec(builder, Get(key, "InSlope")!, new[] { "X", "Y", "Z", "W" });
                AppendVec(builder, Get(key, "OutSlope")!, new[] { "X", "Y", "Z", "W" });
                AppendVec(builder, HasValue(key, "InWeight") ?? Get(key, "InSlope")!, new[] { "X", "Y", "Z", "W" });
                AppendVec(builder, HasValue(key, "OutWeight") ?? Get(key, "OutSlope")!, new[] { "X", "Y", "Z", "W" });
                builder.Append('|').Append(Convert.ToInt32(Get(key, "TangentMode")));
                object? weighted = HasValue(key, "WeightedMode");
                builder.Append('|').Append(weighted is null ? -1 : Convert.ToInt32(weighted));
                builder.Append('\n');
            }
        }
    }

    private static void DumpFloatCurves(StringBuilder builder, object clip)
    {
        object list = Get(clip, "FloatCurves_C74")!;
        for (int i = 0; i < Count(list); i++)
        {
            object curve = At(list, i);
            object envelope = Get(curve, "Curve")!;
            object? script = Get(curve, "Script");
            builder.Append("FLOAT|").Append(i)
                .Append('|').Append(Get(curve, "Path"))
                .Append('|').Append(Get(curve, "Attribute"))
                .Append('|').Append(Convert.ToInt32(Get(curve, "ClassID")))
                .Append('|').Append(script is null ? -1 : Convert.ToInt64(Get(script, "PathID__Int64")))
                .Append('|').Append(Convert.ToInt32(Get(envelope, "PreInfinity")))
                .Append('|').Append(Convert.ToInt32(Get(envelope, "PostInfinity")))
                .Append('|').Append(Gate(envelope, "Has_RotationOrder") ? Convert.ToInt32(Get(envelope, "RotationOrder")) : -1)
                .Append('\n');
            object keys = Get(envelope, "Curve")!;
            for (int k = 0; k < Count(keys); k++)
            {
                object key = At(keys, k);
                builder.Append("FK|").Append(i)
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "Time"))))
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "Value"))))
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "InSlope"))))
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "OutSlope"))));
                object? inWeight = HasValue(key, "InWeight");
                object? outWeight = HasValue(key, "OutWeight");
                builder.Append('|').Append(inWeight is null ? "-1" : Hex(Convert.ToSingle(inWeight)));
                builder.Append('|').Append(outWeight is null ? "-1" : Hex(Convert.ToSingle(outWeight)));
                builder.Append('|').Append(Convert.ToInt32(Get(key, "TangentMode")));
                object? weighted = HasValue(key, "WeightedMode");
                builder.Append('|').Append(weighted is null ? -1 : Convert.ToInt32(weighted));
                builder.Append('\n');
            }
        }
    }

    private static void DumpPPtrCurves(StringBuilder builder, object clip)
    {
        object list = Get(clip, "PPtrCurves_C74")!;
        for (int i = 0; i < Count(list); i++)
        {
            object curve = At(list, i);
            builder.Append("PPTR|").Append(i)
                .Append('|').Append(Get(curve, "Path"))
                .Append('|').Append(Get(curve, "Attribute"))
                .Append('|').Append(Convert.ToInt32(Get(curve, "ClassID")))
                .Append('|').Append(Convert.ToInt32(Get(curve, "Flags")))
                .Append('\n');
            object keys = Get(curve, "Curve")!;
            for (int k = 0; k < Count(keys); k++)
            {
                object key = At(keys, k);
                object value = Get(key, "Value")!;
                long pathId = Convert.ToInt64(Get(value, "PathID__Int64"));
                builder.Append("PK|").Append(i)
                    .Append('|').Append(Hex(Convert.ToSingle(Get(key, "Time"))))
                    .Append('|').Append(Convert.ToInt32(Get(value, "FileID_")))
                    .Append('|').Append(pathId)
                    .Append('\n');
            }
        }
    }
}
