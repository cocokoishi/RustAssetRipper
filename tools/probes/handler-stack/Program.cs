// In which order does `ObjectHandlerStack<T>.GetHandlerStack` yield its handlers?
//
// This decides every primary-content asset's extractor. `PrimaryContentExporter` registers
// `JsonContentExtractor` for `IUnityObjectBase` **first** and `DeletedAssetsExporter` for
// `IUnityObjectBase` **last**, and `IComponent` twice - once as an empty handler and later as the GLB
// model exporter. Which of those wins depends entirely on whether enumeration is registration order or
// its reverse, and reading the code alone invites the wrong answer because `OverrideHandler` sounds
// like it replaces an earlier registration while `Stack<T>.Push` implies the opposite.
//
// `ObjectHandlerStack<T>` is public and independent of any asset type, so the order can simply be
// measured with `string` handlers.
//
// Run:
//   dotnet run --project tools/probes/handler-stack/HandlerStackProbe.csproj -c Release -- <refroot>
using System.Reflection;
using System.Runtime.Loader;

public interface IRoot
{
}

public interface IChild : IRoot
{
}

public sealed class Leaf : IChild
{
}

public static class Program
{
    public static int Main(string[] args)
    {
        string root = args.Length > 0
            ? args[0]
            : Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "Temp", "opencode", "ar-ref", "Source", "0Bins", "AssetRipper.GUI.Free", "Release");
        AssemblyLoadContext.Default.Resolving += (_, name) =>
        {
            string candidate = Path.Combine(root, $"{name.Name}.dll");
            return File.Exists(candidate) ? Assembly.LoadFrom(candidate) : null;
        };

        Assembly export = Assembly.LoadFrom(Path.Combine(root, "AssetRipper.Export.dll"));
        Type stackOpen = export.GetType("AssetRipper.Export.ObjectHandlerStack`1");
        if (stackOpen is null)
        {
            Console.WriteLine("PROBE|no-handler-stack");
            return 1;
        }
        Type stackType = stackOpen.MakeGenericType(typeof(string));
        Console.WriteLine($"PROBE|type|{stackOpen.FullName}");

        MethodInfo overrideHandler = stackType.GetMethod("OverrideHandler")!;
        MethodInfo getStack = stackType.GetMethod("GetHandlerStack")!;

        // Two handlers for the same exact type: which comes first?
        object two = Activator.CreateInstance(stackType)!;
        overrideHandler.Invoke(two, new object[] { typeof(IRoot), "first", true });
        overrideHandler.Invoke(two, new object[] { typeof(IRoot), "second", true });
        Dump(getStack, two, typeof(IRoot), "same-type-twice");

        // Three, to distinguish "reverse order" from "last wins only".
        object three = Activator.CreateInstance(stackType)!;
        overrideHandler.Invoke(three, new object[] { typeof(IRoot), "one", true });
        overrideHandler.Invoke(three, new object[] { typeof(IRoot), "two", true });
        overrideHandler.Invoke(three, new object[] { typeof(IRoot), "three", true });
        Dump(getStack, three, typeof(IRoot), "same-type-thrice");

        // Inheritance: a handler for the base interface, then one for the derived interface, queried
        // through the leaf class.
        object chain = Activator.CreateInstance(stackType)!;
        overrideHandler.Invoke(chain, new object[] { typeof(IRoot), "root", true });
        overrideHandler.Invoke(chain, new object[] { typeof(IChild), "child", true });
        Dump(getStack, chain, typeof(Leaf), "leaf-through-both");

        // `allowInheritance = false` must exclude the handler for a queried subtype.
        object noInherit = Activator.CreateInstance(stackType)!;
        overrideHandler.Invoke(noInherit, new object[] { typeof(IRoot), "root", false });
        overrideHandler.Invoke(noInherit, new object[] { typeof(IChild), "child", true });
        Dump(getStack, noInherit, typeof(Leaf), "leaf-with-exact-root-only");
        Dump(getStack, noInherit, typeof(IRoot), "root-itself");

        // A cache is involved: querying the same type twice must return the same sequence, and the
        // cache is cleared when a handler is added. Confirm a late registration shows up.
        object cached = Activator.CreateInstance(stackType)!;
        overrideHandler.Invoke(cached, new object[] { typeof(IRoot), "before", true });
        Dump(getStack, cached, typeof(IRoot), "before-late-add");
        overrideHandler.Invoke(cached, new object[] { typeof(IRoot), "after", true });
        Dump(getStack, cached, typeof(IRoot), "after-late-add");

        // Repeat the same query twice with no registration in between, to show the cache is not
        // observably reordering anything.
        object repeated = Activator.CreateInstance(stackType)!;
        overrideHandler.Invoke(repeated, new object[] { typeof(IRoot), "a", true });
        overrideHandler.Invoke(repeated, new object[] { typeof(IRoot), "b", true });
        Dump(getStack, repeated, typeof(IRoot), "repeat-first");
        Dump(getStack, repeated, typeof(IRoot), "repeat-second");
        return 0;
    }

    private static void Dump(MethodInfo getStack, object stack, Type queried, string name)
    {
        var handlers = ((System.Collections.IEnumerable)getStack.Invoke(stack, new object[] { queried })!)
            .Cast<object>()
            .Select(h => h?.ToString() ?? "<null>");
        Console.WriteLine($"STACK|{name}|{queried.Name}|{string.Join(",", handlers)}");
    }
}
