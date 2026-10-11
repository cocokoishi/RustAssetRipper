param(
    [Parameter(Mandatory = $true)][string]$SourceProject,
    [Parameter(Mandatory = $true)][string]$RustProject,
    [Parameter(Mandatory = $true)][string]$SourcePrefab,
    [Parameter(Mandatory = $true)][string]$RustPrefab,
    [Parameter(Mandatory = $true)][string]$OutputReport
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Read-GuidPaths([string]$ProjectRoot) {
    $projectRootFull = [System.IO.Path]::GetFullPath($ProjectRoot).TrimEnd('\')
    $result = [System.Collections.Generic.Dictionary[string,string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    $duplicates = 0
    foreach ($meta in Get-ChildItem -LiteralPath $ProjectRoot -Recurse -File -Filter '*.meta') {
        $contents = [System.IO.File]::ReadAllText($meta.FullName)
        $match = [regex]::Match($contents, '(?m)^guid:\s*([0-9a-fA-F]{32})\s*$')
        if (-not $match.Success) { continue }
        $guid = $match.Groups[1].Value
        $path = $meta.FullName.Substring($projectRootFull.Length).TrimStart('\').Replace('\', '/')
        $path = $path.Substring(0, $path.Length - 5)
        if ($result.ContainsKey($guid)) {
            if ($result[$guid] -ne $path) { $duplicates++ }
        } else {
            $result.Add($guid, $path)
        }
    }
    return [pscustomobject]@{ Map = $result; DuplicateGuidPaths = $duplicates }
}

function Read-YamlDocuments([string]$Path) {
    $text = [System.IO.File]::ReadAllText($Path)
    $headers = [regex]::Matches($text, '(?m)^--- !u!(?<class>\d+) &(?<fileid>-?\d+)(?:\s+[^\r\n]*)?\r?\n')
    $documents = [System.Collections.Generic.Dictionary[string,object]]::new([System.StringComparer]::Ordinal)
    for ($i = 0; $i -lt $headers.Count; $i++) {
        $header = $headers[$i]
        $end = if ($i + 1 -lt $headers.Count) { $headers[$i + 1].Index } else { $text.Length }
        $key = "$($header.Groups['class'].Value)|$($header.Groups['fileid'].Value)"
        if ($documents.ContainsKey($key)) { throw "Duplicate YAML identity $key in $Path" }
        $documents.Add($key, [pscustomobject]@{
            ClassId = [int]$header.Groups['class'].Value
            FileId = $header.Groups['fileid'].Value
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

function Get-FieldValues([string]$Document, [string]$FieldName) {
    $matches = [regex]::Matches($Document, "(?m)^\s*$([regex]::Escape($FieldName)):\s*(?<value>[^#\r\n]*)")
    return @($matches | ForEach-Object { $_.Groups['value'].Value.Trim() })
}

$sourcePath = Join-Path $SourceProject $SourcePrefab
$rustPath = Join-Path $RustProject $RustPrefab
$sourceDocuments = Read-YamlDocuments $sourcePath
$rustDocuments = Read-YamlDocuments $rustPath
$sourceGuidResult = Read-GuidPaths $SourceProject
$rustGuidResult = Read-GuidPaths $RustProject
$sourceGuids = $sourceGuidResult.Map
$rustGuids = $rustGuidResult.Map
$classes = [System.Collections.Generic.SortedDictionary[int,object]]::new()
$allKeys = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal)
foreach ($key in $sourceDocuments.Keys) { [void]$allKeys.Add($key) }
foreach ($key in $rustDocuments.Keys) { [void]$allKeys.Add($key) }
foreach ($key in $allKeys) {
    $sourceHas = $sourceDocuments.ContainsKey($key)
    $rustHas = $rustDocuments.ContainsKey($key)
    $classId = if ($sourceHas) { $sourceDocuments[$key].ClassId } else { $rustDocuments[$key].ClassId }
    if (-not $classes.ContainsKey($classId)) {
        $classes.Add($classId, [ordered]@{
            ClassId = $classId; Source = 0; Rust = 0; MatchedByClassAndFileID = 0
            ExactAfterGUIDPathMap = 0; Different = 0; SourceOnly = 0; RustOnly = 0
            TangentDocumentsSource = 0; TangentDocumentsRust = 0
            TangentValuesSource = 0; TangentValuesRust = 0; TangentMismatchedDocuments = 0
        })
    }
    $row = $classes[$classId]
    if ($sourceHas) { $row.Source++ }
    if ($rustHas) { $row.Rust++ }
    if ($sourceHas -and $rustHas) {
        $row.MatchedByClassAndFileID++
        $sourceText = $sourceDocuments[$key].Text
        $rustText = $rustDocuments[$key].Text
        $sourceNormalized = Normalize-Guids $sourceText $sourceGuids
        $rustNormalized = Normalize-Guids $rustText $rustGuids
        if ($sourceNormalized -ceq $rustNormalized) { $row.ExactAfterGUIDPathMap++ } else { $row.Different++ }
        $sourceTangents = @(Get-FieldValues $sourceText 'tangentMode')
        $rustTangents = @(Get-FieldValues $rustText 'tangentMode')
        $row.TangentValuesSource += $sourceTangents.Count
        $row.TangentValuesRust += $rustTangents.Count
        if ($sourceTangents.Count -gt 0) { $row.TangentDocumentsSource++ }
        if ($rustTangents.Count -gt 0) { $row.TangentDocumentsRust++ }
        if (($sourceTangents -join "`n") -cne ($rustTangents -join "`n")) { $row.TangentMismatchedDocuments++ }
    } elseif ($sourceHas) { $row.SourceOnly++ } else { $row.RustOnly++ }
}
$report = [ordered]@{
    SourcePrefab = $SourcePrefab; RustPrefab = $RustPrefab
    SourceDocuments = $sourceDocuments.Count; RustDocuments = $rustDocuments.Count
    MatchedDocuments = @($allKeys | Where-Object { $sourceDocuments.ContainsKey($_) -and $rustDocuments.ContainsKey($_) }).Count
    SourceDuplicateGuidPaths = $sourceGuidResult.DuplicateGuidPaths
    RustDuplicateGuidPaths = $rustGuidResult.DuplicateGuidPaths
    Classes = @($classes.Values)
}
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $OutputReport -Encoding utf8
$report | ConvertTo-Json -Depth 8
