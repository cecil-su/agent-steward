[CmdletBinding()]
param(
    [ValidateSet('Start','Update','Stop')][string]$Action = 'Start',
    [string]$InstallRoot = (Join-Path $env:LOCALAPPDATA 'agent-steward-app'),
    [switch]$LibraryOnly
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Read-Json([string]$Path) { Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json }
function Write-Json([string]$Path, $Value) {
    $temp = "$Path.$([guid]::NewGuid().ToString('N')).tmp"
    [IO.File]::WriteAllText($temp, ($Value | ConvertTo-Json -Depth 10), [Text.UTF8Encoding]::new($false))
    if (Test-Path -LiteralPath $Path) { [IO.File]::Replace($temp, $Path, [NullString]::Value) }
    else { [IO.File]::Move($temp, $Path) }
}
function Assert-Version([string]$Version) {
    if ($Version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+$') { throw 'Only stable vMAJOR.MINOR.PATCH releases are supported.' }
}
function Assert-BuildId([string]$Version) {
    if ($Version -notmatch '^(v[0-9]+\.[0-9]+\.[0-9]+|local-[0-9]{14}-[0-9a-f]{32})$') { throw 'Invalid installed build ID.' }
}
function Assert-Manifest($Manifest) {
    Assert-BuildId $Manifest.version
    if ($Manifest.databaseSchema -ne 6 -or $Manifest.launcherProtocol -ne 1 -or $Manifest.target -ne 'x86_64-pc-windows-msvc') {
        throw 'This launcher requires a Schema 6 package. Cross-schema upgrades require a separately backed-up migration and a separate installation directory.'
    }
}
function Assert-Settings($Settings) {
    $ip = [Net.IPAddress]::Parse($Settings.bind)
    if ($ip.AddressFamily -ne [Net.Sockets.AddressFamily]::InterNetwork -or $ip.ToString() -ne $Settings.bind -or $Settings.bind -eq '0.0.0.0' -or $ip.GetAddressBytes()[0] -ge 224) { throw 'bind must be a specific local IPv4 address.' }
    if ($Settings.port -lt 1 -or $Settings.port -gt 65535) { throw 'port must be 1..65535.' }
    foreach ($path in @($Settings.database, $Settings.runtimeDir)) {
        if (-not [IO.Path]::IsPathRooted($path) -or $path -match '["\r\n]') { throw 'Data paths must be absolute paths without quotes or line breaks.' }
    }
    if ($Settings.requireLocalAuth -isnot [bool]) { throw 'requireLocalAuth must be a JSON boolean.' }
}
function Quote-Argument([string]$Value) {
    if ($Value -match '["\r\n]') { throw 'Invalid command argument.' }
    '"' + ($Value -replace '(\\+)$', '$1$1') + '"'
}
function Expand-Release([string]$Zip, [string]$Checksum, [string]$Destination) {
    $expected = (Get-Content -LiteralPath $Checksum -Raw).Trim().Split(' ')[0]
    if ($expected -notmatch '^[0-9a-fA-F]{64}$' -or (Get-FileHash -LiteralPath $Zip -Algorithm SHA256).Hash -ne $expected) { throw 'Release checksum mismatch; running service was not changed.' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($Zip)
    try {
        $allowed = @('taskd.exe','taskctl.exe','task-hook.exe','manifest.json','steward.ps1','Start.cmd','Update.cmd','Stop.cmd','README.md')
        $seen = @{}
        foreach ($entry in $archive.Entries) {
            if ($entry.FullName -cnotin $allowed -or $seen.ContainsKey($entry.FullName) -or $entry.Length -gt 200MB) { throw 'Invalid release archive entry.' }
            $seen[$entry.FullName] = $true
        }
        if ($seen.Count -ne $allowed.Count) { throw 'Incomplete release archive.' }
        [IO.Compression.ZipFile]::ExtractToDirectory($Zip, $Destination)
    } finally { $archive.Dispose() }
    Assert-Manifest (Read-Json (Join-Path $Destination 'manifest.json'))
}
function Get-ManagedProcess {
    $path = Join-Path $InstallRoot 'process.json'
    if (-not (Test-Path -LiteralPath $path)) { return $null }
    $record = Read-Json $path
    $process = Get-Process -Id $record.pid -ErrorAction SilentlyContinue
    if (-not $process) { Remove-Item -LiteralPath $path; return $null }
    if ($process.Path -ne $record.executable -or $process.StartTime.ToUniversalTime().Ticks.ToString() -ne $record.started) {
        throw 'Managed PID no longer identifies our process. No process was stopped. Inspect process.json before proceeding.'
    }
    $expectedDirectory = [IO.Path]::GetFullPath((Join-Path $InstallRoot 'runs'))
    if ([IO.Path]::GetDirectoryName($record.marker) -ne $expectedDirectory) { throw 'Invalid shutdown marker.' }
    [pscustomobject]@{ Process = $process; Record = $record }
}
function Stop-Managed {
    $managed = Get-ManagedProcess
    if (-not $managed) { return }
    [IO.File]::WriteAllText($managed.Record.marker, 'stop')
    if (-not $managed.Process.WaitForExit(60000)) {
        throw 'Graceful stop timed out. No force/kill was used; finish in-flight work and retry.'
    }
    Remove-Item -LiteralPath (Join-Path $InstallRoot 'process.json')
}
function Assert-StartCompatible([string]$Version) {
    Assert-BuildId $Version
    $directory = Join-Path $InstallRoot "versions\$Version"
    $manifest = Read-Json (Join-Path $directory 'manifest.json'); Assert-Manifest $manifest
    $settings = Read-Json (Join-Path $InstallRoot 'settings.json'); Assert-Settings $settings
    # Run only the selected trusted binary, with an explicit path. This mode must
    # exit before initialization/listening; old binaries reject the unknown flag.
    $result = @(& (Join-Path $directory 'taskd.exe') --check-database-schema --database $settings.database)
    if ($LASTEXITCODE -ne 0 -or $result.Count -ne 1 -or $result[0] -cne "databaseSchema=$($manifest.databaseSchema)") {
        throw 'Database/binary schema preflight failed. Arrange an explicit migration; no database restore or migration was attempted.'
    }
}
function Assert-SwitchCompatible([string]$Version) {
    # Do not even attempt an automatic cross-schema rollback. Preserve the old
    # installation; activate an explicitly migrated database in a separate root.
    $currentPath = Join-Path $InstallRoot 'current.json'
    if (Test-Path -LiteralPath $currentPath) {
        $current = Read-Json $currentPath; Assert-BuildId $current.version
        Assert-Manifest (Read-Json (Join-Path $InstallRoot "versions\$($current.version)\manifest.json"))
    }
    Assert-StartCompatible $Version
}
function Start-Managed([string]$Version) {
    Assert-StartCompatible $Version
    if (Get-ManagedProcess) { throw 'A managed process is already running.' }
    $settings = Read-Json (Join-Path $InstallRoot 'settings.json'); Assert-Settings $settings
    $directory = Join-Path $InstallRoot "versions\$Version"
    $manifest = Read-Json (Join-Path $directory 'manifest.json'); Assert-Manifest $manifest
    $executable = Join-Path $directory 'taskd.exe'
    $id = [guid]::NewGuid().ToString('N')
    $marker = Join-Path $InstallRoot "runs\$id.stop"
    $arguments = @('--no-open','--bind',$settings.bind,'--port',"$($settings.port)",'--database',(Quote-Argument $settings.database),'--runtime-dir',(Quote-Argument $settings.runtimeDir),'--shutdown-file',(Quote-Argument $marker))
    if ($manifest.PSObject.Properties['uiPackageProtocol'] -and $manifest.uiPackageProtocol -eq 1) {
        $arguments += @('--ui-root',(Quote-Argument (Join-Path $InstallRoot 'ui')))
    }
    if ($settings.requireLocalAuth) { $arguments += '--require-local-auth' }
    $process = Start-Process -FilePath $executable -ArgumentList $arguments -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $InstallRoot "runs\$id.out.log") -RedirectStandardError (Join-Path $InstallRoot "runs\$id.err.log")
    Write-Json (Join-Path $InstallRoot 'process.json') @{pid=$process.Id; started=$process.StartTime.ToUniversalTime().Ticks.ToString(); executable=$executable; marker=$marker; version=$Version}
    $url = "http://$($settings.bind):$($settings.port)"
    for ($i=0; $i -lt 30; $i++) {
        Start-Sleep -Milliseconds 500
        if ($process.HasExited) { throw "taskd exited. See $InstallRoot\runs\$id.err.log (port may be occupied by an unmanaged service)." }
        # Check socket ownership, not merely a response from an unrelated old daemon.
        $listener = Get-NetTCPConnection -State Listen -LocalPort $settings.port -ErrorAction SilentlyContinue | Where-Object { $_.OwningProcess -eq $process.Id -and $_.LocalAddress -eq $settings.bind }
        if ($listener) {
            try { $response = Invoke-WebRequest -UseBasicParsing -Uri $url -TimeoutSec 2; if ($response.StatusCode -eq 200) { return $url } } catch { }
        }
    }
    throw 'New taskd did not become healthy. No forced termination will be used.'
}
function Install-Bundle([string]$Bundle) {
    $manifest = Read-Json (Join-Path $Bundle 'manifest.json'); Assert-Manifest $manifest
    $destination = Join-Path $InstallRoot "versions\$($manifest.version)"
    if (Test-Path -LiteralPath $destination) {
        foreach ($file in @('taskd.exe','taskctl.exe','task-hook.exe','manifest.json','steward.ps1','Start.cmd','Update.cmd','Stop.cmd','README.md')) {
            if ((Get-FileHash -LiteralPath (Join-Path $Bundle $file)).Hash -ne (Get-FileHash -LiteralPath (Join-Path $destination $file)).Hash) { throw 'Existing version differs; refusing to overwrite it.' }
        }
        return $manifest.version
    }
    # Copy only distribution files, never settings, credentials or a task database.
    New-Item -ItemType Directory -Path $destination | Out-Null
    foreach ($file in @('taskd.exe','taskctl.exe','task-hook.exe','manifest.json','steward.ps1','Start.cmd','Update.cmd','Stop.cmd','README.md')) {
        Copy-Item -LiteralPath (Join-Path $Bundle $file) -Destination $destination
    }
    return $manifest.version
}
function Update-Managed {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/cecil-su/agent-steward/releases/latest' -Headers @{'User-Agent'='agent-steward-updater'}
    Assert-Version $release.tag_name
    $current = Read-Json (Join-Path $InstallRoot 'current.json')
    Assert-Version $current.version # Local builds use Update-Local.cmd, not automatic release comparison.
    if ($release.tag_name -eq $current.version) { Write-Host 'Already current.'; return }
    if ([version]$release.tag_name.Substring(1) -le [version]$current.version.Substring(1)) { throw 'Refusing an automatic downgrade.' }
    $assetName = 'agent-steward-windows-x64.zip'
    foreach ($name in @($assetName, "$assetName.sha256")) {
        $assets = @($release.assets | Where-Object { $_.name -ceq $name })
        $expected = "https://github.com/cecil-su/agent-steward/releases/download/$($release.tag_name)/$name"
        if ($assets.Count -ne 1 -or $assets[0].browser_download_url -cne $expected) { throw 'Missing or unexpected release asset URL.' }
    }
    $stage = Join-Path $InstallRoot "downloads\$([guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Path $stage | Out-Null
    foreach ($name in @($assetName, "$assetName.sha256")) {
        Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/cecil-su/agent-steward/releases/download/$($release.tag_name)/$name" -OutFile (Join-Path $stage $name)
    }
    $bundle = Join-Path $stage 'bundle'
    Expand-Release (Join-Path $stage $assetName) (Join-Path $stage "$assetName.sha256") $bundle
    if ((Read-Json (Join-Path $bundle 'manifest.json')).version -cne $release.tag_name) { throw 'Release manifest/tag mismatch.' }
    $version = Install-Bundle $bundle
    # Everything above completes before touching the running process.
    $url = Switch-Managed $version
    Write-Host "Updated to $version. $url"
    Start-Process $url
}

function Switch-Managed([string]$Version) {
    Assert-SwitchCompatible $Version
    $currentPath = Join-Path $InstallRoot 'current.json'
    $current = if (Test-Path -LiteralPath $currentPath) { Read-Json $currentPath } else { $null }
    Stop-Managed
    try {
        $url = Start-Managed $version
        if ($current) { Write-Json (Join-Path $InstallRoot 'previous.json') $current }
        Write-Json (Join-Path $InstallRoot 'current.json') @{version=$version}
    } catch {
        $failure = $_
        Stop-Managed # A timeout here deliberately prevents starting a second daemon.
        if ($current) {
            # The database may have changed while startup failed. Refuse an
            # incompatible fallback instead of launching an old schema binary.
            Assert-StartCompatible $current.version
            Write-Json $currentPath $current
            $null = Start-Managed $current.version
            throw "Update failed; previous binary restarted. No database restore was attempted. $failure"
        }
        throw "First startup failed; no previous build exists. $failure"
    }
    return $url
}

if ($LibraryOnly) { return }
# Copied Start/Stop/Update.cmd must stay in their custom installation, not fall
# back to the user's default installation. Explicit -InstallRoot always wins.
if (-not $PSBoundParameters.ContainsKey('InstallRoot') -and (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'current.json'))) {
    $InstallRoot = $PSScriptRoot
}
$InstallRoot = [IO.Path]::GetFullPath($InstallRoot)
# An old shortcut follows the installed current version, so launcher fixes take effect too.
$currentPath = Join-Path $InstallRoot 'current.json'
if (Test-Path -LiteralPath $currentPath) {
    $current = Read-Json $currentPath; Assert-BuildId $current.version
    Assert-Manifest (Read-Json (Join-Path $InstallRoot "versions\$($current.version)\manifest.json"))
    $launcher = Join-Path $InstallRoot "versions\$($current.version)\steward.ps1"
    if ([IO.Path]::GetFullPath($PSCommandPath) -ne $launcher) { & $launcher -Action $Action -InstallRoot $InstallRoot; return }
}
New-Item -ItemType Directory -Force -Path $InstallRoot,(Join-Path $InstallRoot 'versions'),(Join-Path $InstallRoot 'runs') | Out-Null
$lock = [IO.File]::Open((Join-Path $InstallRoot 'launcher.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
try {
    if (-not (Test-Path -LiteralPath $currentPath)) {
        if ($Action -ne 'Start') { throw 'First run Start.cmd from an extracted release package.' }
        $version = Install-Bundle $PSScriptRoot
        Write-Json $currentPath @{version=$version}
        foreach ($file in @('Start.cmd','Update.cmd','Stop.cmd','steward.ps1')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination $InstallRoot }
    }
    $settingsPath = Join-Path $InstallRoot 'settings.json'
    if (-not (Test-Path -LiteralPath $settingsPath)) {
        $bind = Read-Host 'Local bind IP [127.0.0.1] (for LAN access enter this PC IP, e.g. 172.19.10.185)'
        if (-not $bind) { $bind = '127.0.0.1' }
        $data = Join-Path $env:LOCALAPPDATA 'agent-steward'
        $settings = @{bind=$bind;port=43123;database=(Join-Path $data 'steward.db');runtimeDir=(Join-Path $data 'runtime');requireLocalAuth=$false}
        Assert-Settings $settings; Write-Json $settingsPath $settings
    }
    switch ($Action) {
        'Stop' { Stop-Managed; Write-Host 'Stopped managed taskd.' }
        'Update' { Update-Managed }
        'Start' {
            $managed = Get-ManagedProcess
            if ($managed) {
                $settings = Read-Json $settingsPath; Assert-Settings $settings
                $url = "http://$($settings.bind):$($settings.port)"
            } else { $url = Start-Managed (Read-Json $currentPath).version }
            Write-Host "Running: $url -- future entry: $InstallRoot\Start.cmd"
            Start-Process $url
        }
    }
} finally { $lock.Dispose() }
