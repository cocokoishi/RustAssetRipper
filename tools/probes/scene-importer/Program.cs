using System.Reflection;
using System.Runtime.Loader;

if (args.Length == 0)
{
	Console.Error.WriteLine("usage: SceneImporterProbe <assembly-path> [...]");
	return 2;
}

string unityVersionText = Environment.GetEnvironmentVariable("SCENE_IMPORTER_UNITY_VERSION") ?? "2022.3.22f1";
var directories = args.Select(Path.GetFullPath).Select(Path.GetDirectoryName).Where(path => path is not null).Cast<string>().Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
AssemblyLoadContext.Default.Resolving += (_, name) =>
{
	foreach (string directory in directories)
	{
		string candidate = Path.Combine(directory, $"{name.Name}.dll");
		if (File.Exists(candidate))
		{
			return AssemblyLoadContext.Default.LoadFromAssemblyPath(candidate);
		}
	}
	return null;
};

foreach (string argument in args)
{
	string path = Path.GetFullPath(argument);
	Assembly assembly = AssemblyLoadContext.Default.LoadFromAssemblyPath(path);
	Type[] types;
	try
	{
		types = assembly.GetTypes();
	}
	catch (ReflectionTypeLoadException exception)
	{
		types = exception.Types.Where(type => type is not null).Cast<Type>().ToArray();
		foreach (Exception? loaderException in exception.LoaderExceptions)
		{
			if (loaderException is not null)
			{
				Console.Error.WriteLine($"LOAD_WARNING|{Path.GetFileName(path)}|{loaderException.GetType().Name}|{OneLine(loaderException.Message)}");
			}
		}
	}

	foreach (Type type in types.Where(IsDefaultImporterRelated).OrderBy(type => type.FullName, StringComparer.Ordinal))
	{
		if (type.IsInterface || type.Name != "DefaultImporter")
		{
			continue;
		}
		Console.WriteLine($"TYPE|{Path.GetFileName(path)}|{type.FullName}|base={type.BaseType?.FullName}");
		foreach (Type contract in type.GetInterfaces().Where(contract => contract.Name.Contains("Importer", StringComparison.Ordinal)).OrderBy(contract => contract.FullName, StringComparer.Ordinal))
		{
			Console.WriteLine($"INTERFACE|{contract.FullName}");
		}
		foreach (PropertyInfo property in type.GetProperties(BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly).OrderBy(property => property.Name, StringComparer.Ordinal))
		{
			Console.WriteLine($"PROPERTY|{property.Name}|{property.PropertyType.FullName}");
		}
		foreach (MethodInfo method in type.GetMethods(BindingFlags.Public | BindingFlags.Static | BindingFlags.DeclaredOnly)
			.Where(method =>
			{
				ParameterInfo[] parameters = method.GetParameters();
				return method.Name == "Create"
					&& parameters.Length == 2
					&& parameters[0].ParameterType.Name == "AssetCollection";
			})
			.OrderBy(method => method.ToString(), StringComparer.Ordinal))
		{
			Console.WriteLine($"FACTORY|{method}");
			object? importer;
			try
			{
				ParameterInfo[] parameters = method.GetParameters();
				object version = CreateUnityVersion(parameters[^1].ParameterType, unityVersionText);
				importer = method.Invoke(null, [null, version]);
				Console.WriteLine($"CREATE|{method.GetParameters()[0].ParameterType.Name}|ok");
			}
			catch (Exception exception)
			{
				Exception source = exception is TargetInvocationException { InnerException: not null } invocation
					? invocation.InnerException!
					: exception;
				Console.WriteLine($"CREATE|{method.GetParameters()[0].ParameterType.Name}|error|{source.GetType().Name}|{OneLine(source.Message)}");
				continue;
			}
			if (importer is null)
			{
				Console.WriteLine("CREATE|result|null");
				continue;
			}
			string[] selectedProperties = ["AssetBundleName", "AssetBundleName_R", "AssetBundleVariant", "ExternalObjects", "Name", "UserData"];
			foreach (PropertyInfo property in importer.GetType().GetProperties(BindingFlags.Public | BindingFlags.Instance)
				.Where(property => selectedProperties.Contains(property.Name, StringComparer.Ordinal))
				.OrderBy(property => property.Name, StringComparer.Ordinal))
			{
				try
				{
					object? value = property.GetValue(importer);
					string rendered = value is null ? "<null>" : value is string text ? text : value.ToString() ?? $"<{value.GetType().Name}>";
					Console.WriteLine($"VALUE|{property.Name}|{OneLine(rendered)}");
				}
				catch (Exception exception)
				{
					Exception source = exception is TargetInvocationException { InnerException: not null } invocation
						? invocation.InnerException!
						: exception;
					Console.WriteLine($"VALUE|{property.Name}|ERROR|{source.GetType().Name}|{OneLine(source.Message)}");
				}
			}
		}
	}
}

return 0;

static bool IsDefaultImporterRelated(Type type) =>
	type.Name.Contains("DefaultImporter", StringComparison.Ordinal)
	|| type.GetInterfaces().Any(contract => contract.Name.Contains("DefaultImporter", StringComparison.Ordinal));

static string OneLine(string value) => value.Replace('\r', ' ').Replace('\n', ' ');

static object CreateUnityVersion(Type versionType, string versionText)
{
	MethodInfo parse = versionType.GetMethod("Parse", BindingFlags.Public | BindingFlags.Static, [typeof(string)])
		?? throw new MissingMethodException(versionType.FullName, "Parse(string)");
	return parse.Invoke(null, [versionText]) ?? throw new InvalidOperationException("UnityVersion.Parse returned null");
}
