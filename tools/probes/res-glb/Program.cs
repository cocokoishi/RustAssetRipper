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

    public static int Main(string[] args)
    {
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
