using AssetRipper.IO.Files;
using AssetRipper.IO.Files.Streams;

var scratchRoot = Path.GetFullPath(Path.Combine(Environment.CurrentDirectory, "target", "local-filesystem-probe"));
Directory.CreateDirectory(scratchRoot);
var root = Path.Combine(scratchRoot, Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(root);

var writePath = Path.Combine(root, "write-while-open.bin");
File.WriteAllBytes(writePath, "source-bytes"u8.ToArray());
using (var stream = LocalFileSystem.Instance.File.OpenRead(writePath))
{
    try
    {
        File.WriteAllBytes(writePath, "replacement"u8.ToArray());
        Console.WriteLine("WRITE_WHILE_OPEN|ALLOWED");
    }
    catch (Exception error)
    {
        Console.WriteLine($"WRITE_WHILE_OPEN|REJECTED|{error.GetType().Name}");
    }

    stream.Position = 0;
    using var output = new MemoryStream();
    stream.CopyTo(output);
    Console.WriteLine($"READ_AFTER_WRITE_ATTEMPT|{Convert.ToHexString(output.ToArray())}");
}

var deletePath = Path.Combine(root, "delete-while-open.bin");
File.WriteAllBytes(deletePath, "delete-bytes"u8.ToArray());
using (LocalFileSystem.Instance.File.OpenRead(deletePath))
{
    try
    {
        File.Delete(deletePath);
        Console.WriteLine("DELETE_WHILE_OPEN|ALLOWED");
    }
    catch (Exception error)
    {
        Console.WriteLine($"DELETE_WHILE_OPEN|REJECTED|{error.GetType().Name}");
    }
}

var splitBase = Path.Combine(root, "split.bundle");
var split0 = splitBase + ".split0";
var split1 = splitBase + ".split1";
File.WriteAllBytes(split0, "split-left:"u8.ToArray());
File.WriteAllBytes(split1, "split-right"u8.ToArray());
using (var stream = MultiFileStream.OpenRead(splitBase, LocalFileSystem.Instance))
{
    try
    {
        File.WriteAllBytes(split0, "replacement"u8.ToArray());
        Console.WriteLine("WRITE_SPLIT_WHILE_OPEN|ALLOWED");
    }
    catch (Exception error)
    {
        Console.WriteLine($"WRITE_SPLIT_WHILE_OPEN|REJECTED|{error.GetType().Name}");
    }

    try
    {
        File.Delete(split1);
        Console.WriteLine("DELETE_SPLIT_WHILE_OPEN|ALLOWED");
    }
    catch (Exception error)
    {
        Console.WriteLine($"DELETE_SPLIT_WHILE_OPEN|REJECTED|{error.GetType().Name}");
    }

    using var output = new MemoryStream();
    stream.CopyTo(output);
    Console.WriteLine($"SPLIT_READ_AFTER_MUTATION_ATTEMPTS|{Convert.ToHexString(output.ToArray())}");
}

foreach (var path in Directory.EnumerateFiles(root))
{
    File.Delete(path);
}
Directory.Delete(root);
