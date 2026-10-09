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

VirtualFileSystem virtualFileSystem = new();
(string Name, byte[] Bytes)[] malformedUtf8Corpus =
[
	("isolated-invalid", [0xFF]),
	("stray-continuations", [0x80, 0x81]),
	("bad-continuation", [0xC3, 0x28]),
	("invalid-lead-followed-by-ascii", [0xF5, 0x41]),
	("truncated-three-byte", [0xE2, 0x82]),
	("overlong", [0xC0, 0xAF]),
	("surrogate-scalar", [0xED, 0xA0, 0x80]),
	("above-unicode-range", [0xF4, 0x90, 0x80, 0x80]),
];
foreach ((string name, byte[] bytes) in malformedUtf8Corpus)
{
	string path = $"/malformed-{name}.bin";
	virtualFileSystem.File.WriteAllBytes(path, bytes);
	string text = virtualFileSystem.File.ReadAllText(path);
	Console.WriteLine($"VFS_TEXT|{name}={Convert.ToHexString(Encoding.UTF8.GetBytes(text))}");
}

foreach (EncodingInfo encoding in Encoding.GetEncodings().OrderBy(encoding => encoding.CodePage))
{
	Console.WriteLine($"ENCODING|{encoding.CodePage}|{encoding.Name}");
	Encoding instance = Encoding.GetEncoding(encoding.CodePage);
	string path = Path.Combine(scratch, $"encoding-{encoding.CodePage}.txt");
	fileSystem.File.WriteAllText(path, "A", instance);
	Console.WriteLine($"ENCODING_WRITE|{encoding.CodePage}|{Convert.ToHexString(File.ReadAllBytes(path))}");
	virtualFileSystem.File.WriteAllText($"/encoding-{encoding.CodePage}.txt", "A", instance);
	Console.WriteLine($"VFS_ENCODING_WRITE|{encoding.CodePage}|{Convert.ToHexString(virtualFileSystem.File.ReadAllBytes($"/encoding-{encoding.CodePage}.txt"))}");
	virtualFileSystem.File.WriteAllBytes($"/encoding-invalid-{encoding.CodePage}.bin", [0xFF]);
	Console.WriteLine($"VFS_ENCODING_READ|{encoding.CodePage}|{Convert.ToHexString(Encoding.UTF8.GetBytes(virtualFileSystem.File.ReadAllText($"/encoding-invalid-{encoding.CodePage}.bin", instance)))}");
	string invalidPath = Path.Combine(scratch, $"encoding-invalid-{encoding.CodePage}.bin");
	File.WriteAllBytes(invalidPath, [0xFF]);
	Console.WriteLine($"LOCAL_ENCODING_READ|{encoding.CodePage}|{Convert.ToHexString(Encoding.UTF8.GetBytes(fileSystem.File.ReadAllText(invalidPath, instance)))}");
	Encoding custom = Encoding.GetEncoding(encoding.CodePage,
		new EncoderReplacementFallback("<encode>"), new DecoderReplacementFallback("<decode>"));
	virtualFileSystem.File.WriteAllBytes($"/custom-invalid-{encoding.CodePage}.bin", [0xFF]);
	Console.WriteLine($"VFS_CUSTOM_READ|{encoding.CodePage}|{Convert.ToHexString(Encoding.UTF8.GetBytes(virtualFileSystem.File.ReadAllText($"/custom-invalid-{encoding.CodePage}.bin", custom)))}");
	string customInvalidPath = Path.Combine(scratch, $"custom-invalid-{encoding.CodePage}.bin");
	File.WriteAllBytes(customInvalidPath, [0xFF]);
	Console.WriteLine($"LOCAL_CUSTOM_READ|{encoding.CodePage}|{Convert.ToHexString(Encoding.UTF8.GetBytes(fileSystem.File.ReadAllText(customInvalidPath, custom)))}");
	virtualFileSystem.File.WriteAllText($"/replacement-write-{encoding.CodePage}.txt", "Aé🙂", custom);
	Console.WriteLine($"VFS_REPLACEMENT_WRITE|{encoding.CodePage}|{Convert.ToHexString(virtualFileSystem.File.ReadAllBytes($"/replacement-write-{encoding.CodePage}.txt"))}");
	string replacementWritePath = Path.Combine(scratch, $"replacement-write-{encoding.CodePage}.txt");
	fileSystem.File.WriteAllText(replacementWritePath, "Aé🙂", custom);
	Console.WriteLine($"LOCAL_REPLACEMENT_WRITE|{encoding.CodePage}|{Convert.ToHexString(File.ReadAllBytes(replacementWritePath))}");
}

#pragma warning disable SYSLIB0001
Encoding utf7 = Encoding.UTF7;
#pragma warning restore SYSLIB0001
foreach ((int CodePage, byte[] Bytes) item in new (int, byte[])[]
{
	(1200, [0x00, 0xD8, 0x41, 0x00, 0x42]),
	(1201, [0xD8, 0x00, 0x00, 0x41, 0x42]),
	(12000, [0x00, 0xD8, 0x00, 0x00, 0x00, 0x00, 0x11, 0x00]),
	(12001, [0x00, 0x00, 0xD8, 0x00, 0x00, 0x11, 0x00, 0x00]),
	(20127, [0x80, 0x41]),
	(28591, [0x80, 0xFF]),
	(65001, [0xED, 0xA0, 0x80]),
	(65000, [(byte)'+', (byte)'2', (byte)'A', (byte)'A', (byte)'-']),
})
{
	Encoding encoding = item.CodePage == 65000 ? utf7 : Encoding.GetEncoding(item.CodePage);
	string vfsPath = $"/malformed-{item.CodePage}.bin";
	virtualFileSystem.File.WriteAllBytes(vfsPath, item.Bytes);
	Console.WriteLine($"VFS_MALFORMED|{item.CodePage}|{Convert.ToHexString(Encoding.UTF8.GetBytes(virtualFileSystem.File.ReadAllText(vfsPath, encoding)))}");
	string localPath = Path.Combine(scratch, $"malformed-{item.CodePage}.bin");
	File.WriteAllBytes(localPath, item.Bytes);
	Console.WriteLine($"LOCAL_MALFORMED|{item.CodePage}|{Convert.ToHexString(Encoding.UTF8.GetBytes(fileSystem.File.ReadAllText(localPath, encoding)))}");
}

foreach ((string Name, Encoding Encoding) encoding in new (string, Encoding)[]
{
	("utf8-static", Encoding.UTF8),
	("utf8-codepage", Encoding.GetEncoding(65001)),
	("utf16le-static", Encoding.Unicode),
	("utf16be-static", Encoding.BigEndianUnicode),
	("utf32le-static", Encoding.UTF32),
	("utf32be-static", new UTF32Encoding(bigEndian: true, byteOrderMark: true)),
})
{
	string path = Path.Combine(scratch, $"encoding-{encoding.Name}.txt");
	fileSystem.File.WriteAllText(path, "A", encoding.Encoding);
	Console.WriteLine($"ENCODING_WRITE|{encoding.Name}|{Convert.ToHexString(File.ReadAllBytes(path))}");
}

Console.WriteLine($"ENCODING|{utf7.CodePage}|utf-7-static");
Console.WriteLine($"UTF7_DEFAULT_FALLBACKS|{utf7.EncoderFallback.GetType().Name}:{utf7.EncoderFallback}|{utf7.DecoderFallback.GetType().Name}:{utf7.DecoderFallback}");
string utf7Path = Path.Combine(scratch, "encoding-utf7.txt");
fileSystem.File.WriteAllText(utf7Path, "Aé🙂", utf7);
Console.WriteLine($"LOCAL_UTF7_WRITE|{Convert.ToHexString(File.ReadAllBytes(utf7Path))}");
virtualFileSystem.File.WriteAllText("/utf7.txt", "Aé🙂", utf7);
Console.WriteLine($"VFS_UTF7_WRITE|{Convert.ToHexString(virtualFileSystem.File.ReadAllBytes("/utf7.txt"))}");
virtualFileSystem.File.WriteAllBytes("/utf7-invalid.bin", [0xFF]);
Console.WriteLine($"VFS_UTF7_READ|{Convert.ToHexString(Encoding.UTF8.GetBytes(virtualFileSystem.File.ReadAllText("/utf7-invalid.bin", utf7)))}");
File.WriteAllBytes(utf7Path, [0xFF]);
Console.WriteLine($"LOCAL_UTF7_READ|{Convert.ToHexString(Encoding.UTF8.GetBytes(fileSystem.File.ReadAllText(utf7Path, utf7)))}");
Console.WriteLine($"UTF7_PRINTABLE|{Convert.ToHexString(utf7.GetBytes(new string(Enumerable.Range(0x20, 0x5F).Select(value => (char)value).ToArray())))}");
foreach ((string Name, string Text) item in new (string, string)[]
{
	("plus", "+"),
	("two-plus", "++"),
	("direct-plus", "x+"),
	("plus-direct", "+x"),
	("optional-plus", "*+"),
	("plus-optional", "+*"),
	("plus-unicode", "+é"),
	("unicode-plus", "é+"),
	("unicode-plus-direct", "é+A"),
})
{
	Console.WriteLine($"UTF7_ENCODE|{item.Name}|{Convert.ToHexString(utf7.GetBytes(item.Text))}");
}
foreach ((string Name, byte[] Bytes) item in new (string, byte[])[]
{
	("plain", [(byte)'A', (byte)'~']),
	("plus", [(byte)'+', (byte)'-']),
	("shift-e-acute", [(byte)'+', (byte)'A', (byte)'O', (byte)'k', (byte)'-']),
	("shift-emoji", [(byte)'+', (byte)'2', (byte)'D', (byte)'3', (byte)'e', (byte)'Q', (byte)'g', (byte)'-']),
	("unclosed-shift", [(byte)'+', (byte)'A', (byte)'O', (byte)'k']),
	("dangling-plus", [(byte)'+']),
	("invalid-base64", [(byte)'+', (byte)'!', (byte)'A', (byte)'-']),
	("incomplete-unit", [(byte)'+', (byte)'A', (byte)'-']),
	("invalid-surrogate", [(byte)'+', (byte)'2', (byte)'A', (byte)'A', (byte)'-']),
	("high-byte", [0xFF]),
})
{
	try
	{
		Console.WriteLine($"UTF7_READ|{item.Name}|{Convert.ToHexString(Encoding.UTF8.GetBytes(utf7.GetString(item.Bytes)))}");
	}
	catch (Exception error)
	{
		Console.WriteLine($"UTF7_READ|{item.Name}|ERROR:{error.GetType().Name}");
	}
}
try
{
	Encoding customUtf7 = (Encoding)utf7.Clone();
	customUtf7.EncoderFallback = new EncoderReplacementFallback("<encode>");
	customUtf7.DecoderFallback = new DecoderReplacementFallback("<decode>");
	virtualFileSystem.File.WriteAllText("/utf7-custom.txt", "Aé🙂", customUtf7);
	Console.WriteLine($"VFS_UTF7_CUSTOM_WRITE|{Convert.ToHexString(virtualFileSystem.File.ReadAllBytes("/utf7-custom.txt"))}");
	string customUtf7Path = Path.Combine(scratch, "utf7-custom.txt");
	fileSystem.File.WriteAllText(customUtf7Path, "Aé🙂", customUtf7);
	Console.WriteLine($"LOCAL_UTF7_CUSTOM_WRITE|{Convert.ToHexString(File.ReadAllBytes(customUtf7Path))}");
	foreach ((string Name, byte[] Bytes) item in new (string, byte[])[]
	{
		("invalid-base64", [(byte)'+', (byte)'!', (byte)'A', (byte)'-']),
		("dangling-plus", [(byte)'+']),
		("incomplete-unit", [(byte)'+', (byte)'A', (byte)'-']),
		("invalid-surrogate", [(byte)'+', (byte)'2', (byte)'A', (byte)'A', (byte)'-']),
		("high-byte", [0xFF]),
	})
	{
		Console.WriteLine($"UTF7_CUSTOM_READ|{item.Name}|{Convert.ToHexString(Encoding.UTF8.GetBytes(customUtf7.GetString(item.Bytes)))}");
	}
}
catch (Exception error)
{
	Console.WriteLine($"UTF7_CUSTOM_BAD|ERROR:{error.GetType().Name}");
}

Encoding customUtf8 = Encoding.GetEncoding(
	65001,
	new EncoderReplacementFallback("<encode>"),
	new DecoderReplacementFallback("<decode>"));
virtualFileSystem.File.WriteAllBytes("/custom-utf8.bin", [0xC3, 0x28, 0xFF]);
Console.WriteLine($"VFS_TEXT|custom-utf8={Convert.ToHexString(Encoding.UTF8.GetBytes(virtualFileSystem.File.ReadAllText("/custom-utf8.bin", customUtf8)))}");
virtualFileSystem.File.WriteAllBytes("/utf8-bom.bin", [0xEF, 0xBB, 0xBF, 0x41]);
Console.WriteLine($"VFS_TEXT|utf8-bom={Convert.ToHexString(Encoding.UTF8.GetBytes(virtualFileSystem.File.ReadAllText("/utf8-bom.bin", Encoding.UTF8)))}");
string localCustomUtf8Path = Path.Combine(scratch, "custom-utf8.bin");
File.WriteAllBytes(localCustomUtf8Path, [0xC3, 0x28, 0xFF]);
Console.WriteLine($"LOCAL_TEXT|custom-utf8={Convert.ToHexString(Encoding.UTF8.GetBytes(fileSystem.File.ReadAllText(localCustomUtf8Path, customUtf8)))}");
string localCustomUtf8BomPath = Path.Combine(scratch, "custom-utf8-bom.bin");
File.WriteAllBytes(localCustomUtf8BomPath, [0xEF, 0xBB, 0xBF, 0xC3, 0x28, 0xFF]);
Console.WriteLine($"LOCAL_TEXT|custom-utf8-bom={Convert.ToHexString(Encoding.UTF8.GetBytes(fileSystem.File.ReadAllText(localCustomUtf8BomPath, customUtf8)))}");
Encoding customAscii = Encoding.GetEncoding(
	20127,
	new EncoderReplacementFallback("<encode>"),
	new DecoderReplacementFallback("<decode>"));
Console.WriteLine($"LOCAL_TEXT|custom-ascii-utf8-bom={Convert.ToHexString(Encoding.UTF8.GetBytes(fileSystem.File.ReadAllText(localCustomUtf8BomPath, customAscii)))}");
virtualFileSystem.File.WriteAllText("/custom-ascii.txt", "Aé🙂", Encoding.GetEncoding(20127,
	new EncoderReplacementFallback("<encode>"), new DecoderReplacementFallback("<decode>")));
Console.WriteLine($"VFS_TEXT|custom-ascii-write={Convert.ToHexString(virtualFileSystem.File.ReadAllBytes("/custom-ascii.txt"))}");
string localCustomAsciiPath = Path.Combine(scratch, "custom-ascii.txt");
fileSystem.File.WriteAllText(localCustomAsciiPath, "Aé🙂", Encoding.GetEncoding(20127,
	new EncoderReplacementFallback("<encode>"), new DecoderReplacementFallback("<decode>")));
Console.WriteLine($"LOCAL_TEXT|custom-ascii-write={Convert.ToHexString(File.ReadAllBytes(localCustomAsciiPath))}");
virtualFileSystem.File.WriteAllText("/default-ascii.txt", "Aé🙂", Encoding.ASCII);
Console.WriteLine($"VFS_TEXT|default-ascii-write={Convert.ToHexString(virtualFileSystem.File.ReadAllBytes("/default-ascii.txt"))}");
string localDefaultAsciiPath = Path.Combine(scratch, "default-ascii.txt");
fileSystem.File.WriteAllText(localDefaultAsciiPath, "Aé🙂", Encoding.ASCII);
Console.WriteLine($"LOCAL_TEXT|default-ascii-write={Convert.ToHexString(File.ReadAllBytes(localDefaultAsciiPath))}");

Encoding throwingAscii = Encoding.GetEncoding(20127, EncoderFallback.ExceptionFallback, DecoderFallback.ReplacementFallback);
virtualFileSystem.File.WriteAllBytes("/throwing-ascii.txt", "old"u8);
try
{
	virtualFileSystem.File.WriteAllText("/throwing-ascii.txt", "Aé", throwingAscii);
	Console.WriteLine("VFS_TEXT|throwing-ascii-write=no-error");
}
catch (Exception error)
{
	Console.WriteLine($"VFS_TEXT|throwing-ascii-write={error.GetType().Name}|{Convert.ToHexString(virtualFileSystem.File.ReadAllBytes("/throwing-ascii.txt"))}");
}
string localThrowingAsciiPath = Path.Combine(scratch, "throwing-ascii.txt");
File.WriteAllBytes(localThrowingAsciiPath, "old"u8);
try
{
	fileSystem.File.WriteAllText(localThrowingAsciiPath, "Aé", throwingAscii);
	Console.WriteLine("LOCAL_TEXT|throwing-ascii-write=no-error");
}
catch (Exception error)
{
	Console.WriteLine($"LOCAL_TEXT|throwing-ascii-write={error.GetType().Name}|{Convert.ToHexString(File.ReadAllBytes(localThrowingAsciiPath))}");
}

Encoding throwingUtf8 = Encoding.GetEncoding(65001, EncoderFallback.ExceptionFallback, DecoderFallback.ExceptionFallback);
virtualFileSystem.File.WriteAllBytes("/throwing-utf8.bin", [0xFF]);
try
{
	virtualFileSystem.File.ReadAllText("/throwing-utf8.bin", throwingUtf8);
	Console.WriteLine("VFS_TEXT|throwing-utf8-read=no-error");
}
catch (Exception error)
{
	Console.WriteLine($"VFS_TEXT|throwing-utf8-read={error.GetType().Name}");
}
string localThrowingUtf8Path = Path.Combine(scratch, "throwing-utf8.bin");
File.WriteAllBytes(localThrowingUtf8Path, [0xFF]);
try
{
	fileSystem.File.ReadAllText(localThrowingUtf8Path, throwingUtf8);
	Console.WriteLine("LOCAL_TEXT|throwing-utf8-read=no-error");
}
catch (Exception error)
{
	Console.WriteLine($"LOCAL_TEXT|throwing-utf8-read={error.GetType().Name}");
}

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
