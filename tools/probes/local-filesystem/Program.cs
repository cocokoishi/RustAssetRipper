using System.Text;
using System.Text.RegularExpressions;
using AssetRipper.IO.Files;

if (args.Length != 1)
{
	throw new ArgumentException("Expected one scratch directory path.");
}

string scratch = Path.GetFullPath(args[0]);
Directory.CreateDirectory(scratch);
LocalFileSystem fileSystem = new();
string defaultTemporaryDirectory = fileSystem.TemporaryDirectory;
string defaultRelative = Path.GetRelativePath(AppContext.BaseDirectory, defaultTemporaryDirectory).Replace('\\', '/');
Console.WriteLine($"DEFAULT_TEMP|app-local={Regex.IsMatch(defaultRelative, "^temp/[0-9a-fA-F]{4}$")}|exists={Directory.Exists(defaultTemporaryDirectory)}");

string configured = Path.Combine(scratch, "custom", "..", "temp");
fileSystem.TemporaryDirectory = configured;
string expectedTemporaryDirectory = Path.GetFullPath(configured);
Console.WriteLine($"SET_TEMP|full={fileSystem.TemporaryDirectory == expectedTemporaryDirectory}|normalized={!fileSystem.TemporaryDirectory.Contains("..", StringComparison.Ordinal)}");
fileSystem.TemporaryDirectory = "   ";
Console.WriteLine($"BLANK_SETTER|unchanged={fileSystem.TemporaryDirectory == expectedTemporaryDirectory}");

string temporaryFile = fileSystem.File.CreateTemporary();
Console.WriteLine($"TEMP_FILE|inside={Path.GetDirectoryName(temporaryFile) == expectedTemporaryDirectory}|exists={File.Exists(temporaryFile)}|length={new FileInfo(temporaryFile).Length}|guid={Regex.IsMatch(Path.GetFileName(temporaryFile), "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")}");

string temporaryChild = fileSystem.Directory.CreateTemporary();
Console.WriteLine($"TEMP_DIRECTORY|inside={Directory.GetParent(temporaryChild)?.FullName == expectedTemporaryDirectory}|guid8={Regex.IsMatch(Path.GetFileName(temporaryChild), "^[0-9a-fA-F]{8}$")}|exists={Directory.Exists(temporaryChild)}");

string openWritePath = Path.Combine(scratch, "openwrite.bin");
fileSystem.File.WriteAllBytes(openWritePath, "abcdef"u8);
using (Stream writer = fileSystem.File.OpenWrite(openWritePath))
{
	long initialPosition = writer.Position;
	writer.Write("XY"u8);
	Console.WriteLine($"OPEN_WRITE|initial-position={initialPosition}");
}
Console.WriteLine($"OPEN_WRITE|bytes={Convert.ToHexString(fileSystem.File.ReadAllBytes(openWritePath))}");

string retainedPath = Path.Combine(scratch, "retained.bin");
fileSystem.File.WriteAllBytes(retainedPath, "before"u8);
using (Stream reader = fileSystem.File.OpenRead(retainedPath))
{
	string outcome;
	try
	{
		fileSystem.File.WriteAllBytes(retainedPath, "after"u8);
		outcome = "ALLOWED";
	}
	catch (IOException)
	{
		outcome = "REJECTED";
	}
	Console.WriteLine($"WRITE_WHILE_READ_OPEN|{outcome}");
}

string nestedFile = Path.Combine(temporaryChild, "nested", "file.bin");
fileSystem.Directory.Create(Path.GetDirectoryName(nestedFile)!);
fileSystem.File.WriteAllBytes(nestedFile, "temp"u8);
fileSystem.DeleteTemporaryDirectory();
Console.WriteLine($"DELETE_TEMP|removed={ !Directory.Exists(expectedTemporaryDirectory) }|repeat={DeleteAgain(fileSystem)}");

Console.WriteLine($"DELETE_MISSING_FILE|no-op={DeleteMissing(fileSystem, Path.Combine(scratch, "missing"))}");
Console.WriteLine($"PATH|rooted-dot={fileSystem.Path.IsPathRooted(".")}|rooted-root={fileSystem.Path.IsPathRooted(Path.GetPathRoot(scratch)!)}|full-relative={Path.IsPathRooted(fileSystem.Path.GetFullPath("."))}");

string enumerationRoot = Path.Combine(scratch, "enumeration");
Directory.CreateDirectory(Path.Combine(enumerationRoot, "nested"));
Directory.CreateDirectory(Path.Combine(enumerationRoot, "nested", "deep"));
Directory.CreateDirectory(Path.Combine(enumerationRoot, "other"));
File.WriteAllText(Path.Combine(enumerationRoot, "alpha.txt"), "alpha");
File.WriteAllText(Path.Combine(enumerationRoot, "BETA.TXT"), "beta");
File.WriteAllText(Path.Combine(enumerationRoot, "no-extension"), "none");
File.WriteAllText(Path.Combine(enumerationRoot, ".hidden"), "hidden");
File.WriteAllText(Path.Combine(enumerationRoot, "nested", "inner.txt"), "inner");
File.WriteAllText(Path.Combine(enumerationRoot, "nested", "deep", "deep.txt"), "deep");
File.WriteAllText(Path.Combine(enumerationRoot, "other", "other.bin"), "other");
string RelativeList(IEnumerable<string> paths) => string.Join(",", paths
	.Select(path => Path.GetRelativePath(enumerationRoot, path).Replace('\\', '/'))
	.Order(StringComparer.Ordinal));
Console.WriteLine($"ENUM_FILES|all={RelativeList(fileSystem.Directory.GetFiles(enumerationRoot))}");
Console.WriteLine($"ENUM_FILES|txt={RelativeList(fileSystem.Directory.GetFiles(enumerationRoot, "*.txt"))}");
Console.WriteLine($"ENUM_FILES|star-dot-star={RelativeList(fileSystem.Directory.GetFiles(enumerationRoot, "*.*"))}");
Console.WriteLine($"ENUM_FILES|recursive={RelativeList(fileSystem.Directory.GetFiles(enumerationRoot, "*.txt", SearchOption.AllDirectories))}");
Console.WriteLine($"ENUMERATE_FILES|all={RelativeList(fileSystem.Directory.EnumerateFiles(enumerationRoot))}");
Console.WriteLine($"ENUMERATE_FILES|txt={RelativeList(fileSystem.Directory.EnumerateFiles(enumerationRoot, "*.txt"))}");
Console.WriteLine($"ENUMERATE_FILES|recursive={RelativeList(fileSystem.Directory.EnumerateFiles(enumerationRoot, "*.txt", SearchOption.AllDirectories))}");
Console.WriteLine($"ENUM_DIRS|top={RelativeList(fileSystem.Directory.GetDirectories(enumerationRoot))}");
Console.WriteLine($"ENUM_DIRS|recursive={RelativeList(fileSystem.Directory.GetDirectories(enumerationRoot, "*", SearchOption.AllDirectories))}");
Console.WriteLine($"ENUMERATE_DIRS|top={RelativeList(fileSystem.Directory.EnumerateDirectories(enumerationRoot))}");
Console.WriteLine($"ENUMERATE_DIRS|pattern={RelativeList(fileSystem.Directory.EnumerateDirectories(enumerationRoot, "*"))}");
Console.WriteLine($"ENUMERATE_DIRS|recursive={RelativeList(fileSystem.Directory.EnumerateDirectories(enumerationRoot, "*", SearchOption.AllDirectories))}");

string utf8Path = Path.Combine(scratch, "utf8.txt");
fileSystem.File.WriteAllText(utf8Path, "café");
Console.WriteLine($"TEXT|default-write={Convert.ToHexString(File.ReadAllBytes(utf8Path))}");
File.WriteAllBytes(utf8Path, [0xEF, 0xBB, 0xBF, 0x63, 0x61, 0x66, 0xC3, 0xA9]);
Console.WriteLine($"TEXT|utf8-bom-read={Convert.ToHexString(Encoding.UTF8.GetBytes(fileSystem.File.ReadAllText(utf8Path)))}");
File.WriteAllBytes(utf8Path, [0xFF, 0xFE, 0x63, 0, 0xE9, 0]);
Console.WriteLine($"TEXT|utf16-bom-read={Convert.ToHexString(Encoding.UTF8.GetBytes(fileSystem.File.ReadAllText(utf8Path)))}");
string explicitEncodingPath = Path.Combine(scratch, "explicit-encoding.txt");
fileSystem.File.WriteAllText(explicitEncodingPath, "café", Encoding.Unicode);
Console.WriteLine($"TEXT|utf16-write={Convert.ToHexString(File.ReadAllBytes(explicitEncodingPath))}");

Console.WriteLine($"PATH_HELPERS|join={fileSystem.Path.Join("alpha", "beta", "gamma")}");
Console.WriteLine($"PATH_HELPERS|join-rooted={fileSystem.Path.Join("alpha", "\\beta")}");
Console.WriteLine($"PATH_HELPERS|directory={fileSystem.Path.GetDirectoryName("alpha\\beta.ext")}");
Console.WriteLine($"PATH_HELPERS|extension={fileSystem.Path.GetExtension("alpha\\beta.ext")}");
Console.WriteLine($"PATH_HELPERS|filename={fileSystem.Path.GetFileName("alpha\\beta.ext")}");
Console.WriteLine($"PATH_HELPERS|filename-no-extension={fileSystem.Path.GetFileNameWithoutExtension("alpha\\beta.ext")}");
Console.WriteLine($"PATH_HELPERS|dotfile-extension={fileSystem.Path.GetExtension(".hidden")}");
Console.WriteLine($"PATH_HELPERS|trailing-filename={fileSystem.Path.GetFileName("alpha\\beta\\")}");
Console.WriteLine($"PATH_HELPERS|rooted-drive-relative={fileSystem.Path.IsPathRooted("C:relative")}");
Console.WriteLine($"PATH_HELPERS|relative={fileSystem.Path.GetRelativePath(Path.Combine(scratch, "base"), Path.Combine(scratch, "base", "a", "b.txt"))}");

static bool DeleteAgain(LocalFileSystem fileSystem)
{
	fileSystem.DeleteTemporaryDirectory();
	return true;
}

static bool DeleteMissing(LocalFileSystem fileSystem, string path)
{
	fileSystem.File.Delete(path);
	return !File.Exists(path);
}
