# Compares PNG pixel content (dimensions, format, decoded-pixel SHA-256) for every
# `.png` pair listed in a project-audit report's OpaqueDifferent set.
#
# Byte differences with identical pixels are PNG-encoder-only and semantically
# equivalent for Unity import. Pixel differences point at decode/unpack bugs
# (proven once: DXT5nm unpack skipped over 33 normal maps).
#
# Usage (PowerShell 5.1-safe; use short junctions for 260+ char paths):
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools/compare-png-pixels.ps1 `
#     -AuditJson <audit.json> -SourceRoot <dir> -RustRoot <dir> [-MaxSamples 10]
param([string]$AuditJson, [string]$SourceRoot, [string]$RustRoot, [int]$MaxSamples = 10)
Add-Type -AssemblyName System.Drawing
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$ws = Split-Path -Parent $here
Add-Type -Path (Join-Path $here "PngPixelHash.cs") -ReferencedAssemblies System.Drawing, System.Security
$r = Get-Content $AuditJson -Raw | ConvertFrom-Json
$pngs = @($r.OpaqueDifferent | Where-Object { $_ -like "*.png" })
$samePx = 0; $diffPx = 0; $diffFmt = 0; $diffDim = 0; $checked = 0; $errs = 0; $samples = @()
foreach ($rel in $pngs) {
    $parts = $rel -split "/"
    $s = Join-Path $SourceRoot ([string]::Join("\", $parts))
    $t = Join-Path $RustRoot ([string]::Join("\", $parts))
    try {
        $a = [System.Drawing.Bitmap]::FromFile($s)
        $b = [System.Drawing.Bitmap]::FromFile($t)
        if ($a.Width -ne $b.Width -or $a.Height -ne $b.Height) { $diffDim++ }
        elseif ($a.PixelFormat -ne $b.PixelFormat) { $diffFmt++ }
        else {
            $ha = [PngPixelHash]::Hash($a); $hb = [PngPixelHash]::Hash($b)
            if ($ha -ceq $hb) { $samePx++ } else { $diffPx++; if ($samples.Count -lt $MaxSamples) { $samples += $rel } }
        }
        $a.Dispose(); $b.Dispose(); $checked++
    } catch { $errs++ }
}
Write-Output ("checked=" + $checked + " samePx=" + $samePx + " diffPx=" + $diffPx + " diffFmt=" + $diffFmt + " diffDim=" + $diffDim + " errs=" + $errs)
Write-Output "samples:"; $samples
