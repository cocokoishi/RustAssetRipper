[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $SourceProject,

    [Parameter(Mandatory = $true)]
    [string] $RustProject,

    [Parameter(Mandatory = $true)]
    [string] $ReportPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$projectRoots = @{
    Source = (Resolve-Path -LiteralPath $SourceProject).Path.TrimEnd('\', '/')
    Rust   = (Resolve-Path -LiteralPath $RustProject).Path.TrimEnd('\', '/')
}
$reportFullPath = [System.IO.Path]::GetFullPath($ReportPath)
$reportDirectory = [System.IO.Path]::GetDirectoryName($reportFullPath)
if (-not [System.IO.Directory]::Exists($reportDirectory)) {
    [System.IO.Directory]::CreateDirectory($reportDirectory) | Out-Null
}

$guidToAsset = @{}
$documentCache = @{}
foreach ($side in @('Source', 'Rust')) {
    $guidMap = @{}
    $assetsRoot = Join-Path $projectRoots[$side] 'Assets'
    if (-not [System.IO.Directory]::Exists($assetsRoot)) {
        throw "Missing Assets directory in $side project '$($projectRoots[$side])'."
    }

    Get-ChildItem -LiteralPath $assetsRoot -Recurse -File -Filter '*.meta' | ForEach-Object {
        $match = [regex]::Match(
            [System.IO.File]::ReadAllText($_.FullName),
            '(?m)^guid:\s*([0-9a-fA-F]{32})\s*$'
        )
        if ($match.Success) {
            $relative = $_.FullName.Substring($projectRoots[$side].Length + 1).Replace('\', '/')
            $assetPath = $relative.Substring(0, $relative.Length - 5)
            $guid = $match.Groups[1].Value.ToLowerInvariant()
            if (-not $guidMap.ContainsKey($guid)) {
                $guidMap[$guid] = $assetPath
            }
        }
    }
    $guidToAsset[$side] = $guidMap
}

function Get-UnityYamlDocuments([string] $Side, [string] $Path) {
    $fullPath = [System.IO.Path]::GetFullPath($Path)
    $cacheKey = $Side + '|' + $fullPath
    if ($documentCache.ContainsKey($cacheKey)) {
        return ,$documentCache[$cacheKey]
    }

    $text = [System.IO.File]::ReadAllText($fullPath)
    $headers = [regex]::Matches($text, '(?m)^--- !u!(\d+) &(-?\d+)[^\r\n]*\r?\n')
    $documents = @{}
    for ($index = 0; $index -lt $headers.Count; $index++) {
        $header = $headers[$index]
        $end = if ($index + 1 -lt $headers.Count) { $headers[$index + 1].Index } else { $text.Length }
        $bodyStart = $header.Index + $header.Length
        $body = $text.Substring($bodyStart, $end - $bodyStart)
        $type = [regex]::Match($body, '(?m)^([A-Za-z0-9_]+):\s*$').Groups[1].Value
        $name = [regex]::Match($body, '(?m)^  m_Name:[^\S\r\n]*(.*?)\s*$').Groups[1].Value
        $id = $header.Groups[2].Value
        $documents[$id] = [pscustomobject] @{
            Id    = $id
            Class = $header.Groups[1].Value
            Type  = $type
            Name  = $name
            Body  = $body
            Path  = $fullPath
        }
    }

    $documentCache[$cacheKey] = $documents
    return ,$documents
}

function Get-UnityFieldBlock($Document, [string] $Field) {
    $escapedField = [regex]::Escape($Field)
    $header = [regex]::Match($Document.Body, '(?m)^  ' + $escapedField + ':[^\r\n]*(?:\r?\n|$)')
    if (-not $header.Success) {
        return ''
    }

    $rest = $Document.Body.Substring($header.Index + $header.Length)
    $nextField = [regex]::Match($rest, '(?m)^  [A-Za-z_][A-Za-z0-9_]*:')
    if ($nextField.Success) {
        return $rest.Substring(0, $nextField.Index)
    }
    return $rest
}

function Get-UnityScalar([string] $Text, [string] $Field, [string] $Default = '') {
    $pattern = '(?m)^\s*(?:-\s*)?' + [regex]::Escape($Field) + ':[\t ]*(.*?)[\t ]*$'
    $match = [regex]::Match($Text, $pattern)
    if ($match.Success) {
        return $match.Groups[1].Value.Trim()
    }
    return $Default
}

function Get-UnityRootScalar($Document, [string] $Field, [string] $Default = '') {
    $pattern = '(?m)^  ' + [regex]::Escape($Field) + ':[\t ]*(.*?)[\t ]*$'
    $match = [regex]::Match($Document.Body, $pattern)
    if ($match.Success) {
        return $match.Groups[1].Value.Trim()
    }
    return $Default
}

function Get-UnityPointer([string] $Text) {
    $match = [regex]::Match($Text, '\{fileID:\s*(-?\d+)(?:,\s*guid:\s*([0-9a-fA-F]{32}))?')
    if (-not $match.Success) {
        return $null
    }
    return [pscustomobject] @{
        FileId = $match.Groups[1].Value
        Guid   = $match.Groups[2].Value.ToLowerInvariant()
    }
}

function Get-UnityFieldPointer($Document, [string] $Field) {
    $match = [regex]::Match(
        $Document.Body,
        '(?m)^  ' + [regex]::Escape($Field) + ':[^\S\r\n]*(\{fileID:[^\r\n]*\})'
    )
    if ($match.Success) {
        return Get-UnityPointer $match.Groups[1].Value
    }
    return $null
}

function Get-PositionProjection([string] $Text) {
    $match = [regex]::Match($Text, '\{\s*x:\s*([^,}]+),\s*y:\s*([^}]+)\}')
    if (-not $match.Success) {
        return $Text.Trim()
    }
    return [ordered] @{
        X = $match.Groups[1].Value.Trim()
        Y = $match.Groups[2].Value.Trim()
    }
}

function Get-BlendTreeProjection(
    [string] $Side,
    $OwnerDocuments,
    $Document,
    [string[]] $Stack
) {
    $treeKey = $Side + '|' + $Document.Path + '#' + $Document.Id
    if ($Stack -contains $treeKey) {
        return [ordered] @{ Kind = 'cycle'; Key = $treeKey }
    }
    $nextStack = @($Stack) + @($treeKey)

    $childrenBlock = Get-UnityFieldBlock $Document 'm_Childs'
    $childStarts = [regex]::Matches($childrenBlock, '(?m)^\s*- serializedVersion:')
    $children = [System.Collections.Generic.List[object]]::new()
    for ($index = 0; $index -lt $childStarts.Count; $index++) {
        $start = $childStarts[$index].Index
        $end = if ($index + 1 -lt $childStarts.Count) { $childStarts[$index + 1].Index } else { $childrenBlock.Length }
        $child = $childrenBlock.Substring($start, $end - $start)
        $motionPointer = Get-UnityPointer $child
        $motion = Resolve-UnityMotion $Side $OwnerDocuments $motionPointer $nextStack
        $isAnim = Get-UnityScalar $child 'm_IsAnim' '0'

        $children.Add([ordered] @{
            SerializedVersion     = Get-UnityScalar $child 'serializedVersion' '2'
            Motion                = $motion
            Threshold             = Get-UnityScalar $child 'm_Threshold'
            Position              = Get-PositionProjection (Get-UnityScalar $child 'm_Position')
            TimeScale             = Get-UnityScalar $child 'm_TimeScale'
            CycleOffset           = Get-UnityScalar $child 'm_CycleOffset'
            DirectBlendParameter = Get-UnityScalar $child 'm_DirectBlendParameter'
            Mirror                = Get-UnityScalar $child 'm_Mirror'
            IsAnim                = ($isAnim -in @('1', 'true', 'True'))
            Weight                = Get-UnityScalar $child 'm_Weight'
        })
    }

    return [ordered] @{
        Kind                    = 'BlendTree'
        ClassId                 = '206'
        SerializedVersion       = Get-UnityRootScalar $Document 'serializedVersion' '1'
        Name                    = $Document.Name
        ObjectHideFlags         = Get-UnityRootScalar $Document 'm_ObjectHideFlags'
        BlendParameter          = Get-UnityRootScalar $Document 'm_BlendParameter'
        BlendParameterY         = Get-UnityRootScalar $Document 'm_BlendParameterY'
        MinThreshold            = Get-UnityRootScalar $Document 'm_MinThreshold'
        MaxThreshold            = Get-UnityRootScalar $Document 'm_MaxThreshold'
        UseAutomaticThresholds = Get-UnityRootScalar $Document 'm_UseAutomaticThresholds'
        NormalizedBlendValues   = Get-UnityRootScalar $Document 'm_NormalizedBlendValues'
        BlendType               = Get-UnityRootScalar $Document 'm_BlendType'
        Children                = @($children.ToArray())
    }
}

function Resolve-UnityMotion([string] $Side, $OwnerDocuments, $Pointer, [string[]] $Stack) {
    if ($null -eq $Pointer -or $Pointer.FileId -eq '0') {
        return [ordered] @{ Kind = 'null' }
    }

    $targetDocuments = $OwnerDocuments
    $targetDocument = $null
    if ($Pointer.Guid) {
        if (-not $guidToAsset[$Side].ContainsKey($Pointer.Guid)) {
            return [ordered] @{ Kind = 'unmapped-guid'; Guid = $Pointer.Guid; FileId = $Pointer.FileId }
        }

        $assetPath = $guidToAsset[$Side][$Pointer.Guid]
        if ([System.IO.Path]::GetExtension($assetPath).Equals('.blendtree', [System.StringComparison]::OrdinalIgnoreCase)) {
            $treePath = Join-Path $projectRoots[$Side] $assetPath
            $targetDocuments = Get-UnityYamlDocuments $Side $treePath
            if ($targetDocuments.ContainsKey($Pointer.FileId)) {
                $targetDocument = $targetDocuments[$Pointer.FileId]
            }
            if ($null -eq $targetDocument) {
                $targetDocument = @($targetDocuments.Values | Where-Object Class -eq '206' | Select-Object -First 1)[0]
            }
            if ($null -eq $targetDocument) {
                return [ordered] @{ Kind = 'missing-tree-document'; Asset = $assetPath; FileId = $Pointer.FileId }
            }
            return Get-BlendTreeProjection $Side $targetDocuments $targetDocument $Stack
        }

        return [ordered] @{ Kind = 'asset'; Path = $assetPath; FileId = $Pointer.FileId }
    }

    if ($targetDocuments.ContainsKey($Pointer.FileId)) {
        $targetDocument = $targetDocuments[$Pointer.FileId]
    }
    if ($null -eq $targetDocument) {
        return [ordered] @{ Kind = 'unresolved-local'; FileId = $Pointer.FileId }
    }
    if ($targetDocument.Class -eq '206') {
        return Get-BlendTreeProjection $Side $targetDocuments $targetDocument $Stack
    }

    return [ordered] @{
        Kind    = 'local-object'
        ClassId = $targetDocument.Class
        Name    = $targetDocument.Name
        FileId  = $Pointer.FileId
    }
}

function Get-BlendTreeRows([string] $Side) {
    $assetsRoot = Join-Path $projectRoots[$Side] 'Assets'
    $rows = [System.Collections.Generic.List[object]]::new()
    Get-ChildItem -LiteralPath $assetsRoot -Recurse -File | Where-Object {
        $_.Extension -in @('.controller', '.blendtree')
    } | ForEach-Object {
        $documents = Get-UnityYamlDocuments $Side $_.FullName
        foreach ($document in @($documents.Values | Where-Object Class -eq '206')) {
            $relative = $document.Path.Substring($projectRoots[$Side].Length + 1).Replace('\', '/')
            $rows.Add([pscustomobject] @{
                Key       = $relative + '#' + $document.Id
                Documents = $documents
                Document  = $document
            })
        }
    }
    return ,$rows
}

function Get-ProjectionHash($Projection) {
    $json = ConvertTo-Json -InputObject $Projection -Depth 100 -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        return [System.BitConverter]::ToString($sha256.ComputeHash($bytes)).Replace('-', '')
    }
    finally {
        $sha256.Dispose()
    }
}

function Get-TreeInventory([string] $Side, $Rows) {
    $byHash = @{}
    $childCount = 0
    $rootSerializedVersionFields = 0
    $childIsAnimFields = 0
    foreach ($row in $Rows) {
        $projection = Get-BlendTreeProjection $Side $row.Documents $row.Document @()
        $hash = Get-ProjectionHash $projection
        if (-not $byHash.ContainsKey($hash)) {
            $byHash[$hash] = [System.Collections.Generic.List[string]]::new()
        }
        $byHash[$hash].Add($row.Key)

        $childrenBlock = Get-UnityFieldBlock $row.Document 'm_Childs'
        $childCount += [regex]::Matches($childrenBlock, '(?m)^\s*- serializedVersion:').Count
        if ([regex]::IsMatch($row.Document.Body, '(?m)^  serializedVersion:')) {
            $rootSerializedVersionFields++
        }
        $childIsAnimFields += [regex]::Matches($childrenBlock, '(?m)^\s+m_IsAnim:').Count
    }

    return [pscustomobject] @{
        Rows                       = $Rows
        ByHash                     = $byHash
        ChildCount                 = $childCount
        RootSerializedVersionFields = $rootSerializedVersionFields
        ChildIsAnimFields          = $childIsAnimFields
    }
}

$sourceRows = Get-BlendTreeRows 'Source'
$rustRows = Get-BlendTreeRows 'Rust'
$sourceInventory = Get-TreeInventory 'Source' $sourceRows
$rustInventory = Get-TreeInventory 'Rust' $rustRows

$differenceHashes = [System.Collections.Generic.List[string]]::new()
$allHashes = @($sourceInventory.ByHash.Keys) + @($rustInventory.ByHash.Keys) | Sort-Object -Unique
$unmatched = [System.Collections.Generic.List[object]]::new()
foreach ($hash in $allHashes) {
    $sourceCount = if ($sourceInventory.ByHash.ContainsKey($hash)) { $sourceInventory.ByHash[$hash].Count } else { 0 }
    $rustCount = if ($rustInventory.ByHash.ContainsKey($hash)) { $rustInventory.ByHash[$hash].Count } else { 0 }
    if ($sourceCount -ne $rustCount) {
        $differenceHashes.Add($hash)
        $unmatched.Add([ordered] @{
            Sha256      = $hash
            SourceCount = $sourceCount
            RustCount   = $rustCount
            SourceRoots = if ($sourceInventory.ByHash.ContainsKey($hash)) { @($sourceInventory.ByHash[$hash].ToArray()) } else { @() }
            RustRoots   = if ($rustInventory.ByHash.ContainsKey($hash)) { @($rustInventory.ByHash[$hash].ToArray()) } else { @() }
        })
    }
}

$report = [ordered] @{
    SchemaVersion = 1
    Comparison    = 'Unity BlendTree recursive motion graphs'
    SourceProject = $projectRoots.Source
    RustProject   = $projectRoots.Rust
    Source        = [ordered] @{
        TreeDocuments                  = $sourceRows.Count
        UniqueSemanticGraphs           = $sourceInventory.ByHash.Count
        ChildRecords                   = $sourceInventory.ChildCount
        RootSerializedVersionFields   = $sourceInventory.RootSerializedVersionFields
        ChildIsAnimFields              = $sourceInventory.ChildIsAnimFields
    }
    Rust          = [ordered] @{
        TreeDocuments                  = $rustRows.Count
        UniqueSemanticGraphs           = $rustInventory.ByHash.Count
        ChildRecords                   = $rustInventory.ChildCount
        RootSerializedVersionFields   = $rustInventory.RootSerializedVersionFields
        ChildIsAnimFields              = $rustInventory.ChildIsAnimFields
    }
    DifferenceSignatures = $differenceHashes.Count
    SemanticMatch        = ($differenceHashes.Count -eq 0 -and $sourceRows.Count -eq $rustRows.Count)
    UnmatchedGraphs      = @($unmatched.ToArray())
    Normalization        = @(
        'External object pointers are mapped to project asset paths and fileIDs; ordered child motion structure is preserved.',
        'The reference runtime probe reports BlendTree serializedVersion 1 and ChildMotion IsAnim false; omitted YAML fields normalize to those defaults.',
        'Unity Editor import/runtime behavior and dynamic animation sampling are outside this comparison.'
    )
}

[System.IO.File]::WriteAllText(
    $reportFullPath,
    (ConvertTo-Json -InputObject $report -Depth 100) + [Environment]::NewLine,
    [System.Text.UTF8Encoding]::new($false)
)
Write-Output "Wrote BlendTree comparison report: $reportFullPath"
Write-Output ("Tree documents: {0}/{1}; unique graphs: {2}/{3}; child records: {4}/{5}; semantic differences: {6}" -f `
    $sourceRows.Count, $rustRows.Count, $sourceInventory.ByHash.Count, $rustInventory.ByHash.Count, `
    $sourceInventory.ChildCount, $rustInventory.ChildCount, $differenceHashes.Count)
if (-not $report.SemanticMatch) {
    exit 1
}
