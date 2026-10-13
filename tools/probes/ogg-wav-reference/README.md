# Ogg-to-WAV reference fixture

`Program.cs` creates a deterministic 8 kHz mono sine-wave Vorbis stream and passes it
through the baseline `AudioConverter.OggToWav` writer sequence:
`VorbisWaveReader` followed by `WaveFileWriter.WriteWavFileToStream`.

Run it with fresh output paths:

```powershell
dotnet run --project tools/probes/ogg-wav-reference/Reference.csproj -- target/audio-ogg-reference.ogg target/audio-wav-reference.wav
```

The project uses NAudio.Core 2.0.0 because NAudio.Vorbis 1.5.0 declares that dependency.
The frozen AssetRipper project resolves NAudio.Core 3.0.1 alongside NAudio.Vorbis 1.5.0;
that exact pair failed to load `VorbisWaveReader.Read` in the local reference probe. This
fixture verifies the source method's WAV representation with its compatible package pair;
it does not claim that the frozen application's mismatched pair runs successfully or that
Unity imports the generated WAV.
