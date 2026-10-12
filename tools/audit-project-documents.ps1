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
    and reported separately, never silently passed.

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
    [Parameter(Mandatory = $true)][string]$ReportPath
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
    for ($i = 0; $i -lt $headers.Count; $i++) {
        $header = $headers[$i]
        $end = if ($i + 1 -lt $headers.Count) { $headers[$i + 1].Index } else { $text.Length }
        $key = "$($header.Groups['class'].Value)|$($header.Groups['fileid'].Value)"
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

foreach ($rel in $allPaths) {
    $inSource = $sourceFiles.ContainsKey($rel)
    $inRust = $rustFiles.ContainsKey($rel)
    if (-not $inSource) { $onlyRustPaths.Add($rel); continue }
    if (-not $inRust) { $onlySourcePaths.Add($rel); continue }
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

$report = [ordered]@{
    SourceProject = $SourceProject; RustProject = $RustProject
    PairedFiles = $pairedFiles
    OnlySourcePaths = @($onlySourcePaths); OnlyRustPaths = @($onlyRustPaths)
    OpaqueCompared = $opaqueCompared; OpaqueIdentical = $opaqueIdentical
    OpaqueDifferent = @($opaqueDifferent)
    SourceDuplicateGuidPaths = $sourceGuids.DuplicateGuidPaths
    RustDuplicateGuidPaths = $rustGuids.DuplicateGuidPaths
    Classes = @($classes.Values)
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
