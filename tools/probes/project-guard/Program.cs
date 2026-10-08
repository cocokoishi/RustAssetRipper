using System.Reflection;
using System.Text;

// Reference-runtime probe for the export root guard in the frozen AssetRipper baseline.
//
// Source under test: AssetRipper/Source/AssetRipper.GUI.Web/GameFileLoader.cs
//   * private static bool IsValidExportDirectory(string path)   (lines 108-122)
//   * private static bool IsNonEmptyDirectory(string path)      (lines 124-127)
//   * private static async Task<bool> UserConsentsToDeletion()  (lines 129-142)
//
// The probe loads the already built reference product assembly by reflection so that no
// compile-time reference to AssetRipper projects (and therefore no restore of the whole
// reference solution) is required. It then evaluates a fixed case matrix and writes it as
// a tab separated fixture consumed by src/export/export_root.rs.
//
// Usage: ProjectGuardProbe [reference-bin-directory] [fixture-output-path]

string binRoot = args.Length > 0
	? args[0]
	: Path.Combine(
		Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
		"Temp",
		"opencode",
		"ar-ref",
		"Source",
		"0Bins",
		"AssetRipper.GUI.Free",
		"Release");
string? fixturePath = args.Length > 1 ? args[1] : null;

string assemblyPath = Path.Combine(binRoot, "AssetRipper.GUI.Web.dll");
if (!File.Exists(assemblyPath))
{
	Console.Error.WriteLine($"missing reference assembly: {assemblyPath}");
	return 2;
}

AppDomain.CurrentDomain.AssemblyResolve += (_, eventArgs) =>
{
	string simpleName = new AssemblyName(eventArgs.Name).Name + ".dll";
	string candidate = Path.Combine(binRoot, simpleName);
	return File.Exists(candidate) ? Assembly.LoadFrom(candidate) : null;
};

Assembly webAssembly = Assembly.LoadFrom(assemblyPath);
Type loader = webAssembly.GetType("AssetRipper.GUI.Web.GameFileLoader", throwOnError: true)!;
const BindingFlags StaticMethod = BindingFlags.NonPublic | BindingFlags.Static;
MethodInfo isValidExportDirectory = loader.GetMethod("IsValidExportDirectory", StaticMethod)!;
MethodInfo isNonEmptyDirectory = loader.GetMethod("IsNonEmptyDirectory", StaticMethod)!;
MethodInfo userConsentsToDeletion = loader.GetMethod("UserConsentsToDeletion", StaticMethod)!;
PropertyInfo headless = loader.GetProperty("Headless", BindingFlags.Public | BindingFlags.Static)!;

// The work root stays inside the probe output directory because the host blocks writes to the
// user temporary directory; only the recorded path templates matter for the fixture.
string work = Path.Combine(AppContext.BaseDirectory, "work-root");
if (Directory.Exists(work))
{
	Directory.Delete(work, true);
}

Directory.CreateDirectory(work);

Directory.CreateDirectory(Path.Combine(work, "empty-dir"));
Directory.CreateDirectory(Path.Combine(work, "dir-with-file"));
File.WriteAllText(Path.Combine(work, "dir-with-file", "payload.txt"), "x");
Directory.CreateDirectory(Path.Combine(work, "dir-with-subdir", "nested"));
Directory.CreateDirectory(Path.Combine(work, "dir-with-hidden"));
File.WriteAllText(Path.Combine(work, "dir-with-hidden", ".keep"), "x");
File.WriteAllText(Path.Combine(work, "a-file.txt"), "x");

bool directorySymlinkCreated = false;
bool fileSymlinkCreated = false;
try
{
	Directory.CreateSymbolicLink(Path.Combine(work, "dir-symlink"), Path.Combine(work, "dir-with-file"));
	directorySymlinkCreated = true;
}
catch
{
	// Symlink creation needs privileges; the row is recorded as skipped when unavailable.
}

try
{
	File.CreateSymbolicLink(Path.Combine(work, "file-symlink"), Path.Combine(work, "a-file.txt"));
	fileSymlinkCreated = true;
}
catch
{
}

List<string> rows = new();
void Row(string check, string platform, string template, string result)
{
	rows.Add($"{check}\t{platform}\t{template}\t{result}");
}

string Resolve(string template)
{
	if (template == "{empty}")
	{
		return string.Empty;
	}

	return template.Replace("{tmp}", work);
}

T Invoke<T>(MethodInfo method, string template)
{
	try
	{
		return (T)method.Invoke(null, new object?[] { Resolve(template) })!;
	}
	catch (TargetInvocationException error)
	{
		throw error.InnerException!;
	}
}

string[] validCases =
{
	"{tmp}",
	"{tmp}\\project",
	"{tmp}\\project\\Desktop",
	"{tmp}\\project\\Documents",
	"{tmp}\\project\\Downloads",
	"{tmp}\\project\\desktop",
	"{tmp}\\project\\Desktop.txt",
	"{tmp}\\project\\Documents\\Desktop",
	"{tmp}\\project\\Desktop\\",
	"{tmp}\\project\\Desktop\\.",
	"{tmp}\\project\\Desktop ",
	"{tmp}\\project\\Downloads\\nested",
	"{tmp}\\project\\DesktopX",
	"{tmp}/project/Desktop",
	"Desktop",
	"Documents",
	"Downloads",
	"Desktop/",
	"C:\\Desktop",
	"{empty}",
};

foreach (string template in validCases)
{
	Row("valid", "win", template, Invoke<bool>(isValidExportDirectory, template) ? "valid" : "invalid");
}

string[] nonEmptyCases =
{
	"{tmp}\\empty-dir",
	"{tmp}\\dir-with-file",
	"{tmp}\\dir-with-subdir",
	"{tmp}\\dir-with-hidden",
	"{tmp}\\a-file.txt",
	"{tmp}\\absent-dir",
	"{tmp}\\dir-with-file\\payload.txt",
};

foreach (string template in nonEmptyCases)
{
	Row("nonempty", "win", template, Invoke<bool>(isNonEmptyDirectory, template) ? "nonempty" : "empty");
}

// The source uses Directory.Exists/EnumerateFileSystemEntries, which follow links, so a link to
// a non-empty directory must be reported as non-empty.
Row(
	"nonempty",
	"win",
	"{tmp}\\dir-symlink",
	directorySymlinkCreated
		? (Invoke<bool>(isNonEmptyDirectory, "{tmp}\\dir-symlink") ? "nonempty" : "empty")
		: "skip");
Row(
	"nonempty",
	"win",
	"{tmp}\\file-symlink",
	fileSymlinkCreated
		? (Invoke<bool>(isNonEmptyDirectory, "{tmp}\\file-symlink") ? "nonempty" : "empty")
		: "skip");
Row(
	"valid",
	"win",
	"{tmp}\\dir-symlink",
	Invoke<bool>(isValidExportDirectory, "{tmp}\\dir-symlink") ? "valid" : "invalid");
Row(
	"valid",
	"win",
	"{tmp}\\file-symlink",
	Invoke<bool>(isValidExportDirectory, "{tmp}\\file-symlink") ? "valid" : "invalid");

string[] fileNameCases =
{
	"{empty}",
	"{tmp}",
	"{tmp}\\a-file.txt",
	"{tmp}\\a-file.txt\\",
	"{tmp}\\a\\b",
	"{tmp}\\a.tar.gz",
	"{tmp}\\a\\",
	"relative",
	"relative\\",
	"relative\\nested",
	"/",
	"/root",
	"//root",
	"///root",
	"/root/",
	"\\root",
	"\\\\root",
	"\\",
	"\\\\",
	"\\\\\\",
	"\\\\server\\share\\file",
	"\\\\server\\share\\",
	"\\\\server\\share",
	"C:",
	"C:\\",
	"C:\\a",
	"C:/a",
	"C:a",
	"C:\\a\\",
	"C:\\a\\b",
	"a:b",
	"a:b\\c",
	"a\\b:c",
	".",
	"..",
	"...",
	"a/",
	"a//b",
	"x/y/z",
	"\\\\.\\C:\\x",
	"\\\\?\\C:\\x\\y",
	"\\\\?\\UNC\\server\\share\\file",
	"a b c",
	"Desktop",
	"Desktop/",
	"Desktop\\",
	"{tmp}\\Desktop\\",
	"file.txt ",
};

foreach (string template in fileNameCases)
{
	string name = Path.GetFileName(Resolve(template));
	string result = name.Length == 0 ? "{empty}" : name;
	if (template == "{tmp}")
	{
		// The replay test substitutes its own work directory, so the expectation is
		// expressed as the directory name that test itself chose.
		if (name != new DirectoryInfo(work).Name)
		{
			throw new InvalidOperationException($"unexpected work directory name '{name}'");
		}

		result = "{tmpname}";
	}

	Row("filename", "win", template, result);
}

try
{
	headless.SetValue(null, true);
	Task<bool> decision = (Task<bool>)userConsentsToDeletion.Invoke(null, null)!;
	Row("consent", "any", "headless", decision.GetAwaiter().GetResult() ? "true" : "false");
}
catch (Exception error)
{
	Row("consent", "any", "headless", "error:" + error.GetType().Name);
}

StringBuilder output = new();
output.Append("# Reference-runtime project export root guard differential.\n");
output.Append("# producer: tools/probes/project-guard (reflection over AssetRipper.GUI.Web.dll)\n");
output.Append("# check\tplatform\tpath-template\tresult\n");
output.Append("# {tmp} is the probe work directory and is substituted by the replay test.\n");
output.Append("# {empty} is the empty string. platform=win rows are Windows-only.\n");
foreach (string row in rows)
{
	output.Append(row).Append('\n');
}

string text = output.ToString();
if (fixturePath is not null)
{
	File.WriteAllText(fixturePath, text, new UTF8Encoding(false));
}

Console.Out.Write(text);
return 0;
