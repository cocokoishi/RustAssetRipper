param(
    [string]$BaselineRoot = 'C:\Users\koishi\Desktop\RustAssetripper\AssetRipper'
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path.TrimEnd('\')
$manifestPath = Join-Path $repositoryRoot 'docs\baseline-files.sha256'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "Baseline manifest not found inside RustAssetRipper: $manifestPath"
}

$baselineRoot = (Resolve-Path -LiteralPath $BaselineRoot).Path.TrimEnd('\')
$baselinePrefix = $baselineRoot + [System.IO.Path]::DirectorySeparatorChar
$expected = @{}
foreach ($line in Get-Content -LiteralPath $manifestPath -Encoding utf8) {
    if ($line -notmatch '^([0-9a-fA-F]{64})  (.+)$') {
        throw "Malformed checksum manifest line: $line"
    }

    $hash = $Matches[1].ToLowerInvariant()
    $relativePath = $Matches[2]
    $fullPath = [System.IO.Path]::GetFullPath(
        (Join-Path $baselineRoot ($relativePath.Replace('/', '\')))
    )
    if (-not $fullPath.StartsWith($baselinePrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Manifest path escapes the baseline directory: $relativePath"
    }
    if ($expected.ContainsKey($relativePath)) {
        throw "Duplicate manifest path: $relativePath"
    }
    $expected[$relativePath] = $hash
}

$issues = [System.Collections.Generic.List[string]]::new()
$actualFiles = Get-ChildItem -LiteralPath $baselineRoot -Recurse -File -Force
$actualPaths = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::OrdinalIgnoreCase
)
foreach ($file in $actualFiles) {
    $relativePath = $file.FullName.Substring($baselineRoot.Length + 1).Replace('\', '/')
    [void]$actualPaths.Add($relativePath)
    if (-not $expected.ContainsKey($relativePath)) {
        $issues.Add("Unlisted file: $relativePath")
        continue
    }

    $actualHash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $expected[$relativePath]) {
        $issues.Add("Changed file: $relativePath")
    }
}

foreach ($relativePath in $expected.Keys) {
    if (-not $actualPaths.Contains($relativePath)) {
        $issues.Add("Missing file: $relativePath")
    }
}

if ($issues.Count -gt 0) {
    $issues | ForEach-Object { Write-Error $_ -ErrorAction Continue }
    exit 1
}

Write-Output "Verified $($expected.Count) baseline files; no missing, changed, or unlisted files."
exit 0
