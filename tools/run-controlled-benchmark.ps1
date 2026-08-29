param(
    [int[]]$Fps = @(15, 30),
    [int]$Seconds = 10,
    [string[]]$Scenario = @('static', 'cursor', 'small', 'typing', 'scroll', 'window-move', 'full')
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$bin = Join-Path $root 'target\release\examples'
$stimulus = Join-Path $bin 'controlled_stimulus.exe'
$poll = Join-Path $bin 'poll_updates.exe'
if (!(Test-Path $stimulus) -or !(Test-Path $poll)) {
    throw 'Build examples first: cargo build --release --examples'
}
$resultDir = Join-Path $root 'target\bench-results'
New-Item -ItemType Directory -Force -Path $resultDir | Out-Null
$rows = foreach ($name in $Scenario) {
    foreach ($rate in $Fps) {
        $stimulusInfo = [Diagnostics.ProcessStartInfo]::new($stimulus, "$name $($Seconds + 2)")
        $stimulusInfo.WorkingDirectory = $root
        $stimulusInfo.UseShellExecute = $false
        $stimulusProcess = [Diagnostics.Process]::Start($stimulusInfo)
        Start-Sleep -Milliseconds 700
        $pollInfo = [Diagnostics.ProcessStartInfo]::new($poll)
        $pollInfo.WorkingDirectory = $root
        $pollInfo.UseShellExecute = $false
        $pollInfo.RedirectStandardOutput = $true
        $pollInfo.Environment['SCREENDELTA_FPS'] = "$rate"
        $pollInfo.Environment['SCREENDELTA_SECONDS'] = "$Seconds"
        $watch = [Diagnostics.Stopwatch]::StartNew()
        $pollProcess = [Diagnostics.Process]::Start($pollInfo)
        $pollProcess.WaitForExit()
        $watch.Stop()
        $output = $pollProcess.StandardOutput.ReadToEnd().Trim()
        $stimulusProcess.WaitForExit()
        $stats = @{}
        [regex]::Matches($output, '(?<key>[a-z_]+)(?:=|: )(?<value>[0-9.]+)(?<unit>ms|B|µs)?') | ForEach-Object {
            $stats[$_.Groups['key'].Value] = $_.Groups['value'].Value
        }
        [pscustomobject]@{
            scenario = $name
            requested_fps = $rate
            seconds = $Seconds
            wall_ms = [math]::Round($watch.Elapsed.TotalMilliseconds, 3)
            cpu_ms = [math]::Round($pollProcess.TotalProcessorTime.TotalMilliseconds, 3)
            cpu_percent_one_core = [math]::Round(100 * $pollProcess.TotalProcessorTime.TotalMilliseconds / $watch.Elapsed.TotalMilliseconds, 3)
            full = $stats['full']
            delta = $stats['delta']
            unchanged = $stats['unchanged']
            delta_bytes = $stats['delta_bytes']
            os_frames_acquired = $stats['os_frames_acquired']
            full_payload_bytes = $stats['full_payload_bytes']
            delta_payload_bytes = $stats['delta_payload_bytes']
            move_rects_observed = $stats['move_rects_observed']
            pointer_updates = $stats['pointer_updates']
            separate_pointer_updates = $stats['separate_pointer_updates']
            pointer_shape_updates = $stats['pointer_shape_updates']
            full_initial_updates = $stats['full_initial_updates']
            full_empty_damage_updates = $stats['full_empty_damage_updates']
            full_large_damage_updates = $stats['full_large_damage_updates']
            full_fragmented_damage_updates = $stats['full_fragmented_damage_updates']
            delta_staging_allocations = $stats['delta_staging_allocations']
            readback_ms = $stats['readback']
            raw = $output
        }
    }
}
$path = Join-Path $resultDir ("ScreenDelta_{0:yyyy-MM-dd_HH-mm-ss}.csv" -f (Get-Date))
$rows | Export-Csv -NoTypeInformation -Encoding utf8 -Path $path
Write-Output "Saved $path"
