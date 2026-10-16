// Reference-runtime differential probe for the model-export builders of the frozen
// AssetRipper baseline (`GlbTerrainBuilder`, `GlbNavMeshExporter`). It creates real
// generated objects through the shipped `*.Create(AssetInfo, UnityVersion)` factories,
// invokes the shipped builders, converts the resulting SharpGLTF SceneBuilder with the
// shipped `ToGltf2()`, and dumps the semantic decode of every primitive accessor.
//
// Run (from the RustAssetRipper repository root):
//   dotnet build tools/probes/res-glb/ResGlbProbe.csproj -c Release
//   dotnet run --project tools/probes/res-glb/ResGlbProbe.csproj -c Release --no-build
using System.Reflection;
using AssetRipper.Assets;
using AssetRipper.Assets.Bundles;
using AssetRipper.Assets.Collections;
using AssetRipper.Assets.Metadata;
using AssetRipper.Primitives;
using SharpGLTF.Schema2;

public static class Program
{
    private const string Invariant = "R";

    private static void SetAny(object target, string name, object? value)
    {
        PropertyInfo? prop = target.GetType().GetProperty(name);
        if (prop is not null && prop.CanWrite)
        {
            prop.SetValue(target, value);
            return;
        }
        foreach (string candidate in new[] { "m_" + name, "m_" + name + "_" })
        {
            for (Type? bt = target.GetType(); bt is not null; bt = bt.BaseType)
            {
                FieldInfo? backing = bt.GetField(candidate, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly);
                if (backing is not null)
                {
                    backing.SetValue(target, Convert.ChangeType(value, backing.FieldType));
                    return;
                }
            }
        }
        throw new InvalidOperationException($"no settable {name} on {target.GetType().FullName}");
    }

    private static object GetProp(object target, string name)
    {
        PropertyInfo? prop = target.GetType().GetProperty(name);
        if (prop is null) { throw new InvalidOperationException($"no property {name} on {target.GetType().FullName}"); }
        return prop.GetValue(target)!;
    }

    private static long ReadLong(object target, string name)
    {
        return Convert.ToInt64(GetProp(target, name));
    }

    private static object ToUtf8(string s)
    {
        Type utf8 = typeof(Utf8String);
        MethodInfo? op = utf8.GetMethod("op_Implicit", new[] { typeof(string) });
        if (op is not null) { return op.Invoke(null, new object[] { s })!; }
        return Activator.CreateInstance(utf8, new object[] { s })!;
    }

    private static object CreateAsset(Type factory, ProcessedAssetCollection collection, long pathId, int classId, string version)
    {
        object unityVersion = UnityVersion.Parse(version);
        return factory.GetMethod("Create", new[] { typeof(AssetInfo), typeof(UnityVersion) })!
            .Invoke(null, new object[] { new AssetInfo(collection, pathId, classId), unityVersion })!;
    }

    private static void AddNew(object list, object element)
    {
        list.GetType().GetMethod("Add", new[] { element.GetType() })!.Invoke(list, new object[] { element });
    }

    private static object AddNewElement(object list)
    {
        return list.GetType().GetMethod("AddNew", Type.EmptyTypes)!.Invoke(list, null)!;
    }

    private static string F(float value) => value.ToString(Invariant, System.Globalization.CultureInfo.InvariantCulture);

    private static void DumpModel(string tag, ModelRoot model)
    {
        int meshIndex = 0;
        foreach (Mesh mesh in model.LogicalMeshes)
        {
            int primIndex = 0;
            foreach (MeshPrimitive prim in mesh.Primitives)
            {
                Console.WriteLine($"GMESH|{tag}|{meshIndex}|{mesh.Name}|{primIndex}");
                Accessor? position = prim.GetVertexAccessor("POSITION");
                Accessor? uv = prim.GetVertexAccessor("TEXCOORD_0");
                IList<uint>? indices = prim.GetIndices();
                if (position is not null)
                {
                    IList<System.Numerics.Vector3> values = position.AsVector3Array();
                    for (int i = 0; i < values.Count; i++)
                    {
                        Console.WriteLine($"GPOS|{tag}|{i}|{F(values[i].X)}|{F(values[i].Y)}|{F(values[i].Z)}");
                    }
                }
                if (uv is not null)
                {
                    IList<System.Numerics.Vector2> values = uv.AsVector2Array();
                    for (int i = 0; i < values.Count; i++)
                    {
                        Console.WriteLine($"GUV|{tag}|{i}|{F(values[i].X)}|{F(values[i].Y)}");
                    }
                }
                if (indices is not null)
                {
                    Console.WriteLine($"GIDX|{tag}|{string.Join(",", indices)}");
                }
                primIndex++;
            }
            meshIndex++;
        }
        foreach (Node node in model.LogicalNodes)
        {
            Console.WriteLine($"GNODE|{tag}|{node.Name}");
        }
        if (model.LogicalMeshes.Count == 0)
        {
            Console.WriteLine($"GEMPTY|{tag}|no-meshes|nodes={model.LogicalNodes.Count}");
        }
    }

    private static void TerrainCase(string tag, string version, int width, int height, int resolution,
        (float X, float Y, float Z) scale, short[] heights, string name)
    {
        try
        {
            GameBundle bundle = new();
            ProcessedAssetCollection collection = new(bundle);
            Type factory = typeof(AssetRipper.SourceGenerated.Classes.ClassID_156.TerrainData);
            object terrain = CreateAsset(factory, collection, 100, 156, version);
            SetAny(terrain, "Name_R", ToUtf8(name));
            object heightmap = GetProp(terrain, "Heightmap");
            SetAny(heightmap, "Width", width);
            SetAny(heightmap, "Height", height);
            SetAny(heightmap, "Resolution", resolution);
            object scaleValue = GetProp(heightmap, "Scale");
            SetAny(scaleValue, "X", scale.X);
            SetAny(scaleValue, "Y", scale.Y);
            SetAny(scaleValue, "Z", scale.Z);
            object list = GetProp(heightmap, "Heights");
            MethodInfo add = list.GetType().GetMethod("Add", new[] { typeof(short) })!;
            foreach (short value in heights) { add.Invoke(list, new object[] { value }); }

            Console.WriteLine($"GIN|{tag}|{version}|{width}|{height}|{resolution}|{F(scale.X)}|{F(scale.Y)}|{F(scale.Z)}|{name}|{heights.Length}");
            Console.WriteLine($"GHTS|{tag}|{string.Join(",", heights)}");
            Console.WriteLine($"GREAD|{tag}|{ReadLong(heightmap, "Width")}|{ReadLong(heightmap, "Height")}|{ReadLong(heightmap, "Resolution")}");

            Type builder = typeof(AssetRipper.Export.Modules.Models.GlbTerrainBuilder);
            object sceneBuilder = builder.GetMethod("Build")!.Invoke(null, new[] { terrain })!;
            ModelRoot model = (ModelRoot)sceneBuilder.GetType().GetMethod("ToGltf2", Type.EmptyTypes)!.Invoke(sceneBuilder, null)!;
            DumpModel(tag, model);
        }
        catch (Exception ex)
        {
            Console.WriteLine($"GERR|{tag}|{ex.GetType().Name}|{ex.Message.Split('\n')[0]}");
            if (ex.InnerException is not null) { Console.WriteLine($"GERR|{tag}|inner|{ex.InnerException.GetType().Name}|{ex.InnerException.Message.Split('\n')[0]}"); }
        }
    }

    private static void NavMeshCase(string tag, string version, string name, int[][] triangles, (float X, float Y, float Z)[][] vertices)
    {
        try
        {
            GameBundle bundle = new();
            ProcessedAssetCollection collection = new(bundle);
            Type factory = typeof(AssetRipper.SourceGenerated.Classes.ClassID_238.NavMeshData);
            object navMesh = CreateAsset(factory, collection, 200, 238, version);
            SetAny(navMesh, "Name_R", ToUtf8(name));
            object heightMeshes = GetProp(navMesh, "HeightMeshes");
            for (int i = 0; i < triangles.Length; i++)
            {
                object entry = AddNewElement(heightMeshes);
                object indices = GetProp(entry, "Indices");
                MethodInfo addIndex = indices.GetType().GetMethod("Add", new[] { typeof(int) })!;
                foreach (int index in triangles[i]) { addIndex.Invoke(indices, new object[] { index }); }
                object verts = GetProp(entry, "Vertices");
                foreach ((float X, float Y, float Z) v in vertices[i])
                {
                    object vector = AddNewElement(verts);
                    SetAny(vector, "X", v.X);
                    SetAny(vector, "Y", v.Y);
                    SetAny(vector, "Z", v.Z);
                }
            }
            Console.WriteLine($"NIN|{tag}|{version}|{name}|{triangles.Length}");
            for (int i = 0; i < triangles.Length; i++)
            {
                Console.WriteLine($"NVTS|{tag}|{i}|{string.Join(";", vertices[i].Select(v => $"{F(v.X)},{F(v.Y)},{F(v.Z)}"))}");
                Console.WriteLine($"NIDX|{tag}|{i}|{string.Join(",", triangles[i])}");
            }

            Type exporter = typeof(AssetRipper.Export.PrimaryContent.Models.GlbNavMeshExporter);
            object instance = Activator.CreateInstance(exporter)!;
            Type sceneType = typeof(SharpGLTF.Scenes.SceneBuilder);
            ConstructorInfo ctor = sceneType.GetConstructors(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance).Single();
            ParameterInfo[] ctorParameters = ctor.GetParameters();
            object?[] ctorArgs = ctorParameters.Select(p => p.HasDefaultValue ? p.DefaultValue : string.Empty).ToArray();
            object sceneBuilder = ctor.Invoke(ctorArgs);
            MethodInfo addAsset = exporter.GetMethod("AddAssetToSceneBuilder", BindingFlags.NonPublic | BindingFlags.Static)!;
            addAsset.Invoke(null, new object[] { sceneBuilder, navMesh });
            ModelRoot model = (ModelRoot)sceneBuilder.GetType().GetMethod("ToGltf2", Type.EmptyTypes)!.Invoke(sceneBuilder, null)!;
            DumpModel(tag, model);
        }
        catch (Exception ex)
        {
            Console.WriteLine($"GERR|{tag}|{ex.GetType().Name}|{ex.Message.Split('\n')[0]}");
            if (ex.InnerException is not null) { Console.WriteLine($"GERR|{tag}|inner|{ex.InnerException.GetType().Name}|{ex.InnerException.Message.Split('\n')[0]}"); }
        }
    }

    /// Build a real GameObject/Transform hierarchy and dump what `GlbLevelBuilder.Build` emits.
    ///
    /// Settles the questions the port's `glb_level.rs` has to answer: the node order, the parent
    /// links, which nodes carry a local transform, and whether a root node of a non-scene build
    /// is transformed. `isScene` is flipped per run because the source's rule depends on it.
    private static void LevelCase(string tag, string version, bool isScene)
    {
        try
        {
            GameBundle bundle = new();
            ProcessedAssetCollection collection = new(bundle);
            Type goFactory = typeof(AssetRipper.SourceGenerated.Classes.ClassID_1.GameObject);
            Type trFactory = typeof(AssetRipper.SourceGenerated.Classes.ClassID_4.Transform);

            // Root -> Child -> Grandchild, plus a second root, all with non-identity local values
            // so a dropped or misordered transform is visible.
            object rootGo = CreateAsset(goFactory, collection, 1000, 1, version);
            object rootTr = CreateAsset(trFactory, collection, 1001, 4, version);
            object childGo = CreateAsset(goFactory, collection, 1002, 1, version);
            object childTr = CreateAsset(trFactory, collection, 1003, 4, version);
            object grandGo = CreateAsset(goFactory, collection, 1004, 1, version);
            object grandTr = CreateAsset(trFactory, collection, 1005, 4, version);
            object secondGo = CreateAsset(goFactory, collection, 1006, 1, version);
            object secondTr = CreateAsset(trFactory, collection, 1007, 4, version);

            // `AssetCollection.AddAsset` is protected, so the probe registers each asset through
            // the private backing dictionary that `Assets` exposes. A PPtr resolves through the
            // collection, so without this every link reports "Can't find transform component".
            RegisterAssets(collection, new[]
            {
                rootGo, rootTr, childGo, childTr, grandGo, grandTr, secondGo, secondTr,
            });

            SetName(rootGo, "Root");
            SetName(childGo, "Child");
            SetName(grandGo, "Grandchild");
            SetName(secondGo, "Second");
            // Distinct rotations so the coordinate conversion is actually exercised; a rotation
            // about +Y is the one whose sign the quaternion conversion changes.
            SetLocal(rootTr, 1.0f, 2.0f, 3.0f, 2.0f, 2.0f, 2.0f,
                0.0f, 0.70710677f, 0.0f, 0.70710677f);
            SetLocal(childTr, 4.0f, 5.0f, 6.0f, 1.0f, 1.0f, 1.0f,
                0.5f, 0.0f, 0.0f, 0.8660254f);
            SetLocal(grandTr, 7.0f, 8.0f, 9.0f, 1.0f, 1.0f, 1.0f,
                0.0f, 0.0f, 0.70710677f, 0.70710677f);
            SetLocal(secondTr, -1.0f, -2.0f, -3.0f, 1.0f, 1.0f, 1.0f);

            // The generated API links a GameObject to its components through `AddComponent` and a
            // Transform back to its GameObject through `GameObject_C4P`; hand-building the PPtr
            // pairs does not type them, so `GetTransform` cannot find them.
            LinkPair(rootGo, rootTr);
            LinkPair(childGo, childTr);
            LinkPair(grandGo, grandTr);
            LinkPair(secondGo, secondTr);

            LinkParent(childTr, rootTr);
            LinkParent(grandTr, childTr);

            // Confirm the generated `GetTransform` finds the Transform before `Build` runs, so a
            // failure inside `Build` is not confused with an unlinked fixture.
            MethodInfo getTransform = typeof(AssetRipper.SourceGenerated.Extensions.GameObjectExtensions)
                .GetMethods(BindingFlags.Public | BindingFlags.Static)
                .First(m => m.Name == "GetTransform");
            Console.WriteLine(
                $"LCOL|go={GetProp(rootGo, "Collection")?.GetType().Name}|" +
                $"goCollectionIsTr={ReferenceEquals(GetProp(rootGo, "Collection"), GetProp(rootTr, "Collection"))}|" +
                $"goCount={GetProp(GetProp(rootGo, "Collection"), "Count")}");
            foreach ((object go, string label) in new[] { (rootGo, "root"), (childGo, "child") })
            {
                try
                {
                    object found = getTransform.Invoke(null, new[] { go })!;
                    Console.WriteLine($"LFIND|{label}|{GetProp(found, "PathID")}");
                }
                catch (Exception ex)
                {
                    Console.WriteLine($"LFIND|{label}|FAIL|{ex.InnerException?.Message ?? ex.Message}");
                }
            }
            Console.WriteLine($"LPATH|rootGo={GetProp(rootGo, "PathID")}|rootTr={GetProp(rootTr, "PathID")}|childTr={GetProp(childTr, "PathID")}");
            Console.WriteLine($"LCOMP|rootGoPairs={GetProp(GetProp(rootGo, "Components"), "Count")}");
            // Root and Second stay parentless, so they are two separate roots.

            Console.WriteLine($"LIN|{tag}|{version}|scene={isScene}");

            Type builder = typeof(AssetRipper.Export.Modules.Models.GlbLevelBuilder);
            IEnumerable<AssetRipper.Assets.IUnityObjectBase> assets = new AssetRipper.Assets.IUnityObjectBase[]
            {
                (AssetRipper.Assets.IUnityObjectBase)rootGo,
                (AssetRipper.Assets.IUnityObjectBase)secondGo,
            };
            object sceneBuilder = builder.GetMethod("Build")!.Invoke(null, new object[] { assets, isScene })!;
            ModelRoot model = (ModelRoot)sceneBuilder.GetType().GetMethod("ToGltf2", Type.EmptyTypes)!.Invoke(sceneBuilder, null)!;

            foreach (Node node in model.LogicalNodes)
            {
                string parent = node.VisualParent is null ? "<none>" : node.VisualParent.Name;
                string children = string.Join(";", node.VisualChildren.Select(c => c.Name));
                // `LocalTransform` is a non-nullable SharpGLTF struct; an unset transform is the
                // identity, so the dump reports the components as written.
                SharpGLTF.Transforms.AffineTransform local = node.LocalTransform;
                string localText =
                    $"{F(local.Translation.X)},{F(local.Translation.Y)},{F(local.Translation.Z)}" +
                    $"|{F(local.Scale.X)},{F(local.Scale.Y)},{F(local.Scale.Z)}" +
                    $"|{F(local.Rotation.X)},{F(local.Rotation.Y)},{F(local.Rotation.Z)},{F(local.Rotation.W)}";
                bool hasMesh = node.Mesh is not null;
                Console.WriteLine($"LNODE|{tag}|{node.Name}|parent={parent}|children={children}|mesh={hasMesh}|local={localText}");
            }
        }
        catch (Exception ex)
        {
            Console.WriteLine($"LERR|{tag}|{ex.GetType().Name}|{ex.Message.Split('\n')[0]}");
            if (ex.InnerException is not null) { Console.WriteLine($"LERR|{tag}|inner|{ex.InnerException.Message.Split('\n')[0]}"); }
        }
    }

    private static void SetName(object target, string name)
    {
        PropertyInfo? prop = target.GetType().GetProperty("Name_R");
        if (prop is not null && prop.CanWrite) { prop.SetValue(target, ToUtf8(name)); return; }
        SetAny(target, "Name_R", ToUtf8(name));
    }

    /// Set the local position, rotation and scale a Transform exposes.
    ///
    /// The rotation matters: SharpGLTF validates it when building an `AffineTransform`, and an
    /// all-zero quaternion is rejected with `(Parameter 'rotation')`, which is what a scene build
    /// hits because it transforms its roots as well.
    private static void SetLocal(
        object transform,
        float px, float py, float pz,
        float sx, float sy, float sz,
        float rx = 0.0f, float ry = 0.0f, float rz = 0.0f, float rw = 1.0f)
    {
        object position = GetProp(transform, "LocalPosition_C4");
        SetAny(position, "X", px);
        SetAny(position, "Y", py);
        SetAny(position, "Z", pz);
        object rotation = GetProp(transform, "LocalRotation_C4");
        SetAny(rotation, "X", rx);
        SetAny(rotation, "Y", ry);
        SetAny(rotation, "Z", rz);
        SetAny(rotation, "W", rw);
        object scale = GetProp(transform, "LocalScale_C4");
        SetAny(scale, "X", sx);
        SetAny(scale, "Y", sy);
        SetAny(scale, "Z", sz);
    }

    /// Register probe assets in a collection so PPtr resolution can find them.
    ///
    /// `AddAsset` is protected, so this writes the private dictionary behind the public `Assets`
    /// view by reflection. Probe-only; the reference's own loader uses the protected path.
    private static void RegisterAssets(object collection, object[] assets)
    {
        // The dictionary is declared on the base `AssetCollection`, not on the concrete type.
        FieldInfo? field = null;
        for (Type? type = collection.GetType(); type is not null && field is null; type = type.BaseType)
        {
            field = type
                .GetFields(BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public
                    | BindingFlags.DeclaredOnly)
                .FirstOrDefault(f => f.FieldType.IsGenericType
                    && f.FieldType.GetGenericTypeDefinition() == typeof(Dictionary<,>)
                    && f.FieldType.GetGenericArguments()[1].Name == "IUnityObjectBase");
        }
        if (field is null)
        {
            throw new InvalidOperationException(
                $"no asset dictionary on {collection.GetType().FullName}");
        }
        object store = field.GetValue(collection)!;
        MethodInfo add = store.GetType()
            .GetMethod("Add", new[] { typeof(long), field.FieldType.GetGenericArguments()[1] })!;
        foreach (object asset in assets)
        {
            long pathId = Convert.ToInt64(GetProp(asset, "PathID"));
            add.Invoke(store, new object[] { pathId, asset });
        }
    }

    /// Link a GameObject and a Transform through the generated API.
    ///
    /// `GameObject.AddComponent(ClassIDType, IComponent)` records the typed pair, and
    /// `Transform.GameObject_C4P` points back, which is what `GetTransform` walks. Hand-building
    /// the PPtr pairs instead leaves them untyped, and `GetTransform` then reports "Can't find
    /// transform component".
    private static void LinkPair(object gameObject, object transform)
    {
        // `InitializeDefault` gives the generated Transform its component/collection wiring; the
        // scene-processor probe does the same before linking.
        transform.GetType().GetMethod("InitializeDefault", Type.EmptyTypes)?.Invoke(transform, null);
        SetAny(transform, "GameObject_C4P", gameObject);
        // `AddComponent(this IGameObject, ClassIDType, IComponent)` is an extension method, so the
        // receiver is the first argument; the pair it adds calls `Component.SetAsset`, which is
        // what makes the pointer resolve as a typed component rather than a generic asset.
        MethodInfo add = typeof(AssetRipper.SourceGenerated.Extensions.GameObjectExtensions)
            .GetMethods(BindingFlags.Public | BindingFlags.Static)
            .First(m => m.Name == "AddComponent" && m.GetParameters().Length == 3);
        object classId = Enum.ToObject(
            add.GetParameters()[1].ParameterType, 4);
        add.Invoke(null, new[] { gameObject, classId, transform });
    }

    /// Point a child Transform at a parent and register the child in the parent's child list.
    ///
    /// `Father_C4P` is settable, but `Children_C4P` is a read-only view over the parent's stored
    /// child pointer list, so the child has to be added there too or the walk finds no children.
    private static void LinkParent(object child, object parent)
    {
        PropertyInfo? prop = child.GetType().GetProperty("Father_C4P");
        if (prop is null || !prop.CanWrite)
        {
            throw new InvalidOperationException($"no settable Father_C4P on {child.GetType().FullName}");
        }
        prop.SetValue(child, parent);

        // `Children_C4` is the stored child pointer list that `Children_C4P` views.
        object children = GetProp(parent, "Children_C4");
        object pointer = AddNewElement(children);
        SetAny(pointer, "FileID", 0L);
        SetAny(pointer, "PathID", GetProp(child, "PathID"));
    }

    private static IEnumerable<Type> SafeTypes(Assembly assembly)
    {
        try
        {
            return assembly.GetTypes();
        }
        catch (ReflectionTypeLoadException ex)
        {
            return ex.Types.Where(t => t is not null)!;
        }
    }

    private static string Format(object value)
    {
        // `System.Numerics` vectors and quaternions expose their components as public fields.
        List<string> parts = new();
        foreach (FieldInfo field in value.GetType().GetFields(BindingFlags.Public | BindingFlags.Instance))
        {
            if (field.FieldType == typeof(float))
            {
                parts.Add(((float)field.GetValue(value)!).ToString(Invariant));
            }
        }
        return string.Join(',', parts);
    }

    public static int Main(string[] args)
    {
        if (args.Length > 0 && args[0] == "quat")
        {
            // Call the shipped `GlbCoordinateConversion` helpers directly. The type is internal,
            // so this binds by reflection rather than by reference. Settles the sign question
            // for `ToGltfQuaternionConvert`, which composes an axis flip with the coordinate
            // scale rather than applying the coordinate scale alone.
            Assembly[] loaded = AppDomain.CurrentDomain.GetAssemblies();
            Type? conversion = loaded
                .SelectMany(SafeTypes)
                .FirstOrDefault(t => t.Name == "GlbCoordinateConversion");
            if (conversion is null)
            {
                // The type lives in the Models module, which may not be loaded yet.
                try
                {
                    Assembly models = Assembly.Load("AssetRipper.Export.Modules.Models");
                    conversion = SafeTypes(models)
                        .FirstOrDefault(t => t.Name == "GlbCoordinateConversion");
                }
                catch (Exception ex)
                {
                    Console.WriteLine($"QUAT|load-failed|{ex.GetType().Name}");
                }
            }
            if (conversion is null)
            {
                string searched = string.Join(',', loaded.Select(a => a.GetName().Name).OrderBy(n => n));
                Console.WriteLine($"QUAT|missing|GlbCoordinateConversion|{searched}");
                return 1;
            }
            MethodInfo quat = conversion.GetMethod(
                "ToGltfQuaternionConvert",
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)!;
            MethodInfo vec3 = conversion.GetMethod(
                "ToGltfVector3Convert",
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)!;
            FieldInfo scale = conversion.GetField(
                "CoordinateSpaceConversionScale",
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)!;
            object scaleValue = scale.GetValue(null)!;
            Console.WriteLine($"QUAT|scale|{Format(scaleValue)}");
            foreach ((float x, float y, float z, float w) input in new[]
            {
                (0f, 0f, 0f, 1f),
                (1f, 0f, 0f, 1f),
                (0f, 1f, 0f, 1f),
                (0f, 0f, 1f, 1f),
                (0.1f, 0.2f, 0.3f, 0.9f),
            })
            {
                object quaternion = Activator.CreateInstance(quat.GetParameters()[0].ParameterType, input.x, input.y, input.z, input.w)!;
                object converted = quat.Invoke(null, new[] { quaternion })!;
                object vector = Activator.CreateInstance(vec3.GetParameters()[0].ParameterType, input.x, input.y, input.z)!;
                object vectorConverted = vec3.Invoke(null, new[] { vector })!;
                Console.WriteLine(
                    $"QUAT|{input.x}|{input.y}|{input.z}|{input.w}|{Format(converted)}|{Format(vectorConverted)}");
            }
            return 0;
        }

        if (args.Length > 0 && args[0] == "level")
        {
            LevelCase("scene", "2019.4.0f1", true);
            LevelCase("asset", "2019.4.0f1", false);
            return 0;
        }

        if (args.Length > 0 && args[0] == "dump")
        {
            Assembly sg = typeof(AssetRipper.SourceGenerated.Classes.ClassID_156.TerrainData).Assembly;
            foreach (Type type in sg.GetTypes().Where(t => t.Name.Contains(args.Length > 1 ? args[1] : "TerrainData")))
            {
                if (type.IsPublic || type.IsNestedPublic)
                {
                    Console.WriteLine($"### {type.FullName} (base {type.BaseType?.FullName})");
                    foreach (PropertyInfo prop in type.GetProperties(BindingFlags.Public | BindingFlags.Instance))
                    {
                        Console.WriteLine($"  P {prop.PropertyType.Name} {prop.Name} set={prop.CanWrite}");
                    }
                }
            }
            return 0;
        }

        // Modern terrain: Width/Height are version stubs (0) and Resolution drives the grid.
        TerrainCase("res4", "2019.4.0f1", 0, 0, 4, (1f, 1f, 1f),
            Enumerable.Range(0, 16).Select(i => (short)(i * 2000)).ToArray(), "TerrainB");
        TerrainCase("res3scale", "2019.4.0f1", 0, 0, 3, (2f, 0.5f, 3f),
            new short[] { 0, 3000, 6000, 9000, 12000, 15000, 18000, 21000, 24000 }, "TerrainE");
        // Degenerate modern terrain: no width and no resolution yields no geometry.
        TerrainCase("empty", "2019.4.0f1", 0, 0, 0, (1f, 1f, 1f), new short[] { 0 }, "TerrainZ");
        // Legacy terrain: m_Width/m_Height are serialized and drive the grid.
        TerrainCase("legacy5", "5.0.0f1", 3, 3, 0, (1f, 2f, 1f),
            new short[] { 0, 8192, 16384, 8192, 16384, 24576, 16384, 24576, 32767 }, "TerrainA");
        // A non-square legacy grid indexes the height array out of range in the source.
        TerrainCase("rect48", "5.0.0f1", 4, 8, 0, (1f, 1f, 1f),
            Enumerable.Range(0, 32).Select(i => (short)i).ToArray(), "TerrainR");
        // A one-by-one grid divides by zero when the source builds its UV scale.
        TerrainCase("onebyone", "5.0.0f1", 1, 1, 0, (1f, 1f, 1f), new short[] { 10 }, "TerrainH");
        // Resolution above both dimensions makes the legacy grid square again.
        TerrainCase("legacyres", "5.0.0f1", 2, 2, 4, (1f, 1f, 1f),
            Enumerable.Range(0, 16).Select(i => (short)(i * 1000)).ToArray(), "TerrainI");

        // NavMesh: each triangle is emitted with reversed winding and no UV channel.
        NavMeshCase("two", "2019.4.0f1", "NavA",
            new[] { new[] { 0, 1, 2, 2, 1, 3 } },
            new[]
            {
                new[] { (0f, 0f, 0f), (1f, 0f, 0f), (0f, 1f, 0f), (1f, 1f, 0f) },
            });
        NavMeshCase("badcount", "2019.4.0f1", "NavB",
            new[] { new[] { 0, 1 } },
            new[]
            {
                new[] { (0f, 0f, 0f), (1f, 0f, 0f) },
            });
        return 0;
    }
}
