[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$auditor = Join-Path $repository 'tools/audit-project-documents.ps1'
$fixtures = Join-Path $repository ('target/document-audit-contract-' + [guid]::NewGuid().ToString('N'))
$encoding = [Text.UTF8Encoding]::new($false)
$passed = 0
function Require($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Run-Case([string]$Name, [hashtable]$Source, [hashtable]$Rust, [int]$Exit, [scriptblock]$AssertReport) {
    $caseRoot = Join-Path $fixtures $Name
    foreach ($side in @('source','rust')) {
        $files = if ($side -eq 'source') { $Source } else { $Rust }
        $sideRoot = Join-Path $caseRoot $side
        [void][IO.Directory]::CreateDirectory((Join-Path $sideRoot 'Assets'))
        foreach ($entry in $files.GetEnumerator()) {
            $path = Join-Path $sideRoot $entry.Key
            [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path))
            [IO.File]::WriteAllText($path, $entry.Value, $encoding)
        }
    }
    $reportPath = Join-Path $caseRoot 'report.json'
    $output = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $auditor -SourceProject (Join-Path $caseRoot 'source') -RustProject (Join-Path $caseRoot 'rust') -ReportPath $reportPath -RequireEquivalent 2>&1
    if ($LASTEXITCODE -ne $Exit) { throw "$Name expected exit $Exit, got $LASTEXITCODE : $output" }
    $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    & $AssertReport $report
    $script:passed++
    "$Name passed"
}
$yaml = "%YAML 1.1`n--- !u!114 &11400000`nMonoBehaviour:`n  m_Name: Fixture`n"
$sourceMeta = "guid: 11111111111111111111111111111111`nNativeFormatImporter:`n  mainObjectFileID: 11400000`n"
$rustMeta = "guid: 22222222222222222222222222222222`nNativeFormatImporter:`n  mainObjectFileID: 11400000`n"
Run-Case 'guid-only-meta' @{ 'Assets/a.asset' = $yaml; 'Assets/a.asset.meta' = $sourceMeta } @{
    'Assets/a.asset' = $yaml; 'Assets/a.asset.meta' = $rustMeta
} 0 { param($r) Require ($r.EquivalentWithinComparisonScope -and $r.AdditionalFiles[0].Result -eq 'GUID_PATH_AND_EXPORT_TIME_EQUIVALENT') 'Proven GUID ownership not mapped' }
Run-Case 'export-timestamp-only' @{ 'Assets/a.asset' = $yaml; 'Assets/a.asset.meta' = $sourceMeta + "timeCreated: 1700000000`n" } @{
    'Assets/a.asset' = $yaml; 'Assets/a.asset.meta' = $rustMeta + "timeCreated: 1800000000`n"
} 0 { param($r) Require $r.EquivalentWithinComparisonScope 'Export timestamp not recognized' }
Run-Case 'missing-export-timestamp-field' @{ 'Assets/a.asset' = $yaml; 'Assets/a.asset.meta' = $sourceMeta + "timeCreated: 1700000000`n" } @{
    'Assets/a.asset' = $yaml; 'Assets/a.asset.meta' = $rustMeta
} 1 { param($r) Require (-not $r.EquivalentWithinComparisonScope) 'Timestamp field omission normalized away' }
Run-Case 'importer-content' @{ 'Assets/a.asset' = $yaml; 'Assets/a.asset.meta' = $sourceMeta } @{
    'Assets/a.asset' = $yaml; 'Assets/a.asset.meta' = $rustMeta.Replace('11400000','11400001')
} 1 { param($r) Require ($r.AdditionalDifferences.Count -eq 1 -and $r.AdditionalDifferences[0].Kind -eq 'META') 'Importer fileID difference ignored' }
Run-Case 'project-settings' @{ 'Assets/a.asset' = $yaml; 'ProjectSettings/EditorSettings.asset' = 'value: 1' } @{
    'Assets/a.asset' = $yaml; 'ProjectSettings/EditorSettings.asset' = 'value: 2'
} 1 { param($r) Require ($r.AdditionalDifferences.Count -eq 1 -and $r.AdditionalDifferences[0].Kind -eq 'PROJECT_OR_AUXILIARY') 'ProjectSettings difference ignored' }
Run-Case 'missing-packages' @{ 'Assets/a.asset' = $yaml; 'Packages/manifest.json' = '{"dependencies":{}}' } @{
    'Assets/a.asset' = $yaml
} 1 { param($r) Require ($r.AdditionalDifferences[0].Result -eq 'SOURCE_ONLY') 'Missing project support file ignored' }
Run-Case 'unpaired-documents' @{ 'Assets/a.asset' = $yaml } @{} 1 {
    param($r) Require ($r.Classes[0].Source -eq 1 -and $r.Classes[0].SourceOnly -eq 1) 'Unpaired YAML excluded from source totals'
}
Run-Case 'array-order' @{ 'Assets/a.asset' = $yaml + "  values: [1, 2]`n" } @{
    'Assets/a.asset' = $yaml + "  values: [2, 1]`n"
} 1 { param($r) Require ($r.Classes[0].Different -eq 1) 'Semantic array order normalized away' }
Run-Case 'opaque-bytes' @{ 'Assets/a.bytes' = 'ABC' } @{ 'Assets/a.bytes' = 'ABD' } 1 {
    param($r) Require ($r.OpaqueDifferent.Count -eq 1 -and -not $r.EquivalentWithinComparisonScope) 'Opaque mismatch called equivalent'
}
"$passed document-audit contract cases passed; evidence=$fixtures"
