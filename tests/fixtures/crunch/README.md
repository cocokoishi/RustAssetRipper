# Crunch differential fixtures

These CRN files come from commit
`2b1f1f2f45595058f9523578c8924c2cb14d2127` of
[`UniversalGameExtraction/texture2ddecoder`](https://github.com/UniversalGameExtraction/texture2ddecoder).
That is the exact source revision recorded by the packaged `texture2ddecoder
0.1.2` dependency. The upstream test README says the textures are encodings of
its repositioned `月照琉璃.png`, derived from the original Ferris artwork at
[`rustacean.net`](https://rustacean.net/). The original Ferris artwork is
dedicated to the public domain under CC0 1.0.

The Rust tests compare complete decoded RGBA output against hashes produced by
this repository's bundled arm64 `Texture2DDecoderNative` C++ oracle. The
fixtures cover classic Crunch DXT1/DXT5 and UnityCrunch DXT1/DXT5/ETC1/ETC2A.

`multi_face_multimip_source.dds` is a synthetic 8x8 DDS cubemap with six
single-color faces. `multi_face_multimip.crn` was generated from it with the
public-domain Crunch v1.04 x64 encoder from
[`BinomialLLC/crunch`](https://github.com/BinomialLLC/crunch/tree/36479bc697be19168daafbf15f47f3c60ccec004):

```text
crunch_x64.exe -file multi_face_multimip_source.dds -out multi_face_multimip.crn -fileformat crn -dxt1 -mipmode Generate -maxmips 3 -quality 255 -quiet
```

The source DDS SHA-256 is `D46C54DD103C80FAADF3A6119C23CE6B54AE4733DD09E63A967541A3A9C70509`;
the CRN SHA-256 is `CE3E74438AB9EA066A103C7A3A7996FA52CEF0412A1AD638A67DA3B6D09F00BD`.
Crunch reports 8x8, three mip levels, six faces, and DXT1. The frozen
AssetRipper classic decoder returns 288 bytes (`E321A17C7CFBA23F` SHA-256
prefix); UnityCrunch throws on this pre-2017.3 DXT1 input. The Rust production
conversion regression pins the complete RGBA output hash of each face against
the reference encoder's `-split` output.
