using NAudio.Vorbis;
using NAudio.Wave;

namespace OggVorbisEncoder.Example;

static class Program
{
    private static readonly int WriteBufferSize = 512;

    static void Main(string[] args)
    {
        if (args.Length != 2) throw new ArgumentException("usage: Reference <output.ogg> <output.wav>");
        var ogg = GenerateSine(8000, 1, 440, 0.1f);
        File.WriteAllBytes(args[0], ogg);
        using var reader = new VorbisWaveReader(File.OpenRead(args[0]), true);
        using (var output = new FileStream(args[1], FileMode.CreateNew, FileAccess.Write, FileShare.None))
            WaveFileWriter.WriteWavFileToStream(output, reader);
        Console.WriteLine($"ogg_bytes={ogg.Length} sample_rate={reader.WaveFormat.SampleRate} channels={reader.WaveFormat.Channels} bits={reader.WaveFormat.BitsPerSample} encoding={reader.WaveFormat.Encoding} wav_bytes={new FileInfo(args[1]).Length}");
    }

    private static byte[] GenerateSine(int sampleRate, int channels, int frequency, float durationSeconds)
    {
        var frameCount = (int)(durationSeconds * sampleRate);
        var samples = new float[channels][];
        for (var channel = 0; channel < channels; channel++) samples[channel] = new float[frameCount];
        for (var frame = 0; frame < frameCount; frame++)
        {
            var sample = 0.2f * MathF.Sin(frame / (float)sampleRate * MathF.PI * 2.0f * frequency);
            for (var channel = 0; channel < channels; channel++) samples[channel][frame] = sample;
        }
        return GenerateFile(samples, sampleRate, channels);
    }

    private static byte[] GenerateFile(float[][] samples, int sampleRate, int channels)
    {
        using var output = new MemoryStream();
        var info = VorbisInfo.InitVariableBitRate(channels, sampleRate, 0.5f);
        var stream = new OggStream(1);
        var comments = new Comments();
        var header = HeaderPacketBuilder.BuildInfoPacket(info);
        var comment = HeaderPacketBuilder.BuildCommentsPacket(comments);
        var books = HeaderPacketBuilder.BuildBooksPacket(info);
        stream.PacketIn(header);
        stream.PacketIn(comment);
        stream.PacketIn(books);
        FlushPages(stream, output, true);
        var state = ProcessingState.Create(info);
        for (var start = 0; start < samples[0].Length; start += WriteBufferSize)
        {
            var count = Math.Min(WriteBufferSize, samples[0].Length - start);
            state.WriteData(samples, count, start);
            while (!stream.Finished && state.PacketOut(out OggPacket packet))
            {
                stream.PacketIn(packet);
                FlushPages(stream, output, false);
            }
        }
        state.WriteEndOfStream();
        while (!stream.Finished && state.PacketOut(out OggPacket packet))
        {
            stream.PacketIn(packet);
            FlushPages(stream, output, false);
        }
        FlushPages(stream, output, true);
        return output.ToArray();
    }

    private static void FlushPages(OggStream stream, Stream output, bool force)
    {
        while (stream.PageOut(out OggPage page, force))
        {
            output.Write(page.Header, 0, page.Header.Length);
            output.Write(page.Body, 0, page.Body.Length);
        }
    }
}
