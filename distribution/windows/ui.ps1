[CmdletBinding()]
param(
    [ValidateSet('Build','Activate','Rollback','Status')][string]$Action = 'Status',
    [string]$SourceRoot = (Join-Path $PSScriptRoot '../..'),
    [string]$Output,
    [string]$Version = ('local-' + [DateTime]::UtcNow.ToString('yyyyMMddHHmmss')),
    [string]$Package,
    [string]$Release,
    [string]$InstallRoot = (Join-Path $env:LOCALAPPDATA 'agent-steward-app'),
    [string]$UiRoot,
    [string]$Url,
    [switch]$LibraryOnly
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
function Write-UiJson([string]$Path, $Value) {
    $temp = "$Path.$([guid]::NewGuid().ToString('N')).tmp"
    [IO.File]::WriteAllText($temp, ($Value | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    if (Test-Path -LiteralPath $Path) { [IO.File]::Replace($temp, $Path, [NullString]::Value) }
    else { [IO.File]::Move($temp, $Path) }
}
function Assert-UiFile([string]$Path, [long]$Limit) {
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -gt $Limit) { throw 'Invalid UI file.' }
}
function Assert-UiDirectory([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Invalid UI directory.' }
}
function Get-UiHash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Read-UiPackage([string]$Directory) {
    Assert-UiDirectory $Directory
    $path = Join-Path $Directory 'manifest.json'; Assert-UiFile $path 16384
    $manifest = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
    $keys = @($manifest.PSObject.Properties.Name | Sort-Object)
    if (($keys -join ',') -cne 'entry,files,packageFormat,requiredApiContract,uiVersion') { throw 'Invalid UI manifest fields.' }
    if ($manifest.packageFormat -ne 1 -or $manifest.requiredApiContract -ne 1 -or $manifest.entry -cne 'index.html' -or $manifest.uiVersion -cnotmatch '^[A-Za-z0-9._-]{1,100}$') { throw 'Unsupported UI package or API contract.' }
    if ((@($manifest.files.PSObject.Properties.Name | Sort-Object) -join ',') -cne 'app.js,index.html,style.css') { throw 'Invalid UI resource list.' }
    foreach ($file in @('index.html','app.js','style.css')) {
        $path = Join-Path $Directory $file; Assert-UiFile $path 4194304
        if ((Get-UiHash $path) -cne $manifest.files.$file) { throw 'UI file checksum mismatch.' }
        $null = [Text.UTF8Encoding]::new($false,$true).GetString([IO.File]::ReadAllBytes($path))
    }
    $html = [IO.File]::ReadAllText((Join-Path $Directory 'index.html'))
    if (-not $html.Contains('<head>') -or -not $html.Contains('src="/app.js"') -or -not $html.Contains('href="/style.css"')) { throw 'UI entry does not follow package format 1.' }
    return [pscustomobject]@{ Id=(Get-UiHash (Join-Path $Directory 'manifest.json')); Manifest=$manifest }
}
function Build-UiPackage([string]$Repository, [string]$Destination, [string]$UiVersion) {
    if (-not $Destination -or $UiVersion -cnotmatch '^[A-Za-z0-9._-]{1,100}$') { throw 'Build requires a new -Output directory and a valid -Version.' }
    if (Test-Path -LiteralPath $Destination) { throw 'Output already exists; packages are immutable.' }
    $null = New-Item -ItemType Directory -Path $Destination
    $hashes = @{}
    foreach ($file in @('index.html','app.js','style.css')) {
        $source = Join-Path $Repository "crates/server/web/$file"; Assert-UiFile $source 4194304
        $target = Join-Path $Destination $file
        Copy-Item -LiteralPath $source -Destination $target
        $hashes[$file] = Get-UiHash $target
    }
    # Detect concurrent source changes rather than publishing a known mixed snapshot.
    foreach ($file in @('index.html','app.js','style.css')) {
        if ((Get-UiHash (Join-Path $Repository "crates/server/web/$file")) -cne $hashes[$file]) { throw 'UI sources changed during build; use a fresh output and retry.' }
    }
    Write-UiJson (Join-Path $Destination 'manifest.json') @{packageFormat=1;uiVersion=$UiVersion;requiredApiContract=1;entry='index.html';files=$hashes}
    return Read-UiPackage $Destination
}
function Get-UiStatus([string]$Address) {
    $uri = [Uri]$Address
    if ($uri.Scheme -ne 'http' -or $uri.UserInfo -or $uri.Query -or $uri.Fragment -or $uri.AbsolutePath -ne '/') { throw 'Use the taskd HTTP origin, without credentials, path, query or fragment.' }
    $status = Invoke-RestMethod -Uri ($Address.TrimEnd('/') + '/ui/status') -TimeoutSec 5
    if ($status.packageFormat -ne 1 -or $status.apiContract -ne 1 -or -not $status.externalEnabled) { throw 'taskd must first be upgraded and started with --ui-root. No service will be restarted by this script.' }
    return $status
}
function Set-UiRelease([string]$Root, [string]$Address, [string]$Id) {
    if ($Id -cne 'embedded' -and $Id -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid release ID.' }
    if ($Id -cne 'embedded') {
        $packageInfo = Read-UiPackage (Join-Path $Root "releases/$Id")
        if ($packageInfo.Id -cne $Id) { throw 'Release directory does not match manifest hash.' }
    }
    $before = Get-UiStatus $Address
    $old = if ($before.release -cmatch '^embedded-[a-f0-9]{64}$') { 'embedded' } else { $before.release }
    $pointer = Join-Path $Root 'current.json'
    Write-UiJson $pointer @{release=$Id}
    try {
        for ($i=0; $i -lt 10; $i++) {
            $status = Get-UiStatus $Address
            if (-not $status.error -and (($Id -ceq 'embedded' -and $status.release -cmatch '^embedded-[a-f0-9]{64}$') -or $status.release -ceq $Id)) {
                if ($old -cne $Id) { Write-UiJson (Join-Path $Root 'previous.json') @{release=$old} }
                return $status
            }
            Start-Sleep -Milliseconds 200
        }
        throw 'taskd did not adopt the requested UI. Verify --ui-root and inspect /ui/status.'
    } catch {
        # Restore only our pointer, under the UI installation lock; never stop taskd.
        Write-UiJson $pointer @{release=$old}
        try { $null = Get-UiStatus $Address } catch { Write-Warning 'Rollback adoption is unconfirmed; inspect /ui/status before retrying.' }
        throw
    }
}
if ($LibraryOnly) { return }
if ($Action -eq 'Build') { Build-UiPackage ([IO.Path]::GetFullPath($SourceRoot)) $Output $Version; return }
if (-not $UiRoot) { $UiRoot = Join-Path $InstallRoot 'ui' }
$UiRoot = [IO.Path]::GetFullPath($UiRoot)
if (-not $Url) {
    $settings = Get-Content -LiteralPath (Join-Path $InstallRoot 'settings.json') -Raw | ConvertFrom-Json
    $Url = "http://$($settings.bind):$($settings.port)"
}
if ($Action -eq 'Status') { Get-UiStatus $Url; return }
$null = New-Item -ItemType Directory -Force -Path $UiRoot,(Join-Path $UiRoot 'releases')
Assert-UiDirectory $UiRoot; Assert-UiDirectory (Join-Path $UiRoot 'releases')
$lock = [IO.File]::Open((Join-Path $UiRoot 'update.lock'),'OpenOrCreate','ReadWrite','None')
try {
    $null = Get-UiStatus $Url
    if ($Action -eq 'Activate') {
        if (-not $Package) { throw 'Activate requires -Package (an extracted UI package directory).' }
        $info = Read-UiPackage $Package
        $destination = Join-Path $UiRoot "releases/$($info.Id)"
        if (-not (Test-Path -LiteralPath $destination)) {
            $stage = Join-Path $UiRoot ('.stage-' + [guid]::NewGuid().ToString('N'))
            $null = New-Item -ItemType Directory -Path $stage
            foreach ($file in @('manifest.json','index.html','app.js','style.css')) { Copy-Item -LiteralPath (Join-Path $Package $file) -Destination $stage }
            if ((Read-UiPackage $stage).Id -cne $info.Id) { throw 'Package changed while staging.' }
            [IO.Directory]::Move($stage,$destination)
        }
        Set-UiRelease $UiRoot $Url $info.Id
    } else {
        if (-not $Release) { $Release = (Get-Content -LiteralPath (Join-Path $UiRoot 'previous.json') -Raw | ConvertFrom-Json).release }
        Set-UiRelease $UiRoot $Url $Release
    }
} finally { $lock.Dispose() }
