using System.Text;
using AssetRipper.IO.Files;
using AssetRipper.IO.Files.Streams;

var scratchRoot = Path.GetFullPath(Path.Combine(Environment.CurrentDirectory, "target", "multifile-probe"));
Directory.CreateDirectory(scratchRoot);
var root = Path.Combine(scratchRoot, Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(root);
{
    var fileSystem = LocalFileSystem.Instance;
    var regularPath = Path.Combine(root, "regular.bundle");
    File.WriteAllBytes(regularPath, "regular-data"u8.ToArray());
    DumpFile("REGULAR", SchemeReader.LoadFile(regularPath, fileSystem));

    var splitBase = Path.Combine(root, "stream.bundle");
    File.WriteAllBytes(splitBase + ".split0", "alpha:"u8.ToArray());
    File.WriteAllBytes(splitBase + ".split1", "beta"u8.ToArray());
    foreach (var (label, path) in new[]
    {
        ("SPLIT_BASE", splitBase),
        ("SPLIT_PART", splitBase + ".split1"),
    })
    {
        DumpFile(label, SchemeReader.LoadFile(path, fileSystem));
    }

    foreach (var (label, path) in new[]
    {
        ("plain", "stream.bundle"),
        ("split0", "stream.bundle.split0"),
        ("split007", "stream.bundle.split007"),
        ("split-plus", "stream.bundle.split+1"),
        ("split-upper", "stream.bundle.SPLIT0"),
        ("split-no-index", "stream.bundle.split"),
    })
    {
        Console.WriteLine($"IS_MULTI|{label}|{MultiFileStream.IsMultiFile(path)}");
    }
    Console.WriteLine($"LOGICAL_NAMES|{Path.GetFileName(MultiFileStream.GetFilePath(splitBase + ".split1"))}|{MultiFileStream.GetFileName(splitBase + ".split1")}");

    using (var stream = MultiFileStream.OpenRead(splitBase, fileSystem))
    {
        var length = stream.Length;
        stream.Seek(5, SeekOrigin.Begin);
        var boundaryBuffer = new byte[2];
        var firstBoundaryRead = stream.Read(boundaryBuffer, 0, boundaryBuffer.Length);
        var firstBoundaryBytes = Convert.ToHexString(boundaryBuffer[..firstBoundaryRead]);
        var secondBoundaryRead = stream.Read(boundaryBuffer, 0, boundaryBuffer.Length);
        var secondBoundaryBytes = Convert.ToHexString(boundaryBuffer[..secondBoundaryRead]);
        Console.WriteLine($"BOUNDARY|{firstBoundaryRead}|{firstBoundaryBytes}|{secondBoundaryRead}|{secondBoundaryBytes}|{stream.Position}");

        var endZero = stream.Seek(0, SeekOrigin.End);
        var endNegativeTwo = stream.Seek(-2, SeekOrigin.End);
        var endPositiveTwo = stream.Seek(2, SeekOrigin.End);
        stream.Seek(0, SeekOrigin.Begin);
        var bytes = ReadToEnd(stream);
        Console.WriteLine($"SEEK_END|{length}|{endZero}|{endNegativeTwo}|{endPositiveTwo}");
        Console.WriteLine($"JOINED|{Convert.ToHexString(bytes)}");
    }

    var orderedBase = Path.Combine(root, "ordered.bundle");
    for (var i = 0; i <= 10; i++)
    {
        File.WriteAllBytes($"{orderedBase}.split{i}", Encoding.ASCII.GetBytes(((char)('A' + i)).ToString()));
    }
    using (var ordered = MultiFileStream.OpenRead(orderedBase, fileSystem))
    {
        Console.WriteLine($"NUMERIC_ORDER|{Encoding.ASCII.GetString(ReadToEnd(ordered))}");
    }

    var gapBase = Path.Combine(root, "gap.bundle");
    File.WriteAllBytes(gapBase + ".split0", "first"u8.ToArray());
    File.WriteAllBytes(gapBase + ".split2", "third"u8.ToArray());
    DumpRejected("GAP", gapBase, fileSystem, "gap.bundle.split1");

    var paddedBase = Path.Combine(root, "padded.bundle");
    File.WriteAllBytes(paddedBase + ".split00", "zero"u8.ToArray());
    File.WriteAllBytes(paddedBase + ".split1", "one"u8.ToArray());
    DumpRejected("ZERO_PADDED_INDEX", paddedBase, fileSystem, "padded.bundle.split0");
}

static byte[] ReadToEnd(Stream stream)
{
    using var output = new MemoryStream();
    stream.CopyTo(output);
    return output.ToArray();
}

static void DumpFile(string label, FileBase file)
{
    using (file)
    {
        Console.WriteLine($"{label}|{Path.GetFileName(file.FilePath)}|{file.Name}|{Convert.ToHexString(file.ToByteArray())}");
    }
}

static void DumpRejected(string label, string path, FileSystem fileSystem, string expectedPathFragment)
{
    try
    {
        using var file = SchemeReader.LoadFile(path, fileSystem);
        Console.WriteLine($"{label}|ACCEPTED|{file.Name}|{Convert.ToHexString(file.ToByteArray())}");
    }
    catch (Exception error)
    {
        Console.WriteLine($"{label}|REJECTED|{error.GetType().Name}|{error.Message.Contains(expectedPathFragment, StringComparison.OrdinalIgnoreCase)}");
    }
}
