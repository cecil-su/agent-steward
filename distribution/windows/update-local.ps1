[CmdletBinding()]
param(
    [string]$SourceRoot = (Join-Path $PSScriptRoot '../..'),
    [string]$InstallRoot = (Join-Path $env:LOCALAPPDATA 'agent-steward-app'),
    [switch]$NoOpen,
    [switch]$LibraryOnly
)
$loadOnly = $LibraryOnly
. (Join-Path $PSScriptRoot 'steward.ps1') -InstallRoot $InstallRoot -LibraryOnly

function Invoke-LocalCargo([string]$Repository, [string]$Cache) {
    Push-Location -LiteralPath $Repository
    try {
        & cargo build --workspace --release --locked --target x86_64-pc-windows-msvc --target-dir $Cache | Out-Host
        if ($LASTEXITCODE -ne 0) { throw 'Local compilation failed. The running service was not stopped.' }
    } finally { Pop-Location }
}
function Build-LocalBundle([string]$Repository) {
    $Repository = [IO.Path]::GetFullPath($Repository)
    if (-not (Test-Path -LiteralPath (Join-Path $Repository 'Cargo.lock'))) { throw 'Select the agent-steward source repository, not an extracted binary package.' }
    $cache = Join-Path $InstallRoot 'build-cache'
    Invoke-LocalCargo $Repository $cache
    # A fresh installation ID on every build also supports uncommitted local changes.
    # It is not presented as a Git commit or an official release version.
    $version = 'local-' + [DateTime]::UtcNow.ToString('yyyyMMddHHmmss') + '-' + [guid]::NewGuid().ToString('N')
    $bundle = Join-Path $InstallRoot "local-builds\$version"
    New-Item -ItemType Directory -Path $bundle | Out-Null
    foreach ($file in @('taskd.exe','taskctl.exe','task-hook.exe')) {
        Copy-Item -LiteralPath (Join-Path $cache "x86_64-pc-windows-msvc\release\$file") -Destination $bundle
    }
    foreach ($file in @('steward.ps1','Start.cmd','Update.cmd','Stop.cmd','README.md')) {
        Copy-Item -LiteralPath (Join-Path $Repository "distribution\windows\$file") -Destination $bundle
    }
    Write-Json (Join-Path $bundle 'manifest.json') @{version=$version;databaseSchema=6;launcherProtocol=1;uiPackageProtocol=1;target='x86_64-pc-windows-msvc';sourceRoot=$Repository}
    return Install-Bundle $bundle
}
function Initialize-LocalSettings {
    $path = Join-Path $InstallRoot 'settings.json'
    if (Test-Path -LiteralPath $path) { Assert-Settings (Read-Json $path); return }
    Write-Host 'First setup: reuse the database/runtime paths of your existing service. No running service will be adopted.'
    $data = Join-Path $env:LOCALAPPDATA 'agent-steward'
    $bind = Read-Host 'Local bind IP [127.0.0.1] (e.g. 172.19.10.185)'; if (-not $bind) { $bind = '127.0.0.1' }
    $port = Read-Host 'Port [43123]'; if (-not $port) { $port = '43123' }
    $database = Read-Host "Task database [$data\steward.db]"; if (-not $database) { $database = Join-Path $data 'steward.db' }
    $runtime = Read-Host "Runtime directory [$data\runtime]"; if (-not $runtime) { $runtime = Join-Path $data 'runtime' }
    $settings = @{bind=$bind;port=[int]$port;database=$database;runtimeDir=$runtime;requireLocalAuth=$false}
    Assert-Settings $settings; Write-Json $path $settings
}
function Update-LocalManaged([string]$Repository) {
    # Compile and stage all files BEFORE requesting graceful shutdown.
    $version = Build-LocalBundle $Repository
    Assert-SwitchCompatible $version
    # Keep the stable launcher able to resolve local build IDs. No data/config files
    # are copied. Installed versions keep their own launcher for recovery.
    foreach ($file in @('Start.cmd','Update.cmd','Stop.cmd','steward.ps1')) {
        Copy-Item -LiteralPath (Join-Path $InstallRoot "versions\$version\$file") -Destination $InstallRoot
    }
    return Switch-Managed $version
}

if ($loadOnly) { return }
$InstallRoot = [IO.Path]::GetFullPath($InstallRoot)
$SourceRoot = [IO.Path]::GetFullPath($SourceRoot)
New-Item -ItemType Directory -Force -Path $InstallRoot,(Join-Path $InstallRoot 'versions'),(Join-Path $InstallRoot 'runs') | Out-Null
$lock = [IO.File]::Open((Join-Path $InstallRoot 'launcher.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
try {
    Initialize-LocalSettings
    Write-Host "Building current working-tree sources: $SourceRoot (no Git pull, reset or stash)."
    $url = Update-LocalManaged $SourceRoot
    Write-Host "Local update ready: $url"
    Write-Host "Start/stop next time: $InstallRoot\Start.cmd / Stop.cmd"
    if (-not $NoOpen) { Start-Process $url }
} finally { $lock.Dispose() }
