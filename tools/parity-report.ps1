# Machine-readable completeness report for docs/feature-parity.md.
#
# Parses the ledger table and reports status tallies, non-verified IDs, duplicate stable
# IDs, and malformed rows as JSON. Row fields may contain literal pipes, so the status is
# located by matching parts against the closed status vocabulary (plus the legacy ???
# placeholder) instead of by fixed column position.
#
# Exit code is 0 only when every ledger row is verified; any other state (including ???
# placeholders, uninvestigated rows, duplicates, or malformed rows) exits 1 with the
# details in the JSON payload. "Not run" is never reported as passed.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File tools/parity-report.ps1 [-OutFile report.json]

param(
    [string]$OutFile = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$ledgerPath = Join-Path $repoRoot "docs/feature-parity.md"

$vocabulary = @(
    "未调查",
    "已调查",
    "实现中",
    "已实现但未验证",
    "已验证",
    "受外部条件阻塞"
)

$lines = [System.IO.File]::ReadAllLines($ledgerPath, [System.Text.Encoding]::UTF8)
$rows = @()
$malformed = @()
foreach ($line in $lines) {
    if ($line -match '^\| [A-Z]+-[A-Z0-9-]+ \|') {
        $rows += $line
    }
}

$byStatus = @{}
$nonVerified = @()
$seen = @{}
$duplicates = @()

foreach ($row in $rows) {
    $parts = $row -split '\|'
    $id = if ($parts.Count -ge 2) { $parts[1].Trim() } else { "" }
    if ($id -eq "") {
        $malformed += $row
        continue
    }
    if ($seen.ContainsKey($id)) {
        if (-not ($duplicates -contains $id)) { $duplicates += $id }
    } else {
        $seen[$id] = $true
    }
    $status = $null
    foreach ($part in $parts) {
        $trimmed = $part.Trim()
        if ($trimmed -eq "???") {
            # Legacy placeholder predating UTF-8-safe ledger writes; never a pass.
            $status = "???"
            break
        }
        if ($vocabulary -contains $trimmed) {
            $status = $trimmed
            break
        }
    }
    if ($null -eq $status) {
        $malformed += $id
        continue
    }
    if ($byStatus.ContainsKey($status)) { $byStatus[$status]++ } else { $byStatus[$status] = 1 }
    if ($status -ne "已验证") {
        $nonVerified += @{ id = $id; status = $status }
    }
}

$gateOpen = ($nonVerified.Count -gt 0) -or ($duplicates.Count -gt 0) -or ($malformed.Count -gt 0)

$report = [ordered]@{
    ledger = "docs/feature-parity.md"
    total_rows = $rows.Count
    by_status = $byStatus
    non_verified_count = $nonVerified.Count
    non_verified = $nonVerified
    duplicate_ids = $duplicates
    malformed = $malformed
    phase1_gate = if ($gateOpen) { "OPEN" } else { "PASSED" }
}

$json = $report | ConvertTo-Json -Depth 5
if ($OutFile -ne "") {
    [System.IO.File]::WriteAllText(
        $OutFile, $json + [System.Environment]::NewLine,
        [System.Text.UTF8Encoding]::new($false))
}
Write-Output $json

if ($gateOpen) { exit 1 } else { exit 0 }
