param([string]$Taskd, [string]$IncompatibleTaskd)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'steward.ps1') -LibraryOnly
function Assert($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Assert-Fails([scriptblock]$Code) {
    $failed = $false
    try { & $Code } catch { $failed = $true; $script:lastFailure = $_ }
    Assert $failed 'Expected rejection.'
}
$temp = Join-Path ([IO.Path]::GetTempPath()) "steward-launcher-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $temp | Out-Null
$unicode = ([string][char]0x4E2D) + [char]0x6587
$InstallRoot = Join-Path $temp "install with spaces $unicode"
New-Item -ItemType Directory -Path $InstallRoot,(Join-Path $InstallRoot 'versions'),(Join-Path $InstallRoot 'runs') | Out-Null
try {
    Assert-Fails { Assert-Version '../v1.0.0' }
    foreach ($schema in @(0,1,2,3,4,6)) {
        Assert-Fails { Assert-Manifest @{version='v1.0.0';databaseSchema=$schema;launcherProtocol=1;target='x86_64-pc-windows-msvc'} }
    }
    Assert-Manifest @{version='v1.0.0';databaseSchema=5;launcherProtocol=1;target='x86_64-pc-windows-msvc'}
    Assert-Fails { Assert-Settings @{bind='0.0.0.0';port=43123;database='C:\tasks.db';runtimeDir='C:\runtime';requireLocalAuth=$false} }
    $bundle = Join-Path $temp 'bundle'; New-Item -ItemType Directory -Path $bundle | Out-Null
    foreach ($file in @('taskd.exe','taskctl.exe','task-hook.exe','steward.ps1','Start.cmd','Update.cmd','Stop.cmd','README.md')) { [IO.File]::WriteAllText((Join-Path $bundle $file), 'synthetic') }
    Write-Json (Join-Path $bundle 'manifest.json') @{version='v1.0.1';databaseSchema=5;launcherProtocol=1;target='x86_64-pc-windows-msvc'}
    $zip = Join-Path $temp 'release.zip'; Compress-Archive -Path (Join-Path $bundle '*') -DestinationPath $zip
    $sum = "$zip.sha256"; [IO.File]::WriteAllText($sum, (Get-FileHash $zip).Hash)
    Expand-Release $zip $sum (Join-Path $temp 'expanded')
    [IO.File]::WriteAllText($sum, ('0' * 64)); Assert-Fails { Expand-Release $zip $sum (Join-Path $temp 'bad') }
    Assert (-not (Test-Path (Join-Path $temp 'bad'))) 'Bad checksum extracted files.'
    [IO.File]::WriteAllText($sum, (Get-FileHash $zip).Hash)
    $evil = Join-Path $temp 'evil.zip'; $archive = [IO.Compression.ZipFile]::Open($evil, 'Create')
    $null = $archive.CreateEntry('../escape'); $archive.Dispose()
    $evilSum = "$evil.sha256"; [IO.File]::WriteAllText($evilSum, (Get-FileHash $evil).Hash)
    Assert-Fails { Expand-Release $evil $evilSum (Join-Path $temp 'evil') }
    $null = Install-Bundle $bundle; $null = Install-Bundle $bundle
    [IO.File]::WriteAllText((Join-Path $bundle 'taskd.exe'), 'changed')
    Assert-Fails { Install-Bundle $bundle }
    [IO.File]::WriteAllText((Join-Path $bundle 'taskd.exe'), 'synthetic')
    # Never stop a PID merely because a saved record mentions it.
    Write-Json (Join-Path $InstallRoot 'process.json') @{pid=$PID;started='0';executable='not-our-process';marker=(Join-Path $InstallRoot 'runs\x.stop')}
    Assert-Fails { Stop-Managed }
    Assert (-not (Test-Path (Join-Path $InstallRoot 'runs\x.stop'))) 'PID mismatch created shutdown marker.'
    Remove-Item (Join-Path $InstallRoot 'process.json')

    if ($Taskd) {
        $Taskd = [IO.Path]::GetFullPath($Taskd)
        Copy-Item $Taskd (Join-Path $InstallRoot 'versions\v1.0.1\taskd.exe') -Force
        $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0); $listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop()
        $settings = @{bind='127.0.0.1';port=$port;database=(Join-Path $temp "$unicode tasks.db");runtimeDir=(Join-Path $temp "$unicode runtime");requireLocalAuth=$false}
        Write-Json (Join-Path $InstallRoot 'settings.json') $settings
        try {
            $url = Start-Managed 'v1.0.1'
            $access = Invoke-RestMethod "$url/api/access"
            Assert ($access.data.local -and $access.data.role -eq 'admin') 'Local browser still requires credentials.'
            $result = Invoke-RestMethod "$url/api/commands/task-create" -Method Post -ContentType 'application/json' -Headers @{Origin=$url;'X-Steward-CSRF'='1'} -Body '{"input":{}}'
            Assert $result.ok 'Local write failed.'
            Stop-Managed
            $url = Start-Managed 'v1.0.1'
            $tasks = Invoke-RestMethod "$url/api/tasks"
            Assert ($tasks.data.tasks.Count -eq 1) 'Restart did not preserve the task database.'
            # A different, owned SQLite file advertises the old schema. Preflight
            # must reject it without stopping the still-running Schema 5 daemon.
            $oldDatabase = Join-Path $temp 'old-schema.db'
            $bytes = [IO.File]::ReadAllBytes($settings.database)
            $bytes[60]=0; $bytes[61]=0; $bytes[62]=0; $bytes[63]=2
            [IO.File]::WriteAllBytes($oldDatabase,$bytes)
            $oldHash = (Get-FileHash -LiteralPath $oldDatabase).Hash
            $recordBefore = [IO.File]::ReadAllText((Join-Path $InstallRoot 'process.json'))
            $oldSettings = $settings.Clone(); $oldSettings.database = $oldDatabase
            Write-Json (Join-Path $InstallRoot 'settings.json') $oldSettings
            Assert-Fails { Switch-Managed 'v1.0.1' }
            Assert ([IO.File]::ReadAllText((Join-Path $InstallRoot 'process.json')) -ceq $recordBefore) 'Rejected schema changed the live process record.'
            Assert ((Get-FileHash -LiteralPath $oldDatabase).Hash -eq $oldHash) 'Preflight changed the old database.'
            Write-Json (Join-Path $InstallRoot 'settings.json') $settings
            Assert ((Invoke-RestMethod "$url/api/tasks").data.tasks.Count -eq 1) 'Schema rejection stopped the running daemon.'
        } finally { Stop-Managed }
        # Exercise the real package writer with the built three-piece binary set.
        $packageOutput = Join-Path $temp 'packaged'
        & (Join-Path $PSScriptRoot 'package.ps1') -Version 'v9.0.0' -Binaries ([IO.Path]::GetDirectoryName($Taskd)) -Output $packageOutput
        $packaged = Read-Json (Join-Path $packageOutput 'bundle\manifest.json')
        Assert ($packaged.databaseSchema -eq 5) 'Release package mislabeled Schema 5.'
        Expand-Release (Join-Path $packageOutput 'agent-steward-windows-x64.zip') (Join-Path $packageOutput 'agent-steward-windows-x64.zip.sha256') (Join-Path $temp 'package-verified')
        # Restore the synthetic binary for immutable-package retry checks below.
        [IO.File]::WriteAllText((Join-Path $InstallRoot 'versions\v1.0.1\taskd.exe'), 'synthetic')
    }
    if ($IncompatibleTaskd) {
        $badPackage = Join-Path $temp 'incompatible-package'
        Assert-Fails { & (Join-Path $PSScriptRoot 'package.ps1') -Version 'v9.0.1' -Binaries ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($IncompatibleTaskd))) -Output $badPackage }
        Assert (-not (Test-Path -LiteralPath $badPackage)) 'Mismatched binary created a mislabeled package.'
    }
    # A copied launcher entry must resolve its own custom root, not the default app.
    $entryRoot = Join-Path $temp "custom entry $unicode"; $entryVersion = Join-Path $entryRoot 'versions\v1.0.0'
    New-Item -ItemType Directory -Path $entryVersion | Out-Null
    foreach ($p in @((Join-Path $entryRoot 'steward.ps1'),(Join-Path $entryVersion 'steward.ps1'))) { Copy-Item (Join-Path $PSScriptRoot 'steward.ps1') $p }
    Write-Json (Join-Path $entryRoot 'current.json') @{version='v1.0.0'}
    Write-Json (Join-Path $entryRoot 'settings.json') @{bind='127.0.0.1';port=43123;database=(Join-Path $entryRoot 'entry.db');runtimeDir=(Join-Path $entryRoot 'runtime');requireLocalAuth=$false}
    Write-Json (Join-Path $entryVersion 'manifest.json') @{version='v1.0.0';databaseSchema=5;launcherProtocol=1;target='x86_64-pc-windows-msvc'}
    $savedLocalAppData = $env:LOCALAPPDATA
    try {
        $env:LOCALAPPDATA = Join-Path $temp 'unused default'
        & (Get-Process -Id $PID).Path -NoProfile -File (Join-Path $entryRoot 'steward.ps1') -Action Stop
        Assert ($LASTEXITCODE -eq 0) 'Custom-root launcher entry failed.'
        Assert (-not (Test-Path -LiteralPath $env:LOCALAPPDATA)) 'Custom-root launcher touched the default installation.'
    } finally { $env:LOCALAPPDATA = $savedLocalAppData }
    # Offline update orchestration: download first; failure restarts the old version.
    New-Item -ItemType Directory -Path (Join-Path $InstallRoot 'versions\v1.0.0') | Out-Null
    Write-Json (Join-Path $InstallRoot 'versions\v1.0.0\manifest.json') @{version='v1.0.0';databaseSchema=5;launcherProtocol=1;target='x86_64-pc-windows-msvc'}
    $script:failPreflight = $false
    function Assert-StartCompatible([string]$Version) {
        if ($script:failPreflight) { throw 'synthetic schema mismatch' }
    }
    $script:stops = 0; $script:starts = @(); $script:failDownload = $false; $script:failStartup = $true; $script:invalidateFallback = $false
    function Stop-Managed { $script:stops++ }
    function Start-Managed([string]$Version) { $script:starts += $Version; if ($script:failStartup -and $Version -eq 'v1.0.1') { if ($script:invalidateFallback) { $script:failPreflight=$true }; throw 'synthetic startup failure' }; return 'http://127.0.0.1:43123' }
    function Invoke-RestMethod { return @{tag_name='v1.0.1';assets=@(@{name='agent-steward-windows-x64.zip';browser_download_url='https://github.com/cecil-su/agent-steward/releases/download/v1.0.1/agent-steward-windows-x64.zip'},@{name='agent-steward-windows-x64.zip.sha256';browser_download_url='https://github.com/cecil-su/agent-steward/releases/download/v1.0.1/agent-steward-windows-x64.zip.sha256'})} }
    function Invoke-WebRequest($Uri,$OutFile,[switch]$UseBasicParsing) {
        if ($script:failDownload) { throw 'synthetic download failure' }
        if ($Uri.EndsWith('.sha256')) { Copy-Item $sum $OutFile } else { Copy-Item $zip $OutFile }
    }
    Write-Json (Join-Path $InstallRoot 'current.json') @{version='v1.0.0'}
    $script:failDownload = $true; Assert-Fails { Update-Managed }; Assert ($script:stops -eq 0) 'Download failure stopped the daemon.'
    $script:failDownload = $false
    $script:failPreflight = $true; Assert-Fails { Update-Managed }
    Assert ($script:stops -eq 0 -and $script:starts.Count -eq 0) 'Schema preflight failure touched the daemon.'
    $script:failPreflight = $false
    Write-Json (Join-Path $InstallRoot 'versions\v1.0.0\manifest.json') @{version='v1.0.0';databaseSchema=2;launcherProtocol=1;target='x86_64-pc-windows-msvc'}
    Assert-Fails { Update-Managed }
    Assert ($script:stops -eq 0 -and $script:starts.Count -eq 0) 'Cross-schema update touched the daemon.'
    Write-Json (Join-Path $InstallRoot 'versions\v1.0.0\manifest.json') @{version='v1.0.0';databaseSchema=5;launcherProtocol=1;target='x86_64-pc-windows-msvc'}
    Assert-Fails { Update-Managed }
    Assert (($script:starts -join ',') -eq 'v1.0.1,v1.0.0') "Failed startup did not restart old version: $script:lastFailure"
    Assert ((Read-Json (Join-Path $InstallRoot 'current.json')).version -eq 'v1.0.0') 'Failed update changed current version.'
    $script:starts = @(); $script:invalidateFallback = $true
    Assert-Fails { Update-Managed }
    Assert (($script:starts -join ',') -eq 'v1.0.1') 'An incompatible fallback binary was launched.'
    Assert ((Read-Json (Join-Path $InstallRoot 'current.json')).version -eq 'v1.0.0') 'Failed fallback changed the current pointer.'
    $script:invalidateFallback = $false; $script:failPreflight = $false
    function Start-Process { } # The updater must not open a real browser during this test.
    $script:failStartup = $false; Update-Managed
    Assert ((Read-Json (Join-Path $InstallRoot 'current.json')).version -eq 'v1.0.1') 'Successful update did not activate new version.'
    Assert ((Read-Json (Join-Path $InstallRoot 'previous.json')).version -eq 'v1.0.0') 'Previous binary reference was lost.'
    Write-Host 'PASS: manifest, checksum, archive safety, immutable install, PID guard, update success/failure recovery, and optional real daemon lifecycle.'
} finally {
    # Only this test's unique temporary directory is removed, never user data or installs.
    Remove-Item -LiteralPath $temp -Recurse -Force
}
