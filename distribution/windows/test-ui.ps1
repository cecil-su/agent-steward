# Isolated real-process UI update test. No user database, installation or service is used.
param([string]$Taskd = (Join-Path $PSScriptRoot '../../target/debug/taskd.exe'))
$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot 'ui.ps1'
$source = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$temp = Join-Path ([IO.Path]::GetTempPath()) ('steward-ui-test-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $temp
$root = Join-Path $temp 'ui'; $marker = Join-Path $temp 'stop'; $out = Join-Path $temp 'stdout.log'
$process = $null
function Assert($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
try {
    $a = & $script -Action Build -SourceRoot $source -Output (Join-Path $temp 'a') -Version a
    $b = & $script -Action Build -SourceRoot $source -Output (Join-Path $temp 'b') -Version b
    $arguments = @('--no-open','--port','0','--database',('"'+(Join-Path $temp 'tasks.db')+'"'),'--runtime-dir',('"'+(Join-Path $temp 'runtime')+'"'),'--ui-root',('"'+$root+'"'),'--shutdown-file',('"'+$marker+'"'))
    $process = Start-Process -FilePath ([IO.Path]::GetFullPath($Taskd)) -ArgumentList $arguments -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError (Join-Path $temp 'stderr.log')
    $url = $null
    for ($i=0; $i -lt 50; $i++) {
        Start-Sleep -Milliseconds 100
        if ($process.HasExited) { throw 'Isolated taskd exited before ready.' }
        $text = Get-Content -LiteralPath $out -Raw -ErrorAction SilentlyContinue
        if ($text -match 'Agent Steward: (http://127\.0\.0\.1:\d+)') { $url = $Matches[1]; break }
    }
    Assert ($null -ne $url) 'Isolated listener did not become ready.'
    $started = $process.StartTime
    $status = & $script -Action Activate -Package (Join-Path $temp 'a') -UiRoot $root -Url $url
    Assert ($status.release -ceq $a.Id) 'A was not adopted.'
    # Contract 3 packages must be rejected before changing the active pointer.
    $old = Join-Path $temp 'old-contract'; Copy-Item -LiteralPath (Join-Path $temp 'a') -Destination $old -Recurse
    $oldManifest = Get-Content -LiteralPath (Join-Path $old 'manifest.json') -Raw | ConvertFrom-Json
    $oldManifest.requiredApiContract = 3
    [IO.File]::WriteAllText((Join-Path $old 'manifest.json'), ($oldManifest | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    $pointerBefore = [IO.File]::ReadAllText((Join-Path $root 'current.json'))
    $rejected = $false
    try { $null = & $script -Action Activate -Package $old -UiRoot $root -Url $url } catch { $rejected = $true }
    Assert $rejected 'Old API contract package accepted.'
    Assert ([IO.File]::ReadAllText((Join-Path $root 'current.json')) -ceq $pointerBefore) 'Rejected old contract changed the active pointer.'
    $first = Invoke-WebRequest -UseBasicParsing "$url/"
    Assert ($first.Content.Contains("/ui/releases/$($a.Id)/app.js")) 'A entry is not version-pinned.'
    $status = & $script -Action Activate -Package (Join-Path $temp 'b') -UiRoot $root -Url $url
    Assert ($status.release -ceq $b.Id) 'B was not adopted.'
    foreach ($id in @($a.Id,$b.Id)) {
        $resource = Invoke-WebRequest -UseBasicParsing "$url/ui/releases/$id/app.js"
        Assert ($resource.StatusCode -eq 200 -and "$($resource.Headers['Cache-Control'])" -match 'immutable') 'Old or current immutable resource unavailable.'
    }
    $status = & $script -Action Rollback -UiRoot $root -Url $url
    Assert ($status.release -ceq $a.Id) 'Rollback did not restore A.'
    [IO.File]::WriteAllText((Join-Path $temp 'b/app.js'),'corrupt')
    $rejected = $false
    try { $null = & $script -Action Activate -Package (Join-Path $temp 'b') -UiRoot $root -Url $url } catch { $rejected = $true }
    Assert $rejected 'Corrupt package accepted.'
    [IO.File]::WriteAllText((Join-Path $root 'current.json'),'{')
    $status = & $script -Action Status -UiRoot $root -Url $url
    Assert ($status.release -ceq $a.Id -and $status.error -ceq 'UI_POINTER_INVALID') 'Malformed pointer lost last good release.'
    $status = & $script -Action Rollback -Release embedded -UiRoot $root -Url $url
    Assert ($status.uiVersion -ceq 'embedded' -and -not $status.error) 'Embedded rollback failed.'
    Assert (-not $process.HasExited -and (Get-Process -Id $process.Id).StartTime -eq $started) 'Daemon restarted during UI activation.'
    Write-Host 'PASS: isolated taskd PID/start time unchanged across A/B activation, retained old assets, rollback, corruption rejection and embedded fallback.'
} finally {
    if ($process -and -not $process.HasExited) {
        [IO.File]::WriteAllText($marker,'stop')
        if (-not $process.WaitForExit(10000)) { throw "Isolated taskd did not stop gracefully; test files retained at $temp. No force kill used." }
    }
    Remove-Item -LiteralPath $temp -Recurse -Force
}
