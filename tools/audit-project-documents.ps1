<#
.SYNOPSIS
    Breadth audit of two Unity project trees by stable document identity.

.DESCRIPTION
    Pairs files by their path relative to the first `Assets/` segment, chunks
    YAML files into documents keyed by `classID|localFileID`, and compares each
    pair byte-exact first and then with GUIDs resolved through each project's
    own `.meta` files to relative asset paths. GUID normalization is only the
    proven mapping (each side's GUID independently resolved through its own
    `.meta` files); unmapped GUIDs stay explicit. Array order, curve data, and
    all other bytes are compared intact. Non-YAML files are hash-compared only
    and reported separately, never silently passed. Importer .meta files and
    files outside Assets (including ProjectSettings/Packages) are compared too.
    This is a diagnostic inventory unless RequireEquivalent is supplied; a
    successful diagnostic invocation does not establish product acceptance.

.PARAMETER SourceProject
    Frozen-reference project root (contains an `Assets/` directory).

.PARAMETER RustProject
    Rust-exported project root (contains an `Assets/` directory).

.PARAMETER ReportPath
    Output JSON report path (parent directory is created).
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$SourceProject,
    [Parameter(Mandatory = $true)][string]$RustProject,
    [Parameter(Mandatory = $true)][string]$ReportPath,
    [switch]$RequireEquivalent
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Get-RelativeAssetPath([string]$Root, [string]$FullPath) {
    $rootFull = [System.IO.Path]::GetFullPath($Root).TrimEnd('\', '/')
    $relative = $FullPath.Substring($rootFull.Length).TrimStart('\', '/').Replace('\', '/')
    $marker = 'Assets/'
    $index = $relative.IndexOf($marker, [System.StringComparison]::OrdinalIgnoreCase)
    if ($index -lt 0) { return $null }
    return $relative.Substring($index)
}

function Read-GuidPaths([string]$ProjectRoot) {
    $projectRootFull = [System.IO.Path]::GetFullPath($ProjectRoot).TrimEnd('\', '/')
    $result = [System.Collections.Generic.Dictionary[string,string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    $duplicates = 0
    foreach ($meta in Get-ChildItem -LiteralPath $ProjectRoot -Recurse -File -Filter '*.meta') {
        $contents = [System.IO.File]::ReadAllText($meta.FullName)
        $match = [regex]::Match($contents, '(?m)^guid:\s*([0-9a-fA-F]{32})\s*$')
        if (-not $match.Success) { continue }
        $guid = $match.Groups[1].Value
        $path = $meta.FullName.Substring($projectRootFull.Length).TrimStart('\', '/').Replace('\', '/')
        $path = $path.Substring(0, $path.Length - 5)
        if ($result.ContainsKey($guid)) {
            if ($result[$guid] -ne $path) { $duplicates++ }
        } else {
            $result.Add($guid, $path)
        }
    }
    return [pscustomobject]@{ Map = $result; DuplicateGuidPaths = $duplicates }
}

function Try-Read-YamlDocuments([string]$Path) {
    $text = [System.IO.File]::ReadAllText($Path)
    if ($text.Contains("`0")) { return $null }
    $headers = [regex]::Matches($text, '(?m)^--- !u!(?<class>\d+) &(?<fileid>-?\d+)(?:\s+[^\r\n]*)?\r?\n')
    if ($headers.Count -eq 0) { return $null }
    $documents = [System.Collections.Generic.Dictionary[string,object]]::new([System.StringComparer]::Ordinal)
    $fileIds = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal)
    for ($i = 0; $i -lt $headers.Count; $i++) {
        $header = $headers[$i]
        $end = if ($i + 1 -lt $headers.Count) { $headers[$i + 1].Index } else { $text.Length }
        $key = "$($header.Groups['class'].Value)|$($header.Groups['fileid'].Value)"
        if (-not $fileIds.Add($header.Groups['fileid'].Value)) { throw "Duplicate YAML local fileID in $Path : $key" }
        if ($documents.ContainsKey($key)) { throw "Duplicate YAML identity $key in $Path" }
        $documents.Add($key, [pscustomobject]@{
            ClassId = [int]$header.Groups['class'].Value
            Text = $text.Substring($header.Index, $end - $header.Index)
        })
    }
    return $documents
}

function Normalize-Guids([string]$Document, $GuidMap) {
    return [regex]::Replace($Document, '(?i)(?<prefix>\bguid:\s*)(?<guid>[0-9a-f]{32})', [System.Text.RegularExpressions.MatchEvaluator]{
        param($match)
        $guid = $match.Groups['guid'].Value
        if ($GuidMap.ContainsKey($guid)) {
            return $match.Groups['prefix'].Value + 'ASSET_PATH:' + $GuidMap[$guid]
        }
        return $match.Groups['prefix'].Value + 'UNMAPPED_GUID:' + $guid.ToLowerInvariant()
    })
}

function Normalize-Meta([string]$Document, $GuidMap) {
    $mapped = Normalize-Guids $Document $GuidMap
    # Frozen Meta.cs emits CurrentTick (export-time Unix seconds), not an
    # input/importer setting. Preserve the field and normalize only its numeric
    # root value. Importer fields, mainObjectFileID and array order stay exact.
    return [regex]::Replace($mapped, '(?m)^(?<key>timeCreated:\s*)[0-9]+(?=\r?$)', '${key}EXPORT_CREATION_TIME')
}

function Get-FileHashSha256([string]$Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $sha = [System.Security.Cryptography.SHA256]::Create()
        try {
            return ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-', '')
        } finally { $sha.Dispose() }
    } finally { $stream.Close() }
}

$sourceGuids = Read-GuidPaths $SourceProject
$rustGuids = Read-GuidPaths $RustProject

function Index-Tree([string]$Root) {
    $entries = @{}
    foreach ($file in Get-ChildItem -LiteralPath $Root -Recurse -File) {
        if ($file.Name -like '*.meta') { continue }
        $rel = Get-RelativeAssetPath $Root $file.FullName
        if ($null -eq $rel) { continue }
        $entries[$rel] = $file.FullName
    }
    return $entries
}

$sourceFiles = Index-Tree $SourceProject
$rustFiles = Index-Tree $RustProject
$allPaths = [System.Collections.Generic.SortedSet[string]]::new([System.StringComparer]::Ordinal)
foreach ($p in $sourceFiles.Keys) { [void]$allPaths.Add($p) }
foreach ($p in $rustFiles.Keys) { [void]$allPaths.Add($p) }

$classes = [System.Collections.Generic.SortedDictionary[int,object]]::new()
$pairedFiles = 0
$opaqueCompared = 0
$opaqueIdentical = 0
$onlySourcePaths = [System.Collections.Generic.List[string]]::new()
$onlyRustPaths = [System.Collections.Generic.List[string]]::new()
$opaqueDifferent = [System.Collections.Generic.List[string]]::new()

function Get-ClassRow([int]$ClassId) {
    if (-not $classes.ContainsKey($ClassId)) {
        $classes.Add($ClassId, [ordered]@{
            ClassId = $ClassId; Source = 0; Rust = 0; MatchedByClassAndFileID = 0
            Exact = 0; ExactAfterGUIDPathMap = 0; Different = 0
            SourceOnly = 0; RustOnly = 0; SampleDifferentKeys = @()
        })
    }
    return $classes[$ClassId]
}

function Count-UnpairedDocuments([string]$Path, [bool]$IsSource) {
    $docs = Try-Read-YamlDocuments $Path
    if ($null -eq $docs) { return }
    foreach ($document in $docs.Values) {
        $row = Get-ClassRow $document.ClassId
        if ($IsSource) { $row.Source++; $row.SourceOnly++ }
        else { $row.Rust++; $row.RustOnly++ }
    }
}

foreach ($rel in $allPaths) {
    $inSource = $sourceFiles.ContainsKey($rel)
    $inRust = $rustFiles.ContainsKey($rel)
    if (-not $inSource) { $onlyRustPaths.Add($rel); Count-UnpairedDocuments $rustFiles[$rel] $false; continue }
    if (-not $inRust) { $onlySourcePaths.Add($rel); Count-UnpairedDocuments $sourceFiles[$rel] $true; continue }
    $pairedFiles++
    $sourceDocuments = Try-Read-YamlDocuments $sourceFiles[$rel]
    $rustDocuments = Try-Read-YamlDocuments $rustFiles[$rel]
    if ($null -eq $sourceDocuments -or $null -eq $rustDocuments) {
        $opaqueCompared++
        if ((Get-FileHashSha256 $sourceFiles[$rel]) -ceq (Get-FileHashSha256 $rustFiles[$rel])) {
            $opaqueIdentical++
        } else {
            $opaqueDifferent.Add($rel)
        }
        continue
    }
    $keys = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal)
    foreach ($k in $sourceDocuments.Keys) { [void]$keys.Add($k) }
    foreach ($k in $rustDocuments.Keys) { [void]$keys.Add($k) }
    foreach ($key in $keys) {
        $sourceHas = $sourceDocuments.ContainsKey($key)
        $rustHas = $rustDocuments.ContainsKey($key)
        $classId = if ($sourceHas) { $sourceDocuments[$key].ClassId } else { $rustDocuments[$key].ClassId }
        $row = Get-ClassRow $classId
        if ($sourceHas) { $row.Source++ }
        if ($rustHas) { $row.Rust++ }
        if ($sourceHas -and $rustHas) {
            $row.MatchedByClassAndFileID++
            if ($sourceDocuments[$key].Text -ceq $rustDocuments[$key].Text) {
                $row.Exact++
            } else {
                $sourceNormalized = Normalize-Guids $sourceDocuments[$key].Text $sourceGuids.Map
                $rustNormalized = Normalize-Guids $rustDocuments[$key].Text $rustGuids.Map
                if ($sourceNormalized -ceq $rustNormalized) { $row.ExactAfterGUIDPathMap++ }
                else {
                    $row.Different++
                    if ($row.SampleDifferentKeys.Count -lt 5) {
                        $row.SampleDifferentKeys += @("$rel::$key")
                    }
                }
            }
        } elseif ($sourceHas) { $row.SourceOnly++ } else { $row.RustOnly++ }
    }
}

# The original asset-document inventory omitted importer and project data.
# Compare these separately to retain the meaning of its existing counters.
function Index-AdditionalFiles([string]$Root) {
    $rootFull = [System.IO.Path]::GetFullPath($Root).TrimEnd('\', '/')
    $entries = @{}
    foreach ($file in Get-ChildItem -LiteralPath $Root -Recurse -File) {
        $rel = $file.FullName.Substring($rootFull.Length).TrimStart('\', '/').Replace('\', '/')
        if ($file.Name -like '*.meta' -or -not $rel.StartsWith('Assets/', [StringComparison]::Ordinal)) {
            $entries[$rel] = $file.FullName
        }
    }
    return $entries
}
$sourceAdditional = Index-AdditionalFiles $SourceProject
$rustAdditional = Index-AdditionalFiles $RustProject
$additionalPaths = [System.Collections.Generic.SortedSet[string]]::new([System.StringComparer]::Ordinal)
foreach ($path in $sourceAdditional.Keys) { [void]$additionalPaths.Add($path) }
foreach ($path in $rustAdditional.Keys) { [void]$additionalPaths.Add($path) }
$additionalResults = [System.Collections.Generic.List[object]]::new()
foreach ($path in $additionalPaths) {
    $result = 'DIFFERENT'
    if (-not $sourceAdditional.ContainsKey($path)) { $result = 'RUST_ONLY' }
    elseif (-not $rustAdditional.ContainsKey($path)) { $result = 'SOURCE_ONLY' }
    elseif ((Get-FileHashSha256 $sourceAdditional[$path]) -ceq (Get-FileHashSha256 $rustAdditional[$path])) { $result = 'EXACT' }
    elseif ($path.EndsWith('.meta', [StringComparison]::OrdinalIgnoreCase)) {
        $sourceText = [System.IO.File]::ReadAllText($sourceAdditional[$path])
        $rustText = [System.IO.File]::ReadAllText($rustAdditional[$path])
        if ((Normalize-Meta $sourceText $sourceGuids.Map) -ceq (Normalize-Meta $rustText $rustGuids.Map)) {
            $result = 'GUID_PATH_AND_EXPORT_TIME_EQUIVALENT'
        }
    }
    $additionalResults.Add([pscustomobject]@{
        Path = $path; Kind = $(if ($path.EndsWith('.meta', [StringComparison]::OrdinalIgnoreCase)) { 'META' } else { 'PROJECT_OR_AUXILIARY' })
        Result = $result
    })
}
$additionalDifferences = @($additionalResults | Where-Object { $_.Result -notin @('EXACT', 'GUID_PATH_AND_EXPORT_TIME_EQUIVALENT') })
$documentDifferences = 0
foreach ($row in $classes.Values) { $documentDifferences += $row.Different + $row.SourceOnly + $row.RustOnly }
$equivalent = ($documentDifferences -eq 0 -and $onlySourcePaths.Count -eq 0 -and $onlyRustPaths.Count -eq 0 -and
    $opaqueDifferent.Count -eq 0 -and $additionalDifferences.Count -eq 0 -and
    $sourceGuids.DuplicateGuidPaths -eq 0 -and $rustGuids.DuplicateGuidPaths -eq 0)

$report = [ordered]@{
    SchemaVersion = 2
    Scope = 'All project files; asset document identities, meta importer content, project/auxiliary bytes'
    EquivalentWithinComparisonScope = $equivalent
    AcceptanceGateRequested = [bool]$RequireEquivalent
    Status = $(if ($equivalent) { 'EQUIVALENT_WITHIN_COMPARISON_SCOPE' } else { 'DIFFERENCES_REQUIRE_INVESTIGATION' })
    SourceProject = $SourceProject; RustProject = $RustProject
    PairedFiles = $pairedFiles
    OnlySourcePaths = @($onlySourcePaths); OnlyRustPaths = @($onlyRustPaths)
    OpaqueCompared = $opaqueCompared; OpaqueIdentical = $opaqueIdentical
    OpaqueDifferent = @($opaqueDifferent)
    SourceDuplicateGuidPaths = $sourceGuids.DuplicateGuidPaths
    RustDuplicateGuidPaths = $rustGuids.DuplicateGuidPaths
    Classes = @($classes.Values)
    AdditionalFiles = @($additionalResults.ToArray())
    AdditionalDifferences = $additionalDifferences
    MetadataNormalization = @('GUIDs resolved independently through each project meta owner',
        'Root timeCreated numeric value only: frozen Meta.cs CurrentTick records export time; field presence is preserved')
    ComparisonLimitations = @('GUID normalization proves path ownership only, not imported subasset IDs',
        'Different local fileIDs are reported; no unsupported identity remapping is guessed',
        'Opaque byte differences require independent pixel/packet/resource evidence',
        'Unity import/runtime and source-input semantic completeness are outside this comparison')
}
$reportDirectory = [System.IO.Path]::GetDirectoryName([System.IO.Path]::GetFullPath($ReportPath))
if (-not [System.IO.Directory]::Exists($reportDirectory)) {
    [System.IO.Directory]::CreateDirectory($reportDirectory) | Out-Null
}
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $ReportPath -Encoding utf8
$summary = foreach ($row in $classes.Values) {
    "class $($row.ClassId): src=$($row.Source) rust=$($row.Rust) matched=$($row.MatchedByClassAndFileID) exact=$($row.Exact) semGUID=$($row.ExactAfterGUIDPathMap) diff=$($row.Different) srcOnly=$($row.SourceOnly) rustOnly=$($row.RustOnly)"
}
$summary
"pairedFiles=$pairedFiles onlySource=$($onlySourcePaths.Count) onlyRust=$($onlyRustPaths.Count) opaque=$opaqueCompared identical=$opaqueIdentical different=$($opaqueDifferent.Count)"
"additionalFiles=$($additionalResults.Count) additionalDifferences=$($additionalDifferences.Count) equivalentWithinComparisonScope=$equivalent"
if ($RequireEquivalent -and -not $equivalent) { exit 1 }
