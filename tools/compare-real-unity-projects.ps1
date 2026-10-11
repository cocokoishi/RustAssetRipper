[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $SourceProject,

    [Parameter(Mandatory = $true)]
    [string] $RustProject,

    [Parameter(Mandatory = $true)]
    [string] $Bundle,

    [Parameter(Mandatory = $true)]
    [string] $ReportPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$sourceRoot = (Resolve-Path -LiteralPath $SourceProject).Path.TrimEnd('\')
$rustRoot = (Resolve-Path -LiteralPath $RustProject).Path.TrimEnd('\')
$bundlePath = (Resolve-Path -LiteralPath $Bundle).Path
$reportFullPath = [System.IO.Path]::GetFullPath($ReportPath)
$reportDirectory = [System.IO.Path]::GetDirectoryName($reportFullPath)
if (-not [System.IO.Directory]::Exists($reportDirectory)) {
    [System.IO.Directory]::CreateDirectory($reportDirectory) | Out-Null
}

function Get-ExtendedIoPath([string] $Path) {
    $fullPath = [System.IO.Path]::GetFullPath($Path)
    if ($fullPath.StartsWith('\\?\', [System.StringComparison]::Ordinal)) {
        return $fullPath
    }
    if ($fullPath.StartsWith('\\', [System.StringComparison]::Ordinal)) {
        return '\\?\UNC\' + $fullPath.Substring(2)
    }
    return '\\?\' + $fullPath
}

function Get-ProjectInventory([string] $Root) {
    $files = @{}
    $metas = @{}
    $guidToAsset = @{}
    Get-ChildItem -LiteralPath $Root -Recurse -File | ForEach-Object {
        $relative = $_.FullName.Substring($Root.Length + 1).Replace('\', '/')
        $files[$relative] = [long] $_.Length
        if ($relative.EndsWith('.meta', [System.StringComparison]::OrdinalIgnoreCase)) {
            $assetPath = $relative.Substring(0, $relative.Length - 5)
            $content = [System.IO.File]::ReadAllText((Get-ExtendedIoPath $_.FullName))
            $guidMatch = [regex]::Match($content, '(?m)^guid:\s*([0-9a-fA-F]{32})\s*$')
            $mainIdMatch = [regex]::Match($content, '(?m)^\s*mainObjectFileID:\s*(-?\d+)\s*$')
            if ($guidMatch.Success) {
                $guid = $guidMatch.Groups[1].Value.ToLowerInvariant()
                $metas[$assetPath] = [ordered] @{
                    Guid = $guid
                    MainObjectFileID = if ($mainIdMatch.Success) { $mainIdMatch.Groups[1].Value } else { $null }
                    MetaPath = $relative
                }
                if (-not $guidToAsset.ContainsKey($guid)) {
                    $guidToAsset[$guid] = $assetPath
                }
            }
        }
    }
    return [ordered] @{
        Root = $Root
        Files = $files
        Metas = $metas
        GuidToAsset = $guidToAsset
    }
}

function Get-PathIdInventory([string] $Root) {
    $path = Join-Path $Root 'AuxiliaryFiles/path_id_map.json'
    if (-not [System.IO.File]::Exists($path)) {
        $path = Join-Path (Split-Path -Parent $Root) 'AuxiliaryFiles/path_id_map.json'
    }
    if (-not [System.IO.File]::Exists($path)) {
        throw "Missing AuxiliaryFiles/path_id_map.json in '$Root' or its parent."
    }
    $document = [System.IO.File]::ReadAllText($path) | ConvertFrom-Json
    $rows = @{}
    foreach ($file in $document.Files) {
        foreach ($asset in $file.Assets) {
            $key = '{0}|{1}' -f $file.Name, $asset.PathID
            $rows[$key] = '{0}|{1}' -f $asset.Name, $asset.Type
        }
    }
    return [ordered] @{
        Document = $document
        Rows = $rows
        Files = @($document.Files | ForEach-Object {
            [ordered] @{
                Name = $_.Name
                AssetCount = $_.Assets.Count
            }
        })
    }
}

function New-AssetPairing($SourceInventory, $RustInventory) {
    $rustCandidates = @{}
    foreach ($assetPath in $RustInventory.Metas.Keys) {
        $parent = [System.IO.Path]::GetDirectoryName($assetPath.Replace('/', '\')).Replace('\', '/')
        $stem = [System.IO.Path]::GetFileNameWithoutExtension($assetPath)
        $candidateKey = ($parent + '/' + $stem).TrimStart('/')
        if (-not $rustCandidates.ContainsKey($candidateKey)) {
            $rustCandidates[$candidateKey] = [System.Collections.Generic.List[string]]::new()
        }
        $rustCandidates[$candidateKey].Add($assetPath)
    }

    $sourceToRust = @{}
    $rustToSource = @{}
    $exact = 0
    $extensionChanged = 0
    $ambiguous = [System.Collections.Generic.List[string]]::new()
    foreach ($sourceAssetPath in $SourceInventory.Metas.Keys) {
        if ($RustInventory.Metas.ContainsKey($sourceAssetPath)) {
            $sourceToRust[$sourceAssetPath] = $sourceAssetPath
            $rustToSource[$sourceAssetPath] = $sourceAssetPath
            $exact += 1
            continue
        }
        $parent = [System.IO.Path]::GetDirectoryName($sourceAssetPath.Replace('/', '\')).Replace('\', '/')
        $stem = [System.IO.Path]::GetFileNameWithoutExtension($sourceAssetPath)
        $candidateKey = ($parent + '/' + $stem).TrimStart('/')
        if ($rustCandidates.ContainsKey($candidateKey) -and $rustCandidates[$candidateKey].Count -eq 1) {
            $rustAssetPath = $rustCandidates[$candidateKey][0]
            $sourceToRust[$sourceAssetPath] = $rustAssetPath
            $rustToSource[$rustAssetPath] = $sourceAssetPath
            $extensionChanged += 1
        } elseif ($rustCandidates.ContainsKey($candidateKey)) {
            $ambiguous.Add($sourceAssetPath)
        }
    }
    return [ordered] @{
        SourceToRust = $sourceToRust
        RustToSource = $rustToSource
        Exact = $exact
        ExtensionChanged = $extensionChanged
        UnpairedSource = @($SourceInventory.Metas.Keys | Where-Object { -not $sourceToRust.ContainsKey($_) })
        UnpairedRust = @($RustInventory.Metas.Keys | Where-Object { -not $rustToSource.ContainsKey($_) })
        Ambiguous = @($ambiguous)
    }
}

function Get-CanonicalAsset([string] $Side, [string] $AssetPath, $Pairing) {
    if ($Side -eq 'source') {
        return $AssetPath
    }
    if ($Pairing.RustToSource.ContainsKey($AssetPath)) {
        return $Pairing.RustToSource[$AssetPath]
    }
    return 'RUST_ONLY/' + $AssetPath
}

function Get-CanonicalPointer([string] $Side, [string] $Line, [string] $Owner, $Inventory, $Pairing) {
    $fileIdMatch = [regex]::Match($Line, '\{[^{}]*\bfileID:\s*(-?\d+)[^{}]*\}')
    if (-not $fileIdMatch.Success) {
        return $null
    }
    $fileId = $fileIdMatch.Groups[1].Value
    if ($fileId -eq '0') {
        return 'null'
    }
    $guidMatch = [regex]::Match($fileIdMatch.Value, '\bguid:\s*([0-9a-fA-F]{32})')
    if (-not $guidMatch.Success) {
        return $Owner + '#' + $fileId
    }
    $guid = $guidMatch.Groups[1].Value.ToLowerInvariant()
    if ($Inventory.GuidToAsset.ContainsKey($guid)) {
        $target = Get-CanonicalAsset $Side $Inventory.GuidToAsset[$guid] $Pairing
        return $target + '#' + $fileId
    }
    return 'GUID/' + $guid + '#' + $fileId
}

function Add-YamlDocument($Docs, $ClassCounts, $TypeCounts, [string] $Side, [string] $AssetPath, [string] $ClassId, [string] $FileId, [string] $TypeName, $References, $Pairing) {
    if ([string]::IsNullOrEmpty($ClassId)) {
        return
    }
    $canonicalOwner = Get-CanonicalAsset $Side $AssetPath $Pairing
    $key = '{0}|{1}|{2}' -f $canonicalOwner, $ClassId, $FileId
    $sortedReferences = @($references | Sort-Object)
    $Docs[$key] = [ordered] @{
        Asset = $canonicalOwner
        ClassID = $ClassId
        FileID = $FileId
        Type = $TypeName
        References = $sortedReferences
        ReferenceCount = $sortedReferences.Count
    }
    if (-not $ClassCounts.ContainsKey($ClassId)) {
        $ClassCounts[$ClassId] = 0
    }
    $ClassCounts[$ClassId] += 1
    if (-not [string]::IsNullOrEmpty($TypeName)) {
        if (-not $TypeCounts.ContainsKey($TypeName)) {
            $TypeCounts[$TypeName] = 0
        }
        $TypeCounts[$TypeName] += 1
    }
}

function Get-YamlDocuments([string] $Root, [string] $Side, $Inventory, $Pairing) {
    $docs = @{}
    $classCounts = @{}
    $typeCounts = @{}
    $inventoryLines = & cargo run --locked --quiet --manifest-path (Join-Path $repoRoot 'Cargo.toml') --bin real_project_yaml_inventory -- $Root
    if ($LASTEXITCODE -ne 0) {
        throw "real_project_yaml_inventory failed for '$Root' with exit code $LASTEXITCODE."
    }
    foreach ($rowLine in $inventoryLines) {
        $row = $rowLine | ConvertFrom-Json
        $owner = Get-CanonicalAsset $Side ([string] $row.path) $Pairing
        $key = '{0}|{1}|{2}' -f $owner, $row.classId, $row.fileId
        $references = [System.Collections.Generic.List[string]]::new()
        $nullPointerCount = 0
        foreach ($pointer in $row.pointers) {
            $fileId = [string] $pointer.fileId
            if ($fileId -eq '0') {
                $nullPointerCount += 1
            } elseif ($null -eq $pointer.guid) {
                $references.Add($owner + '#' + $fileId)
            } else {
                $guid = ([string] $pointer.guid).ToLowerInvariant()
                if ($Inventory.GuidToAsset.ContainsKey($guid)) {
                    $target = Get-CanonicalAsset $Side $Inventory.GuidToAsset[$guid] $Pairing
                    $references.Add($target + '#' + $fileId)
                } else {
                    $references.Add('GUID/' + $guid + '#' + $fileId)
                }
            }
        }
        $sortedReferences = @($references | Sort-Object)
        $typeName = if ($null -eq $row.type) { '' } else { [string] $row.type }
        $docs[$key] = [ordered] @{
            Key = $key
            Asset = $owner
            DocumentIndex = [int] $row.documentIndex
            ClassID = [string] $row.classId
            FileID = [string] $row.fileId
            Type = $typeName
            Name = if ($null -eq $row.name) { $null } else { [string] $row.name }
            References = $sortedReferences
            ReferenceCount = $sortedReferences.Count
            NullPointerCount = $nullPointerCount
        }
        $classId = [string] $row.classId
        if (-not $classCounts.ContainsKey($classId)) {
            $classCounts[$classId] = 0
        }
        $classCounts[$classId] += 1
        if (-not [string]::IsNullOrEmpty($typeName)) {
            if (-not $typeCounts.ContainsKey($typeName)) {
                $typeCounts[$typeName] = 0
            }
            $typeCounts[$typeName] += 1
        }
    }
    $documentsByAsset = @{}
    foreach ($document in $docs.Values) {
        if (-not $documentsByAsset.ContainsKey($document.Asset)) {
            $documentsByAsset[$document.Asset] = [System.Collections.Generic.List[object]]::new()
        }
        $documentsByAsset[$document.Asset].Add($document)
    }
    return [ordered] @{
        Documents = $docs
        DocumentsByAsset = $documentsByAsset
        ClassCounts = $classCounts
        TypeCounts = $typeCounts
    }
}

function Compare-PathIdRows($SourceMap, $RustMap) {
    $sourceKeys = @($SourceMap.Rows.Keys)
    $rustKeys = @($RustMap.Rows.Keys)
    $sameIdentity = @($sourceKeys | Where-Object { $RustMap.Rows.ContainsKey($_) })
    $changed = @($sameIdentity | Where-Object { $SourceMap.Rows[$_] -ne $RustMap.Rows[$_] })
    return [ordered] @{
        SourceFileCount = $SourceMap.Files.Count
        RustFileCount = $RustMap.Files.Count
        SourceObjectRows = $sourceKeys.Count
        RustObjectRows = $rustKeys.Count
        SameIdentityRows = $sameIdentity.Count
        NameOrTypeDifferences = @($changed | ForEach-Object {
            [ordered] @{ Identity = $_; Source = $SourceMap.Rows[$_]; Rust = $RustMap.Rows[$_] }
        })
        SourceOnlyRows = @($sourceKeys | Where-Object { -not $RustMap.Rows.ContainsKey($_) } | Select-Object -First 100)
        RustOnlyRows = @($rustKeys | Where-Object { -not $SourceMap.Rows.ContainsKey($_) } | Select-Object -First 100)
        SourceFiles = $SourceMap.Files
        RustFiles = $RustMap.Files
    }
}

function Get-YamlDocumentSignature($Document) {
    $name = if ($null -eq $Document.Name) { '' } else { [string] $Document.Name }
    return ([string] $Document.ClassID) + [char] 0 + [string] $Document.Type + [char] 0 + $name
}

function Group-YamlDocumentsBySignature($Documents) {
    $groups = @{}
    foreach ($document in $Documents) {
        $signature = Get-YamlDocumentSignature $document
        if (-not $groups.ContainsKey($signature)) {
            $groups[$signature] = [System.Collections.Generic.List[object]]::new()
        }
        $groups[$signature].Add($document)
    }
    return $groups
}

function New-YamlDocumentPairing($SourceYaml, $RustYaml) {
    $sourceToRust = @{}
    $rustToSource = @{}
    $sourcePointerToRust = @{}
    $pairs = [System.Collections.Generic.List[object]]::new()
    $assets = @($SourceYaml.DocumentsByAsset.Keys)

    foreach ($asset in $assets) {
        $sourceDocuments = @($SourceYaml.DocumentsByAsset[$asset] | Sort-Object DocumentIndex)
        $rustDocuments = @()
        if ($RustYaml.DocumentsByAsset.ContainsKey($asset)) {
            $rustDocuments = @($RustYaml.DocumentsByAsset[$asset] | Sort-Object DocumentIndex)
        }
        if ($sourceDocuments.Count -eq 0 -or $rustDocuments.Count -eq 0) {
            continue
        }

        $orderedMatch = $sourceDocuments.Count -eq $rustDocuments.Count
        if ($orderedMatch) {
            for ($index = 0; $index -lt $sourceDocuments.Count; $index++) {
                if ((Get-YamlDocumentSignature $sourceDocuments[$index]) -cne
                    (Get-YamlDocumentSignature $rustDocuments[$index])) {
                    $orderedMatch = $false
                    break
                }
            }
        }
        if ($orderedMatch) {
            for ($index = 0; $index -lt $sourceDocuments.Count; $index++) {
                $pairs.Add([ordered] @{
                    Source = $sourceDocuments[$index]
                    Rust = $rustDocuments[$index]
                    Method = 'same-path-document-order'
                })
            }
            continue
        }

        $sourceGroups = Group-YamlDocumentsBySignature $sourceDocuments
        $rustGroups = Group-YamlDocumentsBySignature $rustDocuments
        foreach ($signature in $sourceGroups.Keys) {
            if (-not $rustGroups.ContainsKey($signature)) {
                continue
            }
            $sourceMatches = @($sourceGroups[$signature] | Sort-Object DocumentIndex)
            $rustMatches = @($rustGroups[$signature] | Sort-Object DocumentIndex)
            if ($sourceMatches.Count -ne $rustMatches.Count) {
                continue
            }
            for ($index = 0; $index -lt $sourceMatches.Count; $index++) {
                $pairs.Add([ordered] @{
                    Source = $sourceMatches[$index]
                    Rust = $rustMatches[$index]
                    Method = 'class-type-name'
                })
            }
        }
    }

    foreach ($pair in $pairs) {
        $source = $pair.Source
        $rust = $pair.Rust
        $sourceToRust[$source.Key] = $rust.Key
        $rustToSource[$rust.Key] = $source.Key
        $sourcePointer = $source.Asset + '#' + $source.FileID
        $rustPointer = $rust.Asset + '#' + $rust.FileID
        $sourcePointerToRust[$sourcePointer] = $rustPointer
    }

    return [ordered] @{
        Pairs = $pairs.ToArray()
        SourceToRust = $sourceToRust
        RustToSource = $rustToSource
        SourcePointerToRust = $sourcePointerToRust
    }
}

function Map-SourceReferences($References, $PointerMap) {
    $mapped = [System.Collections.Generic.List[string]]::new()
    foreach ($reference in $References) {
        if ($PointerMap.ContainsKey($reference)) {
            $mapped.Add($PointerMap[$reference])
        } else {
            $mapped.Add([string] $reference)
        }
    }
    return @($mapped | Sort-Object)
}

function Compare-Yaml($SourceYaml, $RustYaml, $DocumentPairing) {
    $sourceKeys = @($SourceYaml.Documents.Keys)
    $rustKeys = @($RustYaml.Documents.Keys)
    $rawMatched = @($sourceKeys | Where-Object { $RustYaml.Documents.ContainsKey($_) })
    $referenceMatches = [System.Collections.Generic.List[string]]::new()
    $rawReferenceMatches = [System.Collections.Generic.List[string]]::new()
    $referenceDifferences = [System.Collections.Generic.List[object]]::new()
    $nullPointerDifferences = [System.Collections.Generic.List[object]]::new()
    $localFileIdMappings = [System.Collections.Generic.List[object]]::new()
    foreach ($pair in $DocumentPairing.Pairs) {
        $sourceDocument = $pair.Source
        $rustDocument = $pair.Rust
        $key = $sourceDocument.Asset + '|' + $sourceDocument.ClassID + '|' + $sourceDocument.Name
        $sourceReferences = [string[]] @(Map-SourceReferences $sourceDocument.References $DocumentPairing.SourcePointerToRust)
        $rustReferences = [string[]] @($rustDocument.References)
        $sourceRefs = [string]::Join("`n", $sourceReferences)
        $rustRefs = [string]::Join("`n", $rustReferences)
        if ($sourceRefs -ceq $rustRefs) {
            $referenceMatches.Add($key)
        } elseif ($referenceDifferences.Count -lt 100) {
            $referenceDifferences.Add([ordered] @{
                Object = $key
                SourceFileID = $sourceDocument.FileID
                RustFileID = $rustDocument.FileID
                PairingMethod = $pair.Method
                SourceReferenceCount = $sourceDocument.ReferenceCount
                RustReferenceCount = $rustDocument.ReferenceCount
                SourceReferences = @(Map-SourceReferences $sourceDocument.References $DocumentPairing.SourcePointerToRust | Select-Object -First 20)
                RustReferences = @($rustDocument.References | Select-Object -First 20)
            })
        }
        if (
            $SourceYaml.Documents.ContainsKey($sourceDocument.Key) -and
            $RustYaml.Documents.ContainsKey($sourceDocument.Key)
        ) {
            $rawSource = $SourceYaml.Documents[$sourceDocument.Key]
            $rawRust = $RustYaml.Documents[$sourceDocument.Key]
            if ([string]::Join("`n", $rawSource.References) -ceq [string]::Join("`n", $rawRust.References)) {
                $rawReferenceMatches.Add($sourceDocument.Key)
            }
        }
        if ($sourceDocument.FileID -cne $rustDocument.FileID -and $localFileIdMappings.Count -lt 100) {
            $localFileIdMappings.Add([ordered] @{
                Asset = $sourceDocument.Asset
                ClassID = $sourceDocument.ClassID
                Type = $sourceDocument.Type
                Name = $sourceDocument.Name
                SourceFileID = $sourceDocument.FileID
                RustFileID = $rustDocument.FileID
                PairingMethod = $pair.Method
            })
        }
        if ($sourceDocument.NullPointerCount -ne $rustDocument.NullPointerCount -and $nullPointerDifferences.Count -lt 100) {
            $nullPointerDifferences.Add([ordered] @{
                Object = $key
                SourceNullPointerCount = $sourceDocument.NullPointerCount
                RustNullPointerCount = $rustDocument.NullPointerCount
            })
        }
    }
    $classIds = @(@($SourceYaml.ClassCounts.Keys) + @($RustYaml.ClassCounts.Keys) | Sort-Object -Unique)
    $classCounts = @($classIds | ForEach-Object {
        [ordered] @{
            ClassID = $_
            Source = if ($SourceYaml.ClassCounts.ContainsKey($_)) { $SourceYaml.ClassCounts[$_] } else { 0 }
            Rust = if ($RustYaml.ClassCounts.ContainsKey($_)) { $RustYaml.ClassCounts[$_] } else { 0 }
        }
    })
    $typeNames = @(@($SourceYaml.TypeCounts.Keys) + @($RustYaml.TypeCounts.Keys) | Sort-Object -Unique)
    $typeCounts = @($typeNames | ForEach-Object {
        [ordered] @{
            Type = $_
            Source = if ($SourceYaml.TypeCounts.ContainsKey($_)) { $SourceYaml.TypeCounts[$_] } else { 0 }
            Rust = if ($RustYaml.TypeCounts.ContainsKey($_)) { $RustYaml.TypeCounts[$_] } else { 0 }
        }
    })
    return [ordered] @{
        SourceDocuments = $sourceKeys.Count
        RustDocuments = $rustKeys.Count
        RawExactDocumentIdentityMatches = $rawMatched.Count
        MatchedDocuments = $DocumentPairing.Pairs.Count
        SourceOnlyDocuments = @($sourceKeys | Where-Object { -not $DocumentPairing.SourceToRust.ContainsKey($_) } | Select-Object -First 100)
        RustOnlyDocuments = @($rustKeys | Where-Object { -not $DocumentPairing.RustToSource.ContainsKey($_) } | Select-Object -First 100)
        LocalFileIDMappingCount = @($DocumentPairing.Pairs | Where-Object { $_.Source.FileID -cne $_.Rust.FileID }).Count
        LocalFileIDMappingSamples = $localFileIdMappings
        ExactReferenceMatches = $referenceMatches.Count
        RawExactReferenceMatches = $rawReferenceMatches.Count
        ReferenceDifferences = $referenceDifferences
        NullPointerCountDifferences = $nullPointerDifferences
        ClassCounts = $classCounts
        TypeCounts = $typeCounts
    }
}

$sourceInventory = Get-ProjectInventory $sourceRoot
$rustInventory = Get-ProjectInventory $rustRoot
$pairing = New-AssetPairing $sourceInventory $rustInventory
$sourcePathIds = Get-PathIdInventory $sourceRoot
$rustPathIds = Get-PathIdInventory $rustRoot

$inventoryLines = & cargo run --locked --quiet --manifest-path (Join-Path $repoRoot 'Cargo.toml') --bin unityfs_inventory -- $bundlePath
if ($LASTEXITCODE -ne 0) {
    throw "unityfs-inventory failed with exit code $LASTEXITCODE."
}
$bundleRows = @($inventoryLines | ForEach-Object { $_ | ConvertFrom-Json })
$bundleHeader = $bundleRows | Where-Object { $_.kind -eq 'header' } | Select-Object -First 1
$bundleEntries = @($bundleRows | Where-Object { $_.kind -eq 'entry' })
if ($null -eq $bundleHeader) {
    throw 'unityfs-inventory did not return a bundle header.'
}

$sourceYaml = Get-YamlDocuments $sourceRoot 'source' $sourceInventory $pairing
$rustYaml = Get-YamlDocuments $rustRoot 'rust' $rustInventory $pairing
$yamlDocumentPairing = New-YamlDocumentPairing $sourceYaml $rustYaml
$sourceFilePaths = @($sourceInventory.Files.Keys)
$rustFilePaths = @($rustInventory.Files.Keys)
$commonFilePaths = @($sourceFilePaths | Where-Object { $rustInventory.Files.ContainsKey($_) })
$missingRustPaths = @($sourceFilePaths | Where-Object { -not $rustInventory.Files.ContainsKey($_) })
$extraRustPaths = @($rustFilePaths | Where-Object { -not $sourceInventory.Files.ContainsKey($_) })
$sameSizePaths = @($commonFilePaths | Where-Object { $sourceInventory.Files[$_] -eq $rustInventory.Files[$_] })
$changedGuidPairs = [System.Collections.Generic.List[object]]::new()
$sameGuidPairs = 0
$mainFileIdDifferences = [System.Collections.Generic.List[object]]::new()
foreach ($sourceAsset in $pairing.SourceToRust.Keys) {
    $rustAsset = $pairing.SourceToRust[$sourceAsset]
    $sourceMeta = $sourceInventory.Metas[$sourceAsset]
    $rustMeta = $rustInventory.Metas[$rustAsset]
    if ($sourceMeta.Guid -eq $rustMeta.Guid) {
        $sameGuidPairs += 1
    } elseif ($changedGuidPairs.Count -lt 100) {
        $changedGuidPairs.Add([ordered] @{
            SourceAsset = $sourceAsset
            RustAsset = $rustAsset
            SourceGUID = $sourceMeta.Guid
            RustGUID = $rustMeta.Guid
        })
    }
    if ($sourceMeta.MainObjectFileID -ne $rustMeta.MainObjectFileID -and $mainFileIdDifferences.Count -lt 100) {
        $mainFileIdDifferences.Add([ordered] @{
            SourceAsset = $sourceAsset
            RustAsset = $rustAsset
            SourceFileID = $sourceMeta.MainObjectFileID
            RustFileID = $rustMeta.MainObjectFileID
        })
    }
}

$report = [ordered] @{
    Input = [ordered] @{
        Path = $bundlePath
        Bytes = (Get-Item -LiteralPath $bundlePath).Length
        SHA256 = (Get-FileHash -LiteralPath $bundlePath -Algorithm SHA256).Hash.ToLowerInvariant()
        UnityVersion = $bundleHeader.unityVersion
        UnityRevision = $bundleHeader.unityRevision
    }
    Bundle = [ordered] @{
        Header = $bundleHeader
        EntryCount = $bundleEntries.Count
        SerializedFileFlagCount = @($bundleEntries | Where-Object { $_.serializedFileFlag }).Count
        Entries = @($bundleEntries | ForEach-Object {
            [ordered] @{
                Index = $_.index
                Path = $_.path
                Offset = $_.offset
                Size = $_.size
                Flags = $_.flags
                SerializedFileFlag = $_.serializedFileFlag
            }
        })
    }
    SerializedFilesAndBundleAssets = (Compare-PathIdRows $sourcePathIds $rustPathIds)
    OutputPaths = [ordered] @{
        SourceFiles = $sourceFilePaths.Count
        RustFiles = $rustFilePaths.Count
        SameRelativePath = $commonFilePaths.Count
        SameSizeAtSamePath = $sameSizePaths.Count
        DifferentSizeAtSamePath = $commonFilePaths.Count - $sameSizePaths.Count
        MissingFromRust = $missingRustPaths.Count
        ExtraInRust = $extraRustPaths.Count
        MissingPathSamples = @($missingRustPaths | Select-Object -First 100)
        ExtraPathSamples = @($extraRustPaths | Select-Object -First 100)
    }
    Metadata = [ordered] @{
        SourceGUIDCount = $sourceInventory.Metas.Count
        RustGUIDCount = $rustInventory.Metas.Count
        PairedAssets = $pairing.SourceToRust.Count
        ExactPathPairs = $pairing.Exact
        ExtensionChangedPairs = $pairing.ExtensionChanged
        SameGUIDPairs = $sameGuidPairs
        ChangedGUIDSamples = $changedGuidPairs
        MainObjectFileIDDifferences = $mainFileIdDifferences
        UnpairedSourceAssets = @($pairing.UnpairedSource | Select-Object -First 100)
        UnpairedRustAssets = @($pairing.UnpairedRust | Select-Object -First 100)
        AmbiguousAssetPairs = $pairing.Ambiguous
    }
    YAML = (Compare-Yaml $sourceYaml $rustYaml $yamlDocumentPairing)
    Normalization = @(
        'Assets pair by exact relative path, then by same folder and basename with extension ignored.',
        'PPtrs compare as canonical asset path plus local fileID; unmapped GUIDs remain explicit.',
        'Documents pair by asset path and ordered ClassID, root type, and m_Name signatures, allowing source-generated local fileIDs to map to Rust local fileIDs within each asset.',
        'The report retains raw fileID and raw reference matches alongside the mapped pointer comparison.',
        'Non-reference YAML field values, binary payload semantics, Unity import behavior, and animation sampling are not compared by this harness.'
    )
}

$json = $report | ConvertTo-Json -Depth 100
[System.IO.File]::WriteAllText($reportFullPath, $json + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
Write-Output "Wrote real-project differential report: $reportFullPath"
Write-Output ("Bundle entries: {0}; serialized asset rows: {1}/{2}; YAML docs: {3}/{4}; matched docs: {5}; mapped refs: {6}; raw refs: {7}; path pairs: {8}; missing/extra paths: {9}/{10}" -f `
    $bundleEntries.Count, $sourcePathIds.Rows.Count, $rustPathIds.Rows.Count, $sourceYaml.Documents.Count, $rustYaml.Documents.Count, `
    $report.YAML.MatchedDocuments, $report.YAML.ExactReferenceMatches, $report.YAML.RawExactReferenceMatches, `
    $pairing.SourceToRust.Count, $missingRustPaths.Count, $extraRustPaths.Count)
