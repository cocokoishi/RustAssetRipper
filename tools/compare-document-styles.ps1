param(
    [Parameter(Mandatory = $true)][string]$SourceRoot,
    [Parameter(Mandatory = $true)][string]$RustRoot,
    [Parameter(Mandatory = $true)][string]$PathListFile,
    [string]$ClassIdList = '',
    [int]$MaxReport = 8
)

$ErrorActionPreference = 'Stop'

# Classify the differences that the semantic audit could not resolve: for each
# paired document it decides whether the two sides are byte-identical, identical
# after resolving GUIDs through each project's own .meta map, identical modulo
# YAML whitespace / brace / comma style, or a real difference whose first
# differing tokens are printed. It reuses the audit's GUID ownership rule so the
# verdict is comparable with audit-project-documents.ps1.

function Read-GuidPaths([string]$ProjectRoot) {
    $projectRootFull = [System.IO.Path]::GetFullPath($ProjectRoot).TrimEnd('\', '/')
    $result = [System.Collections.Generic.Dictionary[string, string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    foreach ($meta in Get-ChildItem -LiteralPath $ProjectRoot -Recurse -File -Filter '*.meta') {
        $contents = [System.IO.File]::ReadAllText($meta.FullName)
        $match = [regex]::Match($contents, '(?m)^guid:\s*([0-9a-fA-F]{32})\s*$')
        if (-not $match.Success) { continue }
        $guid = $match.Groups[1].Value
        $path = $meta.FullName.Substring($projectRootFull.Length).TrimStart('\', '/').Replace('\', '/')
        $path = $path.Substring(0, $path.Length - 5)
        if (-not $result.ContainsKey($guid)) { $result.Add($guid, $path) }
    }
    return $result
}

function Try-Read-YamlDocuments([string]$Path) {
    $text = [System.IO.File]::ReadAllText($Path)
    if ($text.Contains("`0")) { return $null }
    $headers = [regex]::Matches($text, '(?m)^--- !u!(?<class>\d+) &(?<fileid>-?\d+)(?:\s+[^\r\n]*)?\r?\n')
    if ($headers.Count -eq 0) { return $null }
    $documents = [System.Collections.Generic.Dictionary[string, object]]::new([System.StringComparer]::Ordinal)
    for ($i = 0; $i -lt $headers.Count; $i++) {
        $header = $headers[$i]
        $end = if ($i + 1 -lt $headers.Count) { $headers[$i + 1].Index } else { $text.Length }
        $key = "$($header.Groups['class'].Value)|$($header.Groups['fileid'].Value)"
        $documents[$key] = [pscustomobject]@{
            ClassId = [int]$header.Groups['class'].Value
            Text    = $text.Substring($header.Index, $end - $header.Index)
        }
    }
    return $documents
}

function Normalize-Guids([string]$Document, $GuidMap) {
    return [regex]::Replace($Document, '(?i)(?<prefix>\bguid:\s*)(?<guid>[0-9a-f]{32})', [System.Text.RegularExpressions.MatchEvaluator] {
            param($match)
            $guid = $match.Groups['guid'].Value
            if ($GuidMap.ContainsKey($guid)) {
                return $match.Groups['prefix'].Value + 'ASSET_PATH:' + $GuidMap[$guid]
            }
            return $match.Groups['prefix'].Value + 'UNMAPPED_GUID:' + $guid.ToLowerInvariant()
        })
}

function Get-StyleTokens([string]$Document) {
    $tokens = New-Object System.Collections.Generic.List[string]
    foreach ($match in [regex]::Matches($Document, '\S+')) {
        $token = $match.Value.Trim([char[]]'{}[],')
        if ($token.Length -gt 0) { $tokens.Add($token) }
    }
    return $tokens
}

$sourceGuids = Read-GuidPaths $SourceRoot
$rustGuids = Read-GuidPaths $RustRoot
$sourceRootFull = [System.IO.Path]::GetFullPath($SourceRoot).TrimEnd('\', '/')
$rustRootFull = [System.IO.Path]::GetFullPath($RustRoot).TrimEnd('\', '/')
$classFilter = [System.Collections.Generic.HashSet[int]]::new()
foreach ($id in ($ClassIdList -split '[,;\s]+')) {
    $parsed = 0
    if ([int]::TryParse($id, [ref]$parsed)) { [void]$classFilter.Add($parsed) }
}

$exact = 0; $guidEquivalent = 0; $styleOnly = 0; $realDiff = 0; $missing = 0
$reported = 0
$paths = @()
$utf8 = New-Object System.Text.UTF8Encoding($false)
foreach ($line in [System.IO.File]::ReadAllLines($PathListFile, $utf8)) {
    $trimmed = $line.Trim()
    if ($trimmed.Length -gt 0) { $paths += $trimmed }
}

foreach ($rel in $paths) {
    $sourcePath = Join-Path $sourceRootFull $rel
    $rustPath = Join-Path $rustRootFull $rel
    if (-not [System.IO.File]::Exists($sourcePath)) { Write-Output "MISSING_SOURCE $rel"; $missing++; continue }
    if (-not [System.IO.File]::Exists($rustPath)) { Write-Output "MISSING_RUST $rel"; $missing++; continue }
    $sourceDocs = Try-Read-YamlDocuments $sourcePath
    $rustDocs = Try-Read-YamlDocuments $rustPath
    if ($null -eq $sourceDocs -or $null -eq $rustDocs) { Write-Output "NON_YAML_PAIR $rel"; continue }
    $keys = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal)
    foreach ($k in $sourceDocs.Keys) { [void]$keys.Add($k) }
    foreach ($k in $rustDocs.Keys) { [void]$keys.Add($k) }
    foreach ($key in $keys) {
        $classId = if ($sourceDocs.ContainsKey($key)) { $sourceDocs[$key].ClassId } else { $rustDocs[$key].ClassId }
        if ($classFilter.Count -gt 0 -and -not $classFilter.Contains($classId)) { continue }
        $sourceHas = $sourceDocs.ContainsKey($key)
        $rustHas = $rustDocs.ContainsKey($key)
        if (-not ($sourceHas -and $rustHas)) {
            Write-Output "UNPAIRED class=$classId $rel::$key (source=$sourceHas rust=$rustHas)"
            $realDiff++
            continue
        }
        $s = $sourceDocs[$key].Text; $r = $rustDocs[$key].Text
        if ($s -ceq $r) { $exact++; continue }
        $sn = Normalize-Guids $s $sourceGuids
        $rn = Normalize-Guids $r $rustGuids
        if ($sn -ceq $rn) { $guidEquivalent++; continue }
        $st = Get-StyleTokens $sn; $rt = Get-StyleTokens $rn
        $same = $st.Count -eq $rt.Count
        if ($same) {
            for ($i = 0; $i -lt $st.Count; $i++) { if ($st[$i] -cne $rt[$i]) { $same = $false; break } }
        }
        if ($same) {
            $styleOnly++
            if ($reported -lt $MaxReport) {
                Write-Output "STYLE_ONLY class=$classId $rel::$key tokens=$($st.Count)"
                $reported++
            }
            continue
        }
        $realDiff++
        if ($reported -lt $MaxReport) {
            Write-Output "REAL_DIFF class=$classId $rel::$key sourceTokens=$($st.Count) rustTokens=$($rt.Count)"
            $limit = [Math]::Min($st.Count, $rt.Count)
            $shown = 0
            for ($i = 0; $i -lt $limit -and $shown -lt 4; $i++) {
                if ($st[$i] -cne $rt[$i]) {
                    Write-Output ("  token[$i] source=<{0}> rust=<{1}>" -f $st[$i], $rt[$i])
                    $shown++
                }
            }
            if ($st.Count -ne $rt.Count) {
                Write-Output "  token-count differs: source=$($st.Count) rust=$($rt.Count)"
            }
            $reported++
        }
    }
}

Write-Output "SUMMARY exact=$exact guidPathEquivalent=$guidEquivalent styleOnly=$styleOnly realDiff=$realDiff missing=$missing"
