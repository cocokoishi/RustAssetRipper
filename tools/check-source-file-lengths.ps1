param(
    [ValidateRange(1, 100000)]
    [int]$Limit = 2000
)

$root = Split-Path -Parent $PSScriptRoot
$sourceGlobs = @(
    '*.rs', '*.cs', '*.ps1', '*.py', '*.js', '*.jsx', '*.ts', '*.tsx', '*.sh',
    '*.c', '*.h', '*.hpp', '*.cpp', '*.java', '*.go', '*.sql', '*.vue', '*.html', '*.css'
)
$exitCode = 0

Push-Location $root
try {
    $rgArguments = @('--files')
    foreach ($glob in $sourceGlobs) {
        $rgArguments += @('-g', $glob)
    }

    $sourceFiles = @(& rg @rgArguments)
    if ($LASTEXITCODE -gt 1) {
        throw "rg failed while listing source files (exit $LASTEXITCODE)."
    }

    $oversizedFiles = foreach ($sourceFile in $sourceFiles) {
        $sourceLineCount = [System.IO.File]::ReadAllLines((Resolve-Path -LiteralPath $sourceFile)).Length
        if ($sourceLineCount -gt $Limit) {
            [pscustomobject]@{
                Lines = $sourceLineCount
                Path  = $sourceFile.Replace('\', '/')
            }
        }
    }

    $ownedFiles = @($oversizedFiles | Where-Object { $_.Path -notmatch '^vendor/' })
    $vendoredFiles = @($oversizedFiles | Where-Object { $_.Path -match '^vendor/' })

    if ($ownedFiles.Count -gt 0) {
        Write-Host "First-party source files over $Limit lines:"
        $ownedFiles | Sort-Object Lines -Descending | Format-Table -AutoSize
        $exitCode = 1
    } else {
        Write-Host "No first-party source files exceed $Limit lines."
    }

    if ($vendoredFiles.Count -gt 0) {
        Write-Host 'Vendored third-party files over the limit (reported, not modified):'
        $vendoredFiles | Sort-Object Lines -Descending | Format-Table -AutoSize
    }
} catch {
    Write-Error $_
    $exitCode = 2
} finally {
    Pop-Location
}

exit $exitCode
