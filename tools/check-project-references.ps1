# Checks file-local and external PPtrs in exported Unity YAML and .meta files.
# YAML targets must contain the referenced local fileID. Imported binary/text
# assets and Unity builtins are classified separately: ownership/existence is
# checked, but their imported subasset IDs require Unity and are not proven here.
# Use short directory junctions when running under Windows PowerShell 5.1.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ProjectRoot,
    [string]$ReportPath,
    [switch]$RequireComplete
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = [System.IO.Path]::GetFullPath($ProjectRoot).TrimEnd('\', '/')
if (-not [System.IO.Directory]::Exists($root)) { throw "Project root does not exist: $root" }

function Relative-Path([string]$Path) {
    return $Path.Substring($root.Length).TrimStart('\', '/').Replace('\', '/')
}

$issues = [System.Collections.Generic.List[object]]::new()
$unverified = [System.Collections.Generic.List[object]]::new()
function Add-Issue([string]$Kind, [string]$Path, [string]$Detail, [int]$Offset = -1) {
    $issues.Add([pscustomobject]@{ Kind = $Kind; Path = $Path; Offset = $Offset; Detail = $Detail })
}

$guidOwners = @{}
$documents = @{}
$yaml = [System.Collections.Generic.List[object]]::new()
$files = @(Get-ChildItem -LiteralPath $root -Recurse -File | Sort-Object FullName)
foreach ($file in $files) {
    $relative = Relative-Path $file.FullName
    $isMeta = $file.Extension -ieq '.meta'
    if (-not $isMeta) {
        # Detect Unity YAML without allocating full binary media as text.
        $reader = [System.IO.StreamReader]::new($file.FullName)
        $prefix = [char[]]::new(64)
        try { $read = $reader.Read($prefix, 0, $prefix.Length) } finally { $reader.Dispose() }
        $start = [string]::new($prefix, 0, $read)
        if ($start -notmatch '^(%YAML|%TAG|--- !u!)') {
            if (-not $relative.StartsWith('ProjectSettings/', [StringComparison]::Ordinal) -or
                $file.Extension -ine '.asset') { continue }
        }
    }
    $text = [System.IO.File]::ReadAllText($file.FullName)
    # Keep file identities, not all YAML strings: large meshes can exceed 300 MB
    # each, and keeping both passes resident would retain the entire project.
    $yaml.Add([pscustomobject]@{ Path = $relative; FullPath = $file.FullName; IsMeta = $isMeta })
    if ($isMeta) {
        $matches = [regex]::Matches($text, '(?m)^guid:\s*([0-9a-fA-F]{32})\s*\r?$')
        $owner = $relative.Substring(0, $relative.Length - 5)
        if ($matches.Count -ne 1) {
            Add-Issue 'InvalidMetaGuid' $relative "Expected one GUID, found $($matches.Count)"
            continue
        }
        $guid = $matches[0].Groups[1].Value.ToLowerInvariant()
        if ($guidOwners.ContainsKey($guid)) {
            Add-Issue 'DuplicateGuid' $relative "GUID $guid also belongs to $($guidOwners[$guid])"
        } else { $guidOwners[$guid] = $owner }
        $ownerPath = Join-Path $root $owner
        if (-not [System.IO.File]::Exists($ownerPath) -and -not [System.IO.Directory]::Exists($ownerPath)) {
            Add-Issue 'MissingMetaOwner' $relative "Owner does not exist: $owner"
        }
    }
    $ids = @{}
    foreach ($header in [regex]::Matches($text, '(?m)^--- !u!(\d+) &(-?\d+)(?:[^\r\n]*)\r?$')) {
        $id = $header.Groups[2].Value
        try { $id = ([long]::Parse($id, [Globalization.CultureInfo]::InvariantCulture)).ToString([Globalization.CultureInfo]::InvariantCulture) }
        catch { Add-Issue 'InvalidDocumentId' $relative $id $header.Index; continue }
        if ($ids.ContainsKey($id)) { Add-Issue 'DuplicateFileId' $relative "Duplicate YAML fileID $id" $header.Index }
        else { $ids[$id] = $true }
    }
    if ($ids.Count -gt 0) { $documents[$relative] = $ids }
}

$checked = 0; $localChecked = 0; $externalChecked = 0; $nulls = 0; $builtins = 0; $ownerOnly = 0
$pointerPattern = '^\{\s*fileID:\s*(?<id>-?\d+)\s*(?:,\s*guid:\s*(?<guid>[0-9a-fA-F]{32})\s*,\s*type:\s*(?<type>\d+)\s*)?\}$'
foreach ($file in $yaml) {
    $text = [System.IO.File]::ReadAllText($file.FullPath)
    foreach ($literal in [regex]::Matches($text, '\{[^{}\r\n]*\bfileID\s*:[^{}\r\n]*\}')) {
        $pointer = [regex]::Match($literal.Value, $pointerPattern)
        if (-not $pointer.Success) {
            Add-Issue 'UnsupportedPointerSyntax' $file.Path $literal.Value $literal.Index
            continue
        }
        try { $id = [long]::Parse($pointer.Groups['id'].Value, [Globalization.CultureInfo]::InvariantCulture) }
        catch { Add-Issue 'InvalidPointerId' $file.Path $literal.Value $literal.Index; continue }
        if ($id -eq 0) { $nulls++; continue }
        $fid = $id.ToString([Globalization.CultureInfo]::InvariantCulture)
        $guid = $pointer.Groups['guid'].Value.ToLowerInvariant()
        if ($guid -match '^0{16}[ef]0{15}$') {
            $builtins++
            $unverified.Add([pscustomobject]@{ Kind = 'UnityBuiltin'; Path = $file.Path; Offset = $literal.Index; FileId = $fid; Guid = $guid })
            continue
        }
        $checked++
        if ($guid.Length -eq 0 -or $guid -eq ('0' * 32)) {
            $localChecked++
            if (-not $documents.ContainsKey($file.Path) -or -not $documents[$file.Path].ContainsKey($fid)) {
                Add-Issue 'DanglingLocalReference' $file.Path "No YAML document with fileID $fid in the same file" $literal.Index
            }
            continue
        }
        $externalChecked++
        if (-not $guidOwners.ContainsKey($guid)) {
            Add-Issue 'UnmappedGuid' $file.Path "GUID $guid, fileID $fid" $literal.Index
            continue
        }
        $target = $guidOwners[$guid]
        if (-not [System.IO.File]::Exists((Join-Path $root $target))) {
            Add-Issue 'MissingReferenceTarget' $file.Path "GUID $guid resolves to absent file $target" $literal.Index
        } elseif ($documents.ContainsKey($target)) {
            # A SceneAsset uses an imported ID; scene objects still need exact IDs.
            if ($target.EndsWith('.unity', [StringComparison]::OrdinalIgnoreCase) -and $fid -eq '102900000') {
                $ownerOnly++
                $unverified.Add([pscustomobject]@{ Kind = 'ImportedSceneAsset'; Path = $file.Path; Offset = $literal.Index; Target = $target; FileId = $fid })
            } elseif (-not $documents[$target].ContainsKey($fid)) {
                Add-Issue 'DanglingExternalReference' $file.Path "$target has no YAML document with fileID $fid (GUID $guid)" $literal.Index
            }
        } else {
            $ownerOnly++
            $unverified.Add([pscustomobject]@{ Kind = 'ImportedAssetOwnerOnly'; Path = $file.Path; Offset = $literal.Index; Target = $target; FileId = $fid })
        }
    }
}
$status = if ($issues.Count -gt 0) { 'FAIL' } elseif ($unverified.Count -gt 0) { 'PARTIAL' } else { 'PASS' }
$report = [ordered]@{
    SchemaVersion = 1; CheckScope = 'ExportedYAMLReferenceExistence'; Status = $status
    ProjectRoot = $root; FilesInventoried = $files.Count; YamlAndMetaFiles = $yaml.Count
    GuidOwners = $guidOwners.Count; YamlDocumentFiles = $documents.Count
    Checked = $checked; LocalChecked = $localChecked; ExternalChecked = $externalChecked
    NullReferences = $nulls; BuiltinReferences = $builtins; ImportedOwnerOnlyReferences = $ownerOnly
    CompleteReferenceValidation = ($issues.Count -eq 0 -and $unverified.Count -eq 0)
    CompleteValidationRequired = [bool]$RequireComplete
    Issues = @($issues.ToArray()); UnverifiedTargets = @($unverified.ToArray())
}
if ($ReportPath) {
    $reportFull = [System.IO.Path]::GetFullPath($ReportPath)
    [void][System.IO.Directory]::CreateDirectory([System.IO.Path]::GetDirectoryName($reportFull))
    $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportFull -Encoding UTF8
}
"status=$status checked=$checked local=$localChecked external=$externalChecked issues=$($issues.Count) builtin=$builtins importedOwnerOnly=$ownerOnly"
$issues | Select-Object -First 10 | ForEach-Object { "$($_.Kind): $($_.Path)@$($_.Offset): $($_.Detail)" }
if ($issues.Count -gt 0) { exit 1 }
if ($RequireComplete -and $unverified.Count -gt 0) { exit 2 }
