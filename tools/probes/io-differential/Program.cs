// Isolated original-runtime differential probe for the import/IO/container/compression lane.
//
// Runs against the shipped AssetRipper.IO.Files assembly from the frozen reference build
// (0Bins/AssetRipper.GUI.Free/Release) and writes one fixture that Rust replays through its
// production SchemeReader / container / compression / stream code.
//
// Sections
//   SCHEME     top-level SchemeReader.LoadFile dispatch, recursive entry inventory, and errors
//   STREAM     RandomAccessStream (internal, reflection), MultiFileStream, PartialStream,
//              SmartStream observable semantics, including parent-cursor effects
//   RESOURCE   Bundle.ResolveResource lookup outcomes over a real resource hierarchy
//
// Usage: io-differential <output-path>

using System.Reflection;
using System.Text;
using AssetRipper.IO.Files;
using AssetRipper.IO.Files.BundleFiles;
using AssetRipper.IO.Files.BundleFiles.FileStream;
using AssetRipper.IO.Files.CompressedFiles;
using AssetRipper.IO.Files.CompressedFiles.GZip;
using AssetRipper.IO.Files.ResourceFiles;
using AssetRipper.IO.Files.SerializedFiles;
using AssetRipper.IO.Files.Streams;
using AssetRipper.IO.Files.Streams.Smart;
using AssetRipper.IO.Files.WebFiles;
using K4os.Compression.LZ4;
using ZstdSharp;

public static class Program
{
	private static readonly List<string> Lines = new();
	private static string WorkDirectory = "";

	public static int Main(string[] args)
	{
		string output = args.Length > 0 ? args[0] : "io_differential_fixture.txt";
		WorkDirectory = Path.Combine(
			Directory.GetCurrentDirectory(), "target", "io-scratch", "probe-" + Guid.NewGuid().ToString("N"));
		Directory.CreateDirectory(WorkDirectory);
		if (Path.GetDirectoryName(Path.GetFullPath(output)) is { Length: > 0 } outputDirectory)
		{
			Directory.CreateDirectory(outputDirectory);
		}

		Lines.Add("# IO/container/compression/stream reference-runtime differential");
		Lines.Add("# Probe: tools/probes/io-differential (.NET SDK 10.0.401) against the shipped");
		Lines.Add("#   AssetRipper.IO.Files.dll from 0Bins/AssetRipper.GUI.Free/Release of the frozen reference.");
		Lines.Add("# SCHEME rows: SchemeReader.LoadFile + ReadContentsRecursively over probe-built container bytes;");
		Lines.Add("#   the trailing field is the recursive inventory (CONTAINER/CODEC/SER/RES/FAIL).");
		Lines.Add("# STREAM rows: observable stream semantics; each field is name=value, pipe separated.");
		Lines.Add("# RESOURCE rows: Bundle.ResolveResource outcomes over a real in-memory bundle hierarchy.");
		Lines.Add("# Format: SECTION|case|field=value|...");

		try
		{
			EmitSchemeSection();
			EmitStreamSection();
			EmitDirectorySection();
			EmitResourceSection();
		}
		finally
		{
			try { Directory.Delete(WorkDirectory, true); } catch { }
		}

		File.WriteAllLines(output, Lines);
		Console.WriteLine($"wrote {Lines.Count} lines to {output}");
		return 0;
	}

	// ---------------------------------------------------------------- byte writers

	private static void Wu16Be(List<byte> b, ushort v) => Be(b, BitConverter.GetBytes(v));
	private static void Wu32Be(List<byte> b, uint v) => Be(b, BitConverter.GetBytes(v));
	private static void Wi32Be(List<byte> b, int v) => Be(b, BitConverter.GetBytes(v));
	private static void Wi64Be(List<byte> b, long v) => Be(b, BitConverter.GetBytes(v));

	private static void Be(List<byte> b, byte[] value)
	{
		if (BitConverter.IsLittleEndian)
		{
			Array.Reverse(value);
		}
		b.AddRange(value);
	}

	private static void Str0(List<byte> b, string s)
	{
		b.AddRange(Encoding.UTF8.GetBytes(s));
		b.Add(0);
	}

	private static void Align(List<byte> b, int alignment)
	{
		while (b.Count % alignment != 0)
		{
			b.Add(0);
		}
	}

	private static string Hex(byte[] data) => Convert.ToHexString(data);

	private static byte[] Ascii(string s) => Encoding.ASCII.GetBytes(s);

	// ---------------------------------------------------------------- UnityFS builder

	private sealed record Entry(string Path, byte[] Data);

	/// <summary>
	/// Builds a full UnityFS bundle. `metaCompression` and `blockCompression` are raw
	/// CompressionType discriminants: 0 None, 1 Lzma, 2 Lz4, 3 Lz4HC, 4 Lzham, 5 unknown.
	/// </summary>
	private static byte[] BuildUnityFs(
		int version,
		byte metaCompression,
		byte blockCompression,
		bool blocksInfoAtEnd,
		IReadOnlyList<Entry> entries)
		=> BuildUnityFs(
			version,
			metaCompression,
			new[] { blockCompression },
			blocksInfoAtEnd,
			needsPaddingAtStart: false,
			directoryCombined: true,
			entries);

	private static byte[] BuildUnityFs(
		int version,
		byte metaCompression,
		IReadOnlyList<byte> blockCompressions,
		bool blocksInfoAtEnd,
		bool needsPaddingAtStart,
		bool directoryCombined,
		IReadOnlyList<Entry> entries,
		IReadOnlyList<int>? blockSplitOffsets = null)
	{
		if (blockCompressions.Count == 0)
		{
			throw new ArgumentException("UnityFS fixture must contain at least one storage block", nameof(blockCompressions));
		}

		List<byte> dataBlob = new();
		List<byte> directory = new();
		Wu32Be(directory, (uint)entries.Count);
		foreach (Entry entry in entries)
		{
			Wi64Be(directory, dataBlob.Count);
			Wi64Be(directory, entry.Data.Length);
			Wi32Be(directory, 4); // NodeFlags.SerializedFile
			Str0(directory, entry.Path);
			dataBlob.AddRange(entry.Data);
		}

		List<int> blockBoundaries = new() { 0 };
		if (blockSplitOffsets is not null)
		{
			blockBoundaries.AddRange(blockSplitOffsets);
		}
		blockBoundaries.Add(dataBlob.Count);
		if (blockBoundaries.Count != blockCompressions.Count + 1
			|| blockBoundaries.Zip(blockBoundaries.Skip(1), (start, end) => start <= end).Any(valid => !valid))
		{
			throw new ArgumentException("UnityFS fixture block boundaries do not match the storage-block count", nameof(blockSplitOffsets));
		}

		List<byte[]> compressedBlocks = new(blockCompressions.Count);
		List<byte> metadata = new();
		metadata.AddRange(new byte[16]); // BlocksInfo.UncompressedDataHash
		Wu32Be(metadata, (uint)blockCompressions.Count);
		for (int i = 0; i < blockCompressions.Count; i++)
		{
			int start = blockBoundaries[i];
			int length = blockBoundaries[i + 1] - start;
			byte compression = blockCompressions[i];
			byte[] rawBlock = dataBlob.GetRange(start, length).ToArray();
			byte[] compressedBlock = CompressBlock(rawBlock, compression);
			compressedBlocks.Add(compressedBlock);
			Wu32Be(metadata, (uint)rawBlock.Length);
			Wu32Be(metadata, (uint)compressedBlock.Length);
			Wu16Be(metadata, compression);
		}
		if (directoryCombined)
		{
			metadata.AddRange(directory);
		}
		byte[] uncompressedMetadata = metadata.ToArray();
		byte[] metadataPayload = CompressMetadata(uncompressedMetadata, metaCompression);

		const uint combined = 0x40;
		const uint atEnd = 0x80;
		const uint needsPadding = 0x200;
		uint flags = (uint)(metaCompression & 0x3F)
			| (directoryCombined ? combined : 0)
			| (blocksInfoAtEnd ? atEnd : 0)
			| (needsPaddingAtStart ? needsPadding : 0);

		List<byte> header = new();
		Str0(header, "UnityFS");
		Wi32Be(header, version);
		Str0(header, "2022.3.1f1");
		Str0(header, "2022.3.1f1");
		Wi64Be(header, 0); // Size placeholder
		Wi32Be(header, metadataPayload.Length);
		Wi32Be(header, uncompressedMetadata.Length);
		Wi32Be(header, unchecked((int)flags));

		List<byte> file = new(header);
		if (version >= 7)
		{
			Align(file, 16);
		}
		void AppendBlocks()
		{
			foreach (byte[] compressedBlock in compressedBlocks)
			{
				file.AddRange(compressedBlock);
			}
		}

		if (blocksInfoAtEnd)
		{
			if (needsPaddingAtStart)
			{
				Align(file, 16);
			}
			AppendBlocks();
			long size = file.Count + metadataPayload.Length;
			file.AddRange(metadataPayload);
			PatchSize(file, size);
		}
		else
		{
			file.AddRange(metadataPayload);
			if (needsPaddingAtStart)
			{
				Align(file, 16);
			}
			AppendBlocks();
			PatchSize(file, file.Count);
		}
		return file.ToArray();
	}

	private static void PatchSize(List<byte> file, long size)
	{
		// "UnityFS\0" (8) + int32 version (4) + two zero-terminated strings, then the int64 Size.
		int offset = 8 + 4 + Encoding.UTF8.GetByteCount("2022.3.1f1") + 1
			+ Encoding.UTF8.GetByteCount("2022.3.1f1") + 1;
		byte[] value = BitConverter.GetBytes(size);
		if (BitConverter.IsLittleEndian)
		{
			Array.Reverse(value);
		}
		for (int i = 0; i < 8; i++)
		{
			file[offset + i] = value[i];
		}
	}

	private static byte[] CompressMetadata(byte[] metadata, byte metaCompression)
	{
		switch (metaCompression)
		{
			case 0:
				return metadata;
			case 1:
				{
					using MemoryStream output = new();
					LzmaCompression.CompressLzmaStream(new MemoryStream(metadata), metadata.Length, output);
					return output.ToArray();
				}
			case 2:
			case 3:
				return Lz4Encode(metadata);
			default:
				// An unknown metadata compression still needs bytes in place; the reference
				// fails on the discriminant before touching them.
				return metadata;
		}
	}

	private static byte[] CompressBlock(byte[] data, byte compression)
	{
		switch (compression)
		{
			case 0:
			case 4: // Lzham is rejected by the reference decoder
				return data;
			case 1:
				{
					using MemoryStream output = new();
					LzmaCompression.CompressLzmaStream(new MemoryStream(data), data.Length, output);
					return output.ToArray();
				}
			case 2:
			case 3:
				return Lz4Encode(data);
			case 5:
				using (Compressor compressor = new(3))
				{
					return compressor.Wrap(data).ToArray();
				}
			default:
				return data;
		}
	}

	private static byte[] Lz4Encode(byte[] data)
	{
		byte[] output = new byte[LZ4Codec.MaximumOutputSize(data.Length)];
		int written = LZ4Codec.Encode(data, output, LZ4Level.L00_FAST);
		Array.Resize(ref output, written);
		return output;
	}

	// ---------------------------------------------------------------- Raw/Web builder

	private static byte[] BuildRawWeb(
		bool web,
		int version,
		IReadOnlyList<Entry> entries,
		bool multipleWebScenes = false)
	{
		List<byte> rebuilt = new();
		Wu32Be(rebuilt, (uint)entries.Count);
		foreach (Entry entry in entries)
		{
			Str0(rebuilt, entry.Path);
			Wi32Be(rebuilt, 0); // patched once the directory size is known
			Wi32Be(rebuilt, entry.Data.Length);
		}
		Align(rebuilt, 4);
		int directorySize = rebuilt.Count;
		int cursor = directorySize;
		List<byte> payload = new();
		// Patch each node offset now that the aligned directory size is known.
		int scan = 4;
		foreach (Entry entry in entries)
		{
			scan += Encoding.UTF8.GetByteCount(entry.Path) + 1;
			PatchI32(rebuilt, scan, cursor);
			scan += 8;
			cursor += entry.Data.Length;
			payload.AddRange(entry.Data);
		}
		List<byte> inner = new(rebuilt);
		inner.AddRange(payload);

		List<byte> head = new();
		Str0(head, web ? "UnityWeb" : "UnityRaw");
		Wi32Be(head, version);
		Str0(head, "2022.3.1f1");
		Str0(head, "2022.3.1f1");
		bool hasHash = version >= 4;
		bool hasCompleteFileSize = version >= 2;
		bool hasUncompressedBlocksInfoSize = version >= 3;
		if (hasHash)
		{
			head.AddRange(new byte[16]);
			Wu32Be(head, 0x12345678);
		}
		Wu32Be(head, (uint)inner.Count); // MinimumStreamedBytes
		int headerSizePosition = head.Count;
		Wi32Be(head, 0); // HeaderSize placeholder
		Wi32Be(head, 1); // NumberOfScenesToDownloadBeforeStreaming
		if (web)
		{
			using MemoryStream compressed = new();
			LzmaCompression.CompressLzmaSizeStream(new MemoryStream(inner.ToArray()), inner.Count, compressed);
			byte[] chunk = compressed.ToArray();
			Wu32Be(head, multipleWebScenes ? 2u : 1u); // scene count
			if (multipleWebScenes)
			{
				Wu32Be(head, 0); // empty preceding scene
				Wu32Be(head, 0);
			}
			Wu32Be(head, (uint)chunk.Length);
			Wu32Be(head, (uint)inner.Count);
			if (hasCompleteFileSize)
			{
				Wu32Be(head, 0);
			}
			if (hasUncompressedBlocksInfoSize)
			{
				Wu32Be(head, (uint)directorySize);
			}
			Align(head, 4);
			PatchI32(head, headerSizePosition, head.Count);
			head.AddRange(chunk);
			return head.ToArray();
		}
		else
		{
			Wu32Be(head, 0); // scene count
			if (hasCompleteFileSize)
			{
				Wu32Be(head, 0);
			}
			if (hasUncompressedBlocksInfoSize)
			{
				Wu32Be(head, (uint)directorySize);
			}
			Align(head, 4);
			PatchI32(head, headerSizePosition, head.Count);
			head.AddRange(inner);
			return head.ToArray();
		}
	}

	private static void PatchI32(List<byte> file, int offset, int value)
	{
		byte[] bytes = BitConverter.GetBytes(value);
		if (BitConverter.IsLittleEndian)
		{
			Array.Reverse(bytes);
		}
		for (int i = 0; i < 4; i++)
		{
			file[offset + i] = bytes[i];
		}
	}

	private static byte[] BuildWebData(IReadOnlyList<Entry> entries)
	{
		WebFile file = new();
		foreach (Entry entry in entries)
		{
			file.AddResourceFile(new ResourceFile(entry.Data, "mem", entry.Path));
		}
		return file.ToByteArray();
	}

	private static byte[] BuildArchive()
	{
		List<byte> file = new();
		Str0(file, "UnityArchive");
		Wi32Be(file, 6);
		Str0(file, "2022.3.1f1");
		Str0(file, "2022.3.1f1");
		while (file.Count < 0x40)
		{
			file.Add(0);
		}
		return file.ToArray();
	}

	// ---------------------------------------------------------------- SCHEME section

	private static void EmitSchemeSection()
	{
		byte[] ufsNone = BuildUnityFs(6, 0, 0, false, new[] { new Entry("data/file.bin", Ascii("0123456789")) });
		byte[] webData = BuildWebData(new[] { new Entry("inner.bin", Ascii("web!")), new Entry("sub/deep.bin", Ascii("deep")) });
		var blobs = new (string Name, byte[] Data)[]
		{
			("ufs_v1_none", BuildUnityFs(1, 0, 0, false, new[] { new Entry("data/file.bin", Ascii("0123456789")) })),
			("ufs_v6_none", ufsNone),
			("ufs_v6_lz4_meta", BuildUnityFs(6, 2, 0, false, new[] { new Entry("data/file.bin", Ascii("0123456789")) })),
			("ufs_v6_lz4hc_meta", BuildUnityFs(6, 3, 0, false, new[] { new Entry("data/file.bin", Ascii("0123456789")) })),
			("ufs_v6_lzma_meta", BuildUnityFs(6, 1, 0, false, new[] { new Entry("data/file.bin", Ascii("0123456789")) })),
			("ufs_v6_lzham_meta", BuildUnityFs(6, 4, 0, false, new[] { new Entry("data/file.bin", Ascii("0123456789")) })),
			("ufs_v6_unknown_meta", BuildUnityFs(6, 5, 0, false, new[] { new Entry("data/file.bin", Ascii("0123456789")) })),
			("ufs_v6_lzma_block", BuildUnityFs(6, 0, 1, false, new[] { new Entry("data/file.bin", Ascii("0123456789")) })),
			("ufs_v6_lz4_block", BuildUnityFs(6, 0, 2, false, new[] { new Entry("data/file.bin", Ascii("0123456789")) })),
			("ufs_v6_lz4hc_block", BuildUnityFs(6, 0, 3, false, new[] { new Entry("data/file.bin", Ascii("0123456789")) })),
			("ufs_v6_lzham_block", BuildUnityFs(
				6,
				0,
				new byte[] { 0, 4, 0 },
				blocksInfoAtEnd: false,
				needsPaddingAtStart: false,
				directoryCombined: true,
				new[]
				{
					new Entry("valid.bin", Ascii("GOOD")),
					new Entry("unsupported.bin", Ascii("BAD!")),
					new Entry("after.bin", Ascii("LATER")),
				},
				new[] { 4, 8 })),
			("ufs_v6_zstd_block", BuildUnityFs(6, 0, 5, false, new[] { new Entry("data/file.bin", Ascii("0123456789")) })),
			("ufs_v6_multiblock_cross_entry", BuildUnityFs(
				6,
				0,
				new byte[] { 2, 0 },
				blocksInfoAtEnd: false,
				needsPaddingAtStart: false,
				directoryCombined: true,
				new[]
				{
					new Entry("cross.bin", Ascii("ABCDEFGHIJ")),
					new Entry("tail.bin", Ascii("FGHIJ")),
				},
				new[] { 5 })),
			("ufs_v6_padding_at_start", BuildUnityFs(
				6,
				0,
				new byte[] { 0 },
				blocksInfoAtEnd: false,
				needsPaddingAtStart: true,
				directoryCombined: true,
				new[] { new Entry("data/file.bin", Ascii("0123456789")) })),
			("ufs_v6_uncombined_directory", BuildUnityFs(
				6,
				0,
				new byte[] { 0 },
				blocksInfoAtEnd: false,
				needsPaddingAtStart: false,
				directoryCombined: false,
				new[] { new Entry("not-listed.bin", Ascii("0123456789")) })),
			("ufs_v7_atend", BuildUnityFs(7, 0, 0, true, new[] { new Entry("data/file.bin", Ascii("0123456789")) })),
			("ufs_v8_atend_padded", BuildUnityFs(
				8,
				0,
				new byte[] { 0 },
				blocksInfoAtEnd: true,
				needsPaddingAtStart: true,
				directoryCombined: true,
				new[] { new Entry("data/file.bin", Ascii("0123456789")) })),
			("ufs_nested_webdata", BuildUnityFs(6, 0, 0, false, new[] { new Entry("inner.webdata", webData) })),
			("raw_v1", BuildRawWeb(false, 1, new[] { new Entry("a.bin", Ascii("0123456789")) })),
			("raw_v2", BuildRawWeb(false, 2, new[] { new Entry("a.bin", Ascii("0123456789")) })),
			("raw_v3", BuildRawWeb(false, 3, new[] { new Entry("a.bin", Ascii("0123456789")) })),
			("raw_v4", BuildRawWeb(false, 4, new[] { new Entry("a.bin", Ascii("0123456789")) })),
			("raw_v5", BuildRawWeb(false, 5, new[] { new Entry("a.bin", Ascii("0123456789")) })),
			("raw_v6", BuildRawWeb(false, 6, new[] { new Entry("a.bin", Ascii("0123456789")) })),
			("raw_v7", BuildRawWeb(false, 7, new[] { new Entry("a.bin", Ascii("0123456789")) })),
			("raw_v8", BuildRawWeb(false, 8, new[] { new Entry("a.bin", Ascii("0123456789")) })),
			("raw_nested_unityfs", BuildRawWeb(false, 4, new[] { new Entry("nested.bundle", ufsNone) })),
			("web_v1", BuildRawWeb(true, 1, new[] { new Entry("w.bin", Ascii("ABCDEFGHIJ")) })),
			("web_v2", BuildRawWeb(true, 2, new[] { new Entry("w.bin", Ascii("ABCDEFGHIJ")) })),
			("web_v3", BuildRawWeb(true, 3, new[] { new Entry("w.bin", Ascii("ABCDEFGHIJ")) })),
			("web_v4", BuildRawWeb(true, 4, new[] { new Entry("w.bin", Ascii("ABCDEFGHIJ")) })),
			("web_v5", BuildRawWeb(true, 5, new[] { new Entry("w.bin", Ascii("ABCDEFGHIJ")) })),
			("web_v6", BuildRawWeb(true, 6, new[] { new Entry("w.bin", Ascii("ABCDEFGHIJ")) })),
			("web_v6_last_scene", BuildRawWeb(true, 6, new[] { new Entry("w.bin", Ascii("ABCDEFGHIJ")) }, multipleWebScenes: true)),
			("web_v7", BuildRawWeb(true, 7, new[] { new Entry("w.bin", Ascii("ABCDEFGHIJ")) })),
			("web_v8", BuildRawWeb(true, 8, new[] { new Entry("w.bin", Ascii("ABCDEFGHIJ")) })),
			("web_nested_webdata", BuildRawWeb(true, 6, new[] { new Entry("nested.webdata", webData) })),
			("webdata", webData),
			("archive", BuildArchive()),
			("plain", Ascii("plain resource bytes")),
			("gzip_ufs", BuildGZip(ufsNone)),
		};

		foreach ((string name, byte[] data) in blobs)
		{
			string path = Path.Combine(WorkDirectory, name + ".bin");
			File.WriteAllBytes(path, data);
			try
			{
				FileBase file = SchemeReader.LoadFile(path, LocalFileSystem.Instance);
				file.ReadContentsRecursively();
				List<string> parts = new();
				Dump(file, parts, 0);
				Lines.Add($"SCHEME|{name}|in={Hex(data)}|type={file.GetType().Name}|fixed={file.NameFixed}"
					+ $"|tree={string.Join(",", parts)}");
			}
			catch (Exception ex)
			{
				Lines.Add($"SCHEME|{name}|in={Hex(data)}|THROW:{ex.GetType().Name}:{Sanitize(ex.Message)}");
			}
		}
	}

	private static byte[] BuildGZip(byte[] inner)
	{
		GZipFile file = new()
		{
			Name = "wrapped.bin",
			FilePath = "wrapped.bin",
			UncompressedFile = new ResourceFile(inner, "wrapped.bin", "wrapped.bin"),
		};
		return file.ToByteArray();
	}

	private static string Sanitize(string value) =>
		value.Replace("\r", " ").Replace("\n", " ").Replace("|", "/");

	private static void Dump(FileBase file, List<string> parts, int depth)
	{
		if (depth > 4)
		{
			parts.Add("DEPTH");
			return;
		}
		switch (file)
		{
			case CompressedFile compressed:
				parts.Add("CODEC:" + compressed.GetType().Name);
				if (compressed.UncompressedFile is not null)
				{
					Dump(compressed.UncompressedFile, parts, depth + 1);
				}
				break;
			case FileContainer container:
				parts.Add($"CONTAINER:{container.GetType().Name}:{container.NameFixed}"
					+ $":res={container.ResourceFiles.Count}:failed={container.FailedFiles.Count}");
				foreach (ResourceFile resource in container.ResourceFiles)
				{
					Dump(resource, parts, depth + 1);
				}
				foreach (SerializedFile serialized in container.SerializedFiles)
				{
					parts.Add("SER:" + serialized.NameFixed);
				}
				foreach (FileContainer child in container.FileLists)
				{
					Dump(child, parts, depth + 1);
				}
				foreach (FailedFile failed in container.FailedFiles)
				{
					parts.Add("FAIL:" + failed.NameFixed);
				}
				break;
			case ResourceFile resource:
				parts.Add("RES:" + resource.NameFixed + ":" + Hex(resource.ToByteArray()));
				break;
			default:
				parts.Add("OTHER:" + file.GetType().Name + ":" + file.NameFixed);
				break;
		}
	}

	// ---------------------------------------------------------------- STREAM section

	private static void EmitStreamSection()
	{
		EmitRandomAccessSection();
		EmitMultiFileSection();
		EmitPartialSection();
		EmitSmartStreamSection();
	}

	private static Type RandomAccessStreamType =>
		typeof(SchemeReader).Assembly.GetType("AssetRipper.IO.Files.Streams.RandomAccessStream")
		?? throw new InvalidOperationException("RandomAccessStream type not found");

	private static Stream NewRandomAccessStream(FileStream parent, long offset, long length)
	{
		ConstructorInfo ctor = RandomAccessStreamType.GetConstructor(
			BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance,
			null,
			new[] { typeof(FileStream), typeof(long), typeof(long) },
			null) ?? throw new InvalidOperationException("RandomAccessStream ctor not found");
		return (Stream)ctor.Invoke(new object[] { parent, offset, length });
	}

	private static void EmitRandomAccessSection()
	{
		string path = Path.Combine(WorkDirectory, "random.bin");
		byte[] data = new byte[64];
		for (int i = 0; i < data.Length; i++)
		{
			data[i] = (byte)i;
		}
		File.WriteAllBytes(path, data);

		using FileStream parent = new(path, FileMode.Open, FileAccess.Read, FileShare.Read);
		parent.Position = 40;
		Stream first = NewRandomAccessStream(parent, 10, 20);
		Stream second = NewRandomAccessStream(parent, 30, 16);

		byte[] a = new byte[8];
		int readA = first.Read(a, 0, a.Length);
		long parentAfterFirstRead = parent.Position;
		byte[] b = new byte[8];
		int readB = second.Read(b, 0, b.Length);
		long parentAfterSecondRead = parent.Position;
		long firstPosition = first.Position;
		long secondPosition = second.Position;
		long firstLength = first.Length;
		long secondLength = second.Length;

		first.Seek(5, SeekOrigin.Begin);
		byte[] c = new byte[12];
		int readC = first.Read(c, 0, c.Length);
		long parentAfterSeekRead = parent.Position;

		// The parent handle's own cursor must be untouched by positional child reads.
		byte[] parentBytes = new byte[4];
		int readParent = parent.Read(parentBytes, 0, parentBytes.Length);
		long parentPositionAfterOwnRead = parent.Position;

		Lines.Add($"STREAM|R2|first_len={firstLength}|second_len={secondLength}"
			+ $"|read_a={readA}:{Hex(a)}|read_b={readB}:{Hex(b)}"
			+ $"|read_c={readC}:{Hex(c)}"
			+ $"|parent_position_after_child_reads={parentAfterFirstRead}/{parentAfterSecondRead}/{parentAfterSeekRead}"
			+ $"|first_position={firstPosition}|second_position={secondPosition}"
			+ $"|parent_read={readParent}:{Hex(parentBytes)}|parent_position_after_own_read={parentPositionAfterOwnRead}");

		// End-relative seek keeps the source's inverted offset.
		Stream third = NewRandomAccessStream(parent, 0, 20);
		long seekEnd0 = third.Seek(0, SeekOrigin.End);
		long seekEndMinus2 = third.Seek(-2, SeekOrigin.End);
		long seekEndPlus2 = third.Seek(2, SeekOrigin.End);
		Lines.Add($"STREAM|R3|seek_end0={seekEnd0}|seek_end_minus2={seekEndMinus2}|seek_end_plus2={seekEndPlus2}");
	}

	private static void EmitMultiFileSection()
	{
		string basePath = Path.Combine(WorkDirectory, "parts.bundle");
		byte[] part0 = new byte[10];
		byte[] part1 = new byte[6];
		for (int i = 0; i < part0.Length; i++)
		{
			part0[i] = (byte)i;
		}
		for (int i = 0; i < part1.Length; i++)
		{
			part1[i] = (byte)(10 + i);
		}
		File.WriteAllBytes(basePath + ".split0", part0);
		File.WriteAllBytes(basePath + ".split1", part1);

		using Stream stream = MultiFileStream.OpenRead(basePath, LocalFileSystem.Instance);
		long length = stream.Length;

		// A single Read never crosses a part boundary in the source.
		stream.Position = 4;
		byte[] first = new byte[12];
		int readFirst = stream.Read(first, 0, first.Length);
		long afterFirst = stream.Position;
		byte[] second = new byte[12];
		int readSecond = stream.Read(second, 0, second.Length);
		long afterSecond = stream.Position;
		byte[] third = new byte[12];
		int readThird = stream.Read(third, 0, third.Length);
		long afterThird = stream.Position;
		byte[] fourth = new byte[12];
		int readFourth = stream.Read(fourth, 0, fourth.Length);
		long afterFourth = stream.Position;

		// ReadExactly loops internally, so it does cross the boundary.
		stream.Position = 4;
		byte[] exact = new byte[12];
		stream.ReadExactly(exact, 0, exact.Length);

		stream.Position = 7;
		int singleByte = stream.ReadByte();
		long afterByte = stream.Position;

		Lines.Add($"STREAM|M2|len={length}"
			+ $"|r1={readFirst}:{Hex(first)}@{afterFirst}"
			+ $"|r2={readSecond}:{Hex(second)}@{afterSecond}"
			+ $"|r3={readThird}:{Hex(third)}@{afterThird}"
			+ $"|r4={readFourth}:{Hex(fourth)}@{afterFourth}"
			+ $"|exact12={Hex(exact)}|readbyte={singleByte}@{afterByte}");
	}

	private static void EmitPartialSection()
	{
		byte[] data = new byte[64];
		for (int i = 0; i < data.Length; i++)
		{
			data[i] = (byte)i;
		}
		using MemoryStream parent = new(data);
		PartialStream partial = new(parent, 10, 20);
		partial.Seek(0, SeekOrigin.Begin);
		byte[] buffer = new byte[64];
		int read = partial.Read(buffer, 0, buffer.Length);
		long position = partial.Position;
		partial.Seek(5, SeekOrigin.Begin);
		int byteValue = partial.ReadByte();
		byte[] tail = new byte[8];
		int tailRead = partial.Read(tail, 0, tail.Length);
		Lines.Add($"STREAM|P2|len={partial.Length}|read={read}:{Hex(buffer.AsSpan(0, Math.Max(read, 0)).ToArray())}"
			+ $"|position={position}|seek5_byte={byteValue}|tail={tailRead}:{Hex(tail.AsSpan(0, Math.Max(tailRead, 0)).ToArray())}");
	}

	private static void EmitSmartStreamSection()
	{
		using SmartStream memory = SmartStream.CreateMemory(new byte[] { 1, 2, 3, 4 });
		byte[] buffer = new byte[8];
		int read = memory.Read(buffer, 0, buffer.Length);
		Lines.Add($"STREAM|S4|len={memory.Length}|read={read}:{Hex(buffer.AsSpan(0, Math.Max(read, 0)).ToArray())}"
			+ $"|position={memory.Position}|type={memory.StreamType}");

		using SmartStream nullStream = SmartStream.CreateNull();
		string nullBehavior;
		try
		{
			nullStream.Read(buffer, 0, 1);
			nullBehavior = "NO_THROW";
		}
		catch (Exception ex)
		{
			nullBehavior = "THROW:" + ex.GetType().Name;
		}
		Lines.Add($"STREAM|S5|len={nullStream.Length}|read={nullBehavior}|isnull={nullStream.IsNull}");
	}

	// ---------------------------------------------------------------- DIRECTORY section

	private static void EmitDirectorySection()
	{
		string directory = Path.Combine(WorkDirectory, "dir");
		Directory.CreateDirectory(directory);
		// Creation order deliberately differs from name order so the dump exposes the
		// platform enumeration order instead of a name-sorted one.
		File.WriteAllText(Path.Combine(directory, "zeta.bundle"), "z");
		File.WriteAllText(Path.Combine(directory, "alpha.bundle"), "a");
		File.WriteAllText(Path.Combine(directory, "mid.bundle.split0"), "m0");
		File.WriteAllText(Path.Combine(directory, "mid.bundle.split1"), "m1");
		Directory.CreateDirectory(Path.Combine(directory, "sub"));
		File.WriteAllText(Path.Combine(directory, "sub", "nested.bundle"), "n");

		string[] files = LocalFileSystem.Instance.Directory.GetFiles(directory);
		string[] directories = LocalFileSystem.Instance.Directory.GetDirectories(directory);
		string[] splitFiles = MultiFileStream.GetFiles(
			Path.Combine(directory, "mid.bundle"), LocalFileSystem.Instance);
		bool isMultiFilePart = MultiFileStream.IsMultiFile(Path.Combine(directory, "mid.bundle.split1"));

		Lines.Add($"DIR|enumeration|get_files={JoinNames(files)}|get_directories={JoinNames(directories)}"
			+ $"|split_files={JoinNames(splitFiles)}|is_multi_part={isMultiFilePart}");

		using FileStream reader = LocalFileSystem.Instance.File.OpenRead(Path.Combine(directory, "alpha.bundle"));
		string writeOutcome;
		try
		{
			File.WriteAllText(Path.Combine(directory, "alpha.bundle"), "w");
			writeOutcome = "ALLOWED";
		}
		catch (Exception ex)
		{
			writeOutcome = "REJECTED:" + ex.GetType().Name;
		}
		string deleteOutcome;
		try
		{
			File.Delete(Path.Combine(directory, "alpha.bundle"));
			deleteOutcome = "ALLOWED";
		}
		catch (Exception ex)
		{
			deleteOutcome = "REJECTED:" + ex.GetType().Name;
		}
		byte[] retained = new byte[1];
		reader.Position = 0;
		int retainedRead = reader.Read(retained, 0, 1);
		Lines.Add($"DIR|retained|open_read_type={reader.GetType().FullName}"
			+ $"|write_while_open={writeOutcome}|delete_while_open={deleteOutcome}"
			+ $"|read_while_open={retainedRead}:{Hex(retained)}");

		Lines.Add("DIR|disposition|non_windows_arm=LocalFileSystem.g.cs delegates File.OpenRead and "
			+ "Directory.GetFiles to System.IO on every host, so the observed share-lock rejection is the "
			+ "Windows file-sharing rule, not AssetRipper logic; on Unix .NET does not take an advisory share "
			+ "lock for FileShare.Read, so a concurrent write or delete is not guaranteed to be blocked, and "
			+ "Directory.GetFiles order is OS-dependent; Rust narrows the share mode only on Windows "
			+ "(io/stream.rs open_os_read) and uses a plain File::open elsewhere, while sorting discovered "
			+ "directory entries for deterministic output.");
	}

	private static string JoinNames(IEnumerable<string> paths) =>
		string.Join(",", paths.Select(Path.GetFileName));

	// ---------------------------------------------------------------- RESOURCE section

	private static void EmitResourceSection()
	{
		try
		{
			EmitResourceBody();
		}
		catch (Exception ex)
		{
			Exception inner = ex is TargetInvocationException { InnerException: { } nested } ? nested : ex;
			Lines.Add($"RESOURCE|probe|THROW:{inner.GetType().Name}:{Sanitize(inner.Message)}");
		}
	}

	private static void EmitResourceBody()
	{
		Type gameBundleType = typeof(AssetRipper.Assets.Bundles.GameBundle);
		Type bundleType = typeof(AssetRipper.Assets.Bundles.Bundle);
		object game = Activator.CreateInstance(gameBundleType)!;

		// A GameBundle only accepts non-GameBundle children.
		object child = Activator.CreateInstance(
			typeof(AssetRipper.Assets.Bundles.ProcessedBundle), new object?[] { "child" })!;
		AddBundle(bundleType, game, child);

		ResourceFile sharedRess = new(Ascii("0123456789abcdef"), "mem", "archive:/textures/Shared.resS");
		ResourceFile siblingRess = new(Ascii("ABCDEFGH"), "mem", "sibling.resS");
		ResourceFile rootResource = new(Ascii("ROOTRESOURCE"), "mem", "root.resource");
		AddResource(bundleType, game, rootResource);
		AddResource(bundleType, child, sharedRess);
		AddResource(bundleType, child, siblingRess);

		foreach ((string label, object from, string query) in new (string, object, string)[]
		{
			("root_root", game, "root.resource"),
			("game_to_child_shared", game, "archive:/textures/Shared.resS"),
			("game_to_child_shared_fixed", game, "ARCHIVE:/REQUEST/SHARED.ress"),
			("game_to_child_sibling", game, "sibling.resS"),
			("child_own_shared", child, "archive:/textures/Shared.resS"),
			("child_parent_root", child, "root.resource"),
			("missing", game, "absent.resS"),
			("empty", game, ""),
			("wrong_extension_only", game, "archive:/textures/Shared.resS.resS"),
		})
		{
			MethodInfo resolve = bundleType.GetMethod("ResolveResource", new[] { typeof(string) })!;
			object? result = resolve.Invoke(from, new object?[] { query });
			FileBase? file = result as FileBase;
			string outcome = file is null ? "NULL" : "FILE:" + file.NameFixed;
			long length = file is ResourceFile rf ? rf.Stream.Length : -1;
			Lines.Add($"RESOURCE|lookup|case={label}|query={query}|outcome={outcome}|length={length}");
		}

		// Range read through the resolved ResourceFile's stream, exactly as
		// StreamedResourceExtensions.GetContent does it.
		object? resolved = bundleType.GetMethod("ResolveResource", new[] { typeof(string) })!
			.Invoke(game, new object?[] { "archive:/textures/Shared.resS" });
		if (resolved is ResourceFile resource)
		{
			foreach ((string label, long offset, int size) in new (string, long, int)[]
			{
				("head", 0, 4),
				("middle", 4, 6),
				("tail", 10, 6),
				("past_end", 12, 6),
			})
			{
				if (offset + size > resource.Stream.Length)
				{
					Lines.Add($"RESOURCE|range|case={label}|offset={offset}|size={size}|outcome=OUT_OF_RANGE");
					continue;
				}
				byte[] data = new byte[size];
				resource.Stream.Position = offset;
				resource.Stream.ReadExactly(data);
				Lines.Add($"RESOURCE|range|case={label}|offset={offset}|size={size}|outcome={Hex(data)}");
			}
		}
	}

	private static void AddBundle(Type bundleType, object parent, object child)
	{
		bundleType.GetMethod("AddBundle", new[] { bundleType })!.Invoke(parent, new[] { child });
	}

	private static void AddResource(Type bundleType, object bundle, ResourceFile resource)
	{
		bundleType.GetMethod("AddResource", new[] { typeof(ResourceFile) })!.Invoke(bundle, new object[] { resource });
	}
}
