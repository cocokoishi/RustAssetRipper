[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$scanner = Join-Path $repository 'tools/check-project-references.ps1'
$fixtures = Join-Path $repository ('target/reference-check-contract-' + [guid]::NewGuid().ToString('N'))
[void][System.IO.Directory]::CreateDirectory($fixtures)
$encoding = [System.Text.UTF8Encoding]::new($false)
$passed = 0

function Run-Case([string]$Name, [hashtable]$Files, [int]$Exit, [scriptblock]$AssertReport) {
    $caseRoot = Join-Path $fixtures $Name
    [void][System.IO.Directory]::CreateDirectory($caseRoot)
    foreach ($entry in $Files.GetEnumerator()) {
        $path = Join-Path $caseRoot $entry.Key
        [void][System.IO.Directory]::CreateDirectory([System.IO.Path]::GetDirectoryName($path))
        [System.IO.File]::WriteAllText($path, $entry.Value, $encoding)
    }
    $reportPath = Join-Path $fixtures ($Name + '.json')
    $output = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $scanner -ProjectRoot $caseRoot -ReportPath $reportPath 2>&1
    if ($LASTEXITCODE -ne $Exit) { throw "$Name expected exit $Exit, got $LASTEXITCODE : $output" }
    $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    & $AssertReport $report
    $script:passed++
    "$Name passed"
}
function Require($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }

$prefix = "%YAML 1.1`n%TAG !u! tag:unity3d.com,2011:`n--- !u!91 &9100000`nAnimatorController:`n"
$meta = "fileFormatVersion: 2`nguid: 11111111111111111111111111111111`nNativeFormatImporter:`n  mainObjectFileID: 9100000`n"
Run-Case 'valid-local-null-negative-stripped' @{
    'Assets/valid.controller' = $prefix + "  m_State: {fileID: -7}`n  m_Null: {fileID: 0}`n--- !u!1102 &-7 stripped`nAnimatorState:`n  m_Controller: {fileID: 9100000}`n"
    'Assets/valid.controller.meta' = $meta
} 0 { param($r) Require ($r.Status -eq 'PASS' -and $r.LocalChecked -eq 2 -and $r.NullReferences -eq 1) 'Valid local IDs or stripped header not checked' }
Run-Case 'dangling-local' @{ 'Assets/bad.controller' = $prefix + "  m_State: {fileID: 777}`n" } 1 {
    param($r) Require ($r.Issues.Count -eq 1 -and $r.Issues[0].Kind -eq 'DanglingLocalReference') 'Missing local reference was hidden'
}
Run-Case 'same-id-other-file' @{
    'Assets/bad.controller' = $prefix + "  m_State: {fileID: 777}`n"
    'Assets/other.state' = "%YAML 1.1`n--- !u!1102 &777`nAnimatorState: {}`n"
} 1 { param($r) Require ($r.Issues[0].Kind -eq 'DanglingLocalReference') 'File-local scope escaped' }
Run-Case 'valid-external' @{
    'Assets/from.controller' = $prefix + "  m_Other: {fileID: 9100000, guid: 11111111111111111111111111111111, type: 2}`n"
    'Assets/to.controller' = $prefix
    'Assets/to.controller.meta' = $meta
} 0 { param($r) Require ($r.Status -eq 'PASS' -and $r.ExternalChecked -eq 1) 'External target document not checked' }
Run-Case 'dangling-external' @{
    'Assets/from.controller' = $prefix + "  m_Other: {fileID: 777, guid: 11111111111111111111111111111111, type: 2}`n"
    'Assets/to.controller' = $prefix
    'Assets/to.controller.meta' = $meta
} 1 { param($r) Require ($r.Issues[0].Kind -eq 'DanglingExternalReference') 'Missing external fileID accepted' }
Run-Case 'unmapped-guid' @{
    'Assets/from.controller' = $prefix + "  m_Other: {fileID: 777, guid: 22222222222222222222222222222222, type: 2}`n"
} 1 { param($r) Require ($r.Issues[0].Kind -eq 'UnmappedGuid') 'Unknown GUID accepted' }
Run-Case 'duplicate-guid' @{
    'Assets/a.controller' = $prefix; 'Assets/b.controller' = $prefix
    'Assets/a.controller.meta' = $meta; 'Assets/b.controller.meta' = $meta
} 1 { param($r) Require ($r.Issues[0].Kind -eq 'DuplicateGuid') 'Duplicate GUID overwritten' }
Run-Case 'duplicate-fileid-other-class' @{
    'Assets/bad.controller' = $prefix + "--- !u!1102 &9100000`nAnimatorState: {}`n"
} 1 { param($r) Require ($r.Issues[0].Kind -eq 'DuplicateFileId') 'Duplicate fileID accepted across class IDs' }
Run-Case 'missing-meta-owner' @{ 'Assets/missing.controller.meta' = $meta } 1 {
    param($r) Require ($r.Issues[0].Kind -eq 'MissingMetaOwner') 'Orphan meta accepted'
}
Run-Case 'builtins-and-imported-id-explicitly-unverified' @{
    'Assets/from.controller' = $prefix + "  m_Builtin: {fileID: 10303, guid: 0000000000000000f000000000000000, type: 0}`n  m_Texture: {fileID: 2800000, guid: 11111111111111111111111111111111, type: 3}`n"
    'Assets/image.png' = 'not YAML; fixture needs only owner existence'
    'Assets/image.png.meta' = $meta
} 0 { param($r) Require ($r.Status -eq 'PARTIAL' -and -not $r.CompleteReferenceValidation -and $r.UnverifiedTargets.Count -eq 2) 'Imported/builtin identity claimed complete' }
Run-Case 'scene-object-not-imported-scene-id' @{
    'Assets/from.controller' = $prefix + "  m_Object: {fileID: 777, guid: 11111111111111111111111111111111, type: 2}`n"
    'Assets/to.unity' = "%YAML 1.1`n--- !u!1 &1`nGameObject: {}`n"
    'Assets/to.unity.meta' = $meta
} 1 { param($r) Require ($r.Issues[0].Kind -eq 'DanglingExternalReference') 'Scene object IDs skipped' }
Run-Case 'malformed-pointer' @{ 'Assets/bad.controller' = $prefix + "  m_State: {fileID: 7, guid: bad, type: 2}`n" } 1 {
    param($r) Require ($r.Issues[0].Kind -eq 'UnsupportedPointerSyntax') 'Malformed pointer ignored'
}
$completeReport = Join-Path $fixtures 'require-complete.json'
$completeOutput = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $scanner -ProjectRoot (Join-Path $fixtures 'builtins-and-imported-id-explicitly-unverified') -ReportPath $completeReport -RequireComplete 2>&1
Require ($LASTEXITCODE -eq 2) "Incomplete required validation must exit 2: $completeOutput"
$strict = Get-Content -LiteralPath $completeReport -Raw | ConvertFrom-Json
Require ($strict.Status -eq 'PARTIAL' -and $strict.CompleteValidationRequired) 'Required validation scope not recorded'
$passed++
"$passed reference-check contract cases passed; evidence=$fixtures"
