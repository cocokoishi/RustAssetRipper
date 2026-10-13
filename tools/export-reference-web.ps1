# Development oracle only: drive the frozen AssetRipper Web entry point.
# The Rust product never invokes this script or the reference executable.
# Refuse existing output/evidence paths because the reference's headless
# export handler otherwise deletes a nonempty destination automatically.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ReferenceExecutable,
    [Parameter(Mandatory = $true)][string]$InputPath,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [Parameter(Mandatory = $true)][string]$EvidenceDirectory,
    [ValidateRange(1, 65535)][int]$Port = 57611,
    [ValidateRange(1, 14400)][int]$RequestTimeoutSeconds = 1800
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Net.Http
$referenceExe = (Resolve-Path -LiteralPath $ReferenceExecutable).ProviderPath
$inputFile = (Resolve-Path -LiteralPath $InputPath).ProviderPath
if (-not (Test-Path -LiteralPath $referenceExe -PathType Leaf)) { throw 'Reference executable is not a file.' }
if (-not (Test-Path -LiteralPath $inputFile -PathType Leaf)) { throw 'Input must be a file.' }
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
$evidenceRoot = [IO.Path]::GetFullPath($EvidenceDirectory)
if (Test-Path -LiteralPath $outputRoot) { throw "Output already exists: $outputRoot" }
if (Test-Path -LiteralPath $evidenceRoot) { throw "Evidence directory already exists: $evidenceRoot" }
if ($outputRoot -eq $evidenceRoot -or $evidenceRoot.StartsWith($outputRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Evidence must be outside the export output.'
}

# Check for an existing listener before launching our own process.
$listener = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, $Port)
try { $listener.Start() } finally { $listener.Stop() }
New-Item -ItemType Directory -Path $evidenceRoot | Out-Null
$serverLog = Join-Path $evidenceRoot 'reference.log'
$stdoutLog = Join-Path $evidenceRoot 'stdout.log'
$stderrLog = Join-Path $evidenceRoot 'stderr.log'
$reportPath = Join-Path $evidenceRoot 'report.json'
$inputHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $inputFile).Hash
$referenceDll = [IO.Path]::ChangeExtension($referenceExe, '.dll')
$settingsPath = Join-Path ([IO.Path]::GetDirectoryName($referenceExe)) 'AssetRipper.Settings.json'
$report = [ordered]@{
    purpose = 'Frozen reference development oracle; not a Rust runtime dependency'
    startedUtc = [DateTime]::UtcNow.ToString('o')
    referenceExecutable = $referenceExe
    referenceExecutableSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $referenceExe).Hash
    referenceDllSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $referenceDll).Hash
    referenceVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($referenceExe).FileVersion
    settingsFile = $(if (Test-Path -LiteralPath $settingsPath) { $settingsPath } else { $null })
    inputPath = $inputFile
    inputBytes = (Get-Item -LiteralPath $inputFile).Length
    inputSha256 = $inputHash
    outputDirectory = $outputRoot
    evidenceDirectory = $evidenceRoot
    baseUrl = "http://127.0.0.1:$Port"
    launchArguments = @('--headless', '--port', "$Port", '--log-path', $serverLog)
    requestTimeoutSeconds = $RequestTimeoutSeconds
    processId = $null
    requests = @()
    status = 'starting'
    failure = $null
}
function Save-Report {
    $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath -Encoding UTF8
}
Save-Report
$handler = New-Object Net.Http.HttpClientHandler
$handler.AllowAutoRedirect = $false
$client = New-Object Net.Http.HttpClient($handler)
$client.Timeout = [TimeSpan]::FromSeconds($RequestTimeoutSeconds)
$referenceProcess = $null
function Send-Form([string]$Route, [hashtable]$Fields) {
    $pairs = foreach ($entry in $Fields.GetEnumerator()) {
        [Uri]::EscapeDataString($entry.Key) + '=' + [Uri]::EscapeDataString([string]$entry.Value)
    }
    $requestRecord = [ordered]@{ route = $Route; fields = $Fields; startedUtc = [DateTime]::UtcNow.ToString('o'); status = 'running' }
    $report.requests += $requestRecord
    Save-Report
    Write-Output "POST $Route started $($requestRecord.startedUtc)"
    $content = New-Object Net.Http.StringContent(($pairs -join '&'), [Text.Encoding]::UTF8, 'application/x-www-form-urlencoded')
    try {
        $response = $client.PostAsync($report.baseUrl + $Route, $content).GetAwaiter().GetResult()
        try {
            $body = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            $requestRecord['httpStatus'] = [int]$response.StatusCode
            $requestRecord['location'] = [string]$response.Headers.Location
            $requestRecord['completedUtc'] = [DateTime]::UtcNow.ToString('o')
            $requestRecord.status = 'completed'
            Save-Report
            Write-Output "POST $Route HTTP $([int]$response.StatusCode) completed $($requestRecord.completedUtc)"
            if ([int]$response.StatusCode -ne 302 -or [string]$response.Headers.Location -ne '/') {
                $body | Set-Content -LiteralPath (Join-Path $evidenceRoot 'unexpected-response.txt') -Encoding UTF8
                throw "Reference $Route returned HTTP $([int]$response.StatusCode), expected redirect to /."
            }
        } finally { $response.Dispose() }
    } finally { $content.Dispose() }
}
try {
    # Start-Process requires explicit quoting for a path argument containing spaces.
    $referenceProcess = Start-Process -FilePath $referenceExe -ArgumentList @('--headless', '--port', "$Port", '--log-path', ('"' + $serverLog + '"')) -WorkingDirectory $evidenceRoot -WindowStyle Hidden -RedirectStandardOutput $stdoutLog -RedirectStandardError $stderrLog -PassThru
    $report.processId = $referenceProcess.Id
    Save-Report
    $startup = [Diagnostics.Stopwatch]::StartNew()
    $ready = $false
    while ($startup.Elapsed.TotalSeconds -lt 60) {
        $referenceProcess.Refresh()
        if ($referenceProcess.HasExited) { throw "Reference exited during startup: $($referenceProcess.ExitCode). See $stderrLog" }
        try {
            $response = $client.GetAsync($report.baseUrl + '/Commands').GetAwaiter().GetResult()
            try { $ready = [int]$response.StatusCode -eq 200 } finally { $response.Dispose() }
        } catch { $ready = $false }
        if ($ready) { break }
        Start-Sleep -Milliseconds 200
    }
    if (-not $ready) { throw 'Reference did not become ready within 60 seconds.' }
    $settingsResponse = $client.GetAsync($report.baseUrl + '/Settings/Edit').GetAwaiter().GetResult()
    try {
        if ([int]$settingsResponse.StatusCode -ne 200) { throw 'Could not capture reference settings.' }
        $settingsResponse.Content.ReadAsStringAsync().GetAwaiter().GetResult() | Set-Content -LiteralPath (Join-Path $evidenceRoot 'settings.html') -Encoding UTF8
    } finally { $settingsResponse.Dispose() }
    $report.status = 'loading'
    Save-Report
    Send-Form '/LoadFile' @{ Path = $inputFile }
    $report.status = 'exporting'
    Save-Report
    # Recheck immediately before the reference handler can delete a directory.
    if (Test-Path -LiteralPath $outputRoot) { throw "Output appeared before export: $outputRoot" }
    Send-Form '/Export/UnityProject' @{ Path = $outputRoot; CreateSubfolder = 'false' }
    $referenceText = [IO.File]::ReadAllText($serverLog)
    if ($referenceText -notmatch 'Finished exporting assets' -or $referenceText -notmatch 'Finished post-export') {
        throw 'HTTP request completed but export completion markers are missing from the reference log.'
    }
    $projectVersionFile = Join-Path $outputRoot 'ExportedProject/ProjectSettings/ProjectVersion.txt'
    if (-not (Test-Path -LiteralPath $projectVersionFile -PathType Leaf)) { throw 'Exported project version file is missing.' }
    $report['projectVersion'] = [IO.File]::ReadAllText($projectVersionFile)
    $report['inputSha256After'] = (Get-FileHash -Algorithm SHA256 -LiteralPath $inputFile).Hash
    if ($report.inputSha256After -ne $inputHash) { throw 'Input changed during validation.' }
    $report.status = 'export-completed-semantic-comparison-pending'
    Write-Output "Reference export completed: $outputRoot"
} catch {
    $report.status = 'failed'
    $report.failure = $_.Exception.ToString()
    throw
} finally {
    if ($null -ne $referenceProcess) {
        $referenceProcess.Refresh()
        if (-not $referenceProcess.HasExited) {
            # Stop only the process created above, after the HTTP operation ends.
            Stop-Process -Id $referenceProcess.Id
            $report['processStoppedByTool'] = $true
        } else {
            $report['processExitCode'] = $referenceProcess.ExitCode
        }
    }
    $client.Dispose()
    $handler.Dispose()
    $report['finishedUtc'] = [DateTime]::UtcNow.ToString('o')
    Save-Report
}
