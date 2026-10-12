# Compares Ogg packet streams (headers plus audio payloads) for every `.ogg`
# pair listed in a project-audit report's OpaqueDifferent set.
#
# Identical packet streams with differing bytes isolate Ogg page framing
# (page grouping, checksums) from audio content: Rust remuxes original FSB5
# Vorbis packets verbatim, so packet identity proves content parity.
#
# Usage (PowerShell 5.1-safe; use short junctions for 260+ char paths):
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools/compare-ogg-packets.ps1 `
#     -AuditJson <audit.json> -SourceRoot <dir> -RustRoot <dir>
param([string]$AuditJson, [string]$SourceRoot, [string]$RustRoot)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Add-Type -Path (Join-Path $here "OggPacketHash.cs") -ReferencedAssemblies System.Security
$r = Get-Content $AuditJson -Raw | ConvertFrom-Json
$oggs = @($r.OpaqueDifferent | Where-Object { $_ -like "*.ogg" })
Write-Output ("ogg pairs=" + $oggs.Count)
$allIdentical = $true
foreach ($rel in $oggs) {
    $parts = $rel -split "/"
    $s = Join-Path $SourceRoot ([string]::Join("\", $parts))
    $t = Join-Path $RustRoot ([string]::Join("\", $parts))
    try {
        $a = [System.IO.File]::ReadAllBytes($s)
        $b = [System.IO.File]::ReadAllBytes($t)
        $pa = [OggPacketHash]::Packets($a); $pb = [OggPacketHash]::Packets($b)
        $same = ($pa.Count -eq $pb.Count) -and ([OggPacketHash]::Sha256Many($pa, 0, $pa.Count) -ceq [OggPacketHash]::Sha256Many($pb, 0, $pb.Count))
        if (-not $same) { $allIdentical = $false }
        Write-Output (($rel -split "/")[-1] + " packets=" + $pa.Count + "/" + $pb.Count + " identical=" + $same)
    } catch { Write-Output (($rel -split "/")[-1] + " ERROR " + $_.Exception.Message); $allIdentical = $false }
}
Write-Output ("ALL_IDENTICAL=" + $allIdentical)
