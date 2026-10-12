# Referential-integrity scan for an exported Unity project: every
# `{fileID, guid, type}` PPtr must resolve to an existing YAML document
# (same file path plus fileID), a `.meta` guid owner without YAML documents
# (scripts, shader text, binary media), or an engine builtin family.
#
# Null PPtrs and engine builtins (`000...000`, `000...E000`, `000...F000`)
# are skipped by design. Scene files (`.unity`) carry no addressable YAML
# documents — references to scenes resolve by GUID (their fileIDs such as
# 102900000 are conventional, proven identical on the frozen side) — so guid
# ownership is the whole check there, as for scripts and binary media.
#
# Usage (PowerShell 5.1-safe; use short junctions for 260+ char paths):
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools/check-project-references.ps1 `
#     -ProjectRoot <exported project dir>
param([string]$ProjectRoot)
$files = Get-ChildItem $ProjectRoot -Recurse -File | Where-Object { $_.Name -notlike "*.meta" }
$guidOwner = @{}
foreach ($m in Get-ChildItem $ProjectRoot -Recurse -Filter "*.meta" -File) {
    $c = [System.IO.File]::ReadAllText($m.FullName)
    $g = [regex]::Match($c, "(?m)^guid:\s*([0-9a-fA-F]{32})\s*$")
    if ($g.Success) {
        $rel = $m.FullName.Substring($ProjectRoot.Length).TrimStart('\', '/').Replace('\', '/')
        $guidOwner[$g.Groups[1].Value.ToLowerInvariant()] = $rel.Substring(0, $rel.Length - 5)
    }
}
$docIndex = @{}
foreach ($f in $files) {
    $rel = $f.FullName.Substring($ProjectRoot.Length).TrimStart('\', '/').Replace('\', '/')
    $t = [System.IO.File]::ReadAllText($f.FullName)
    foreach ($h in [regex]::Matches($t, "(?m)^--- !u!(\d+) &(-?\d+)\r?$")) {
        $docIndex[$rel + "|" + $h.Groups[2].Value] = $true
    }
}
$dangling = 0; $checked = 0; $samples = @()
foreach ($f in $files) {
    $rel = $f.FullName.Substring($ProjectRoot.Length).TrimStart('\', '/').Replace('\', '/')
    $t = [System.IO.File]::ReadAllText($f.FullName)
    foreach ($m in [regex]::Matches($t, "\{fileID:\s*(-?\d+),\s*guid:\s*([0-9a-fA-F]{32}),\s*type:\s*(\d+)\}")) {
        $fid = $m.Groups[1].Value; $guid = $m.Groups[2].Value.ToLowerInvariant()
        if ($fid -eq "0" -and $guid -eq "00000000000000000000000000000000") { continue }
        if ($guid -match "^0{16}[ef]0{15}$") { continue } # engine builtin families
        $checked++
        if (-not $guidOwner.ContainsKey($guid)) {
            $dangling++; if ($samples.Count -lt 10) { $samples += ($rel + " -> UNMAPPED " + $guid) }
            continue
        }
        $target = $guidOwner[$guid]
        # Script (.cs), shader-text, scene, and binary media targets carry no
        # addressable YAML documents; guid ownership is the whole check. Other
        # targets must contain the referenced fileID.
        if ($target -match "\.(cs|shader|compute|cginc|hlsl|unity|png|jpg|jpeg|tga|bmp|exr|tif|tiff|psd|otf|ttf|ttc|fnt|ogg|wav|mp3|mp4|mov|fbx|glb|gltf|bytes|txt|json|xml|dll)$") { continue }
        if (-not $docIndex.ContainsKey($target + "|" + $fid)) {
            $dangling++; if ($samples.Count -lt 10) { $samples += ($rel + " -> " + $target + " missing fileID " + $fid) }
        }
    }
}
Write-Output ("checked=" + $checked + " dangling=" + $dangling)
$samples
