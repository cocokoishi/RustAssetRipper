# Machine-readable completeness report for docs/feature-parity.md.
#
# Parses the ledger table and reports status tallies, non-verified IDs, duplicate stable
# IDs, and malformed rows as JSON. The status is read only from the column named "Status"
# in the table header. Escaped pipes are treated as cell content; row-width mismatches are
# reported instead of guessing where a missing or extra column belongs.
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

function Split-MarkdownTableRow([string]$Line) {
    $cells = [System.Collections.Generic.List[string]]::new()
    $builder = [System.Text.StringBuilder]::new()
    for ($index = 0; $index -lt $Line.Length; $index++) {
        $character = $Line[$index]
        if ($character -eq '|') {
            $cells.Add($builder.ToString().Trim())
            [void]$builder.Clear()
            continue
        }
        if ([int]$character -eq 92 -and $index + 1 -lt $Line.Length -and $Line[$index + 1] -eq '|') {
            [void]$builder.Append('|')
            $index++
            continue
        }
        [void]$builder.Append($character)
    }
    $cells.Add($builder.ToString().Trim())

    if ($cells.Count -gt 0 -and $cells[0] -eq '') { $cells.RemoveAt(0) }
    if ($cells.Count -gt 0 -and $cells[$cells.Count - 1] -eq '') { $cells.RemoveAt($cells.Count - 1) }
    return $cells.ToArray()
}

$headerLine = $lines | Where-Object { $_ -match '^\|\s*Stable ID\s*\|' } | Select-Object -First 1
if (-not $headerLine) { throw "Ledger header with a Stable ID column was not found: $ledgerPath" }
$headers = @(Split-MarkdownTableRow $headerLine)
$idColumn = [Array]::IndexOf($headers, 'Stable ID')
$statusColumn = [Array]::IndexOf($headers, 'Status')
if ($idColumn -lt 0 -or $statusColumn -lt 0) {
    throw 'Ledger must contain Stable ID and Status columns.'
}

$rows = @($lines | Where-Object { $_ -match '^\|\s*[A-Z][A-Z0-9-]+\s*\|' })
$byStatus = @{}
$nonVerified = @()
$seen = @{}
$duplicates = @()
$malformed = @()

foreach ($row in $rows) {
    $cells = @(Split-MarkdownTableRow $row)
    $id = if ($cells.Count -gt $idColumn) { $cells[$idColumn] } else { '' }
    if ($id -eq '') {
        $malformed += @{ id = ''; reason = 'missing_stable_id'; actual_cells = $cells.Count }
        continue
    }
    if ($seen.ContainsKey($id)) {
        if (-not ($duplicates -contains $id)) { $duplicates += $id }
    } else {
        $seen[$id] = $true
    }

    if ($cells.Count -ne $headers.Count) {
        $malformed += @{
            id = $id
            reason = 'cell_count_mismatch'
            expected_cells = $headers.Count
            actual_cells = $cells.Count
        }
    }

    # Read only the actual header-indexed Status cell. A malformed row can still have
    # that cell in the correct position (for example, an unescaped pipe later in its
    # Evidence prose); count the status and report the width defect independently.
    if ($cells.Count -le $statusColumn) { continue }
    $status = $cells[$statusColumn]
    if ($vocabulary -notcontains $status) {
        if ($cells.Count -eq $headers.Count) {
            $malformed += @{
                id = $id
                reason = 'invalid_status_cell'
                status_cell = $status
            }
        }
        continue
    }
    if ($byStatus.ContainsKey($status)) { $byStatus[$status]++ } else { $byStatus[$status] = 1 }
    if ($status -ne '已验证') { $nonVerified += @{ id = $id; status = $status } }
}

$gateOpen = ($nonVerified.Count -gt 0) -or ($duplicates.Count -gt 0) -or ($malformed.Count -gt 0)

$report = [ordered]@{
    ledger = "docs/feature-parity.md"
    total_rows = $rows.Count
    status_column = $headers[$statusColumn]
    recognized_status_rows = [int](($byStatus.Values | Measure-Object -Sum).Sum)
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
