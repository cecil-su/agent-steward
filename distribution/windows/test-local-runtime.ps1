# Optional end-to-end test: really compiles the current repository twice and switches
# only isolated test daemons. Never uses the user's install, database or listening port.
$ErrorActionPreference = 'Stop'
$source = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$temp = Join-Path ([IO.Path]::GetTempPath()) "steward-local-runtime-$([guid]::NewGuid().ToString('N'))"
. (Join-Path $PSScriptRoot 'update-local.ps1') -InstallRoot $temp -LibraryOnly
New-Item -ItemType Directory -Path $temp,(Join-Path $temp 'versions'),(Join-Path $temp 'runs') | Out-Null
try {
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0)
    $listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop()
    Write-Json (Join-Path $temp 'settings.json') @{bind='127.0.0.1';port=$port;database=(Join-Path $temp 'tasks.db');runtimeDir=(Join-Path $temp 'runtime');requireLocalAuth=$false}
    $url = Update-LocalManaged $source
    $first = (Read-Json (Join-Path $temp 'current.json')).version
    $null = Invoke-RestMethod "$url/api/commands/task-create" -Method Post -ContentType 'application/json' -Headers @{Origin=$url;'X-Steward-CSRF'='1'} -Body '{"input":{}}'
    $url = Update-LocalManaged $source
    $second = (Read-Json (Join-Path $temp 'current.json')).version
    if ($first -eq $second -or (Read-Json (Join-Path $temp 'previous.json')).version -ne $first) { throw 'Build activation references are incorrect.' }
    $tasks = Invoke-RestMethod "$url/api/tasks"
    if ($tasks.data.tasks.Count -ne 1) { throw 'Task did not survive the local update.' }
    Write-Host 'PASS: real local release compilation, initial installation, incremental rebuild, graceful version switch and database preservation.'
} finally {
    Stop-Managed
    Remove-Item -LiteralPath $temp -Recurse -Force
}
