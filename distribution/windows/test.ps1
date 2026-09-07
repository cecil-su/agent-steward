param([string]$Taskd)
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
$InstallRoot = Join-Path $temp 'install with spaces'
New-Item -ItemType Directory -Path $InstallRoot,(Join-Path $InstallRoot 'versions'),(Join-Path $InstallRoot 'runs') | Out-Null
try {
    Assert-Fails { Assert-Version '../v1.0.0' }
    Assert-Fails { Assert-Manifest @{version='v1.0.0';databaseSchema=3;launcherProtocol=1;target='x86_64-pc-windows-msvc'} }
    Assert-Fails { Assert-Settings @{bind='0.0.0.0';port=43123;database='C:\tasks.db';runtimeDir='C:\runtime';requireLocalAuth=$false} }
    $bundle = Join-Path $temp 'bundle'; New-Item -ItemType Directory -Path $bundle | Out-Null
    foreach ($file in @('taskd.exe','taskctl.exe','task-hook.exe','steward.ps1','Start.cmd','Update.cmd','Stop.cmd','README.md')) { [IO.File]::WriteAllText((Join-Path $bundle $file), 'synthetic') }
    Write-Json (Join-Path $bundle 'manifest.json') @{version='v1.0.1';databaseSchema=2;launcherProtocol=1;target='x86_64-pc-windows-msvc'}
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
        $settings = @{bind='127.0.0.1';port=$port;database=(Join-Path $temp 'tasks.db');runtimeDir=(Join-Path $temp 'runtime');requireLocalAuth=$false}
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
        } finally { Stop-Managed }
        # Restore the synthetic binary for immutable-package retry checks below.
        [IO.File]::WriteAllText((Join-Path $InstallRoot 'versions\v1.0.1\taskd.exe'), 'synthetic')
    }
    # Offline update orchestration: download first; failure restarts the old version.
    $script:stops = 0; $script:starts = @(); $script:failDownload = $false; $script:failStartup = $true
    function Stop-Managed { $script:stops++ }
    function Start-Managed([string]$Version) { $script:starts += $Version; if ($script:failStartup -and $Version -eq 'v1.0.1') { throw 'synthetic startup failure' }; return 'http://127.0.0.1:43123' }
    function Invoke-RestMethod { return @{tag_name='v1.0.1';assets=@(@{name='agent-steward-windows-x64.zip';browser_download_url='https://github.com/cecil-su/agent-steward/releases/download/v1.0.1/agent-steward-windows-x64.zip'},@{name='agent-steward-windows-x64.zip.sha256';browser_download_url='https://github.com/cecil-su/agent-steward/releases/download/v1.0.1/agent-steward-windows-x64.zip.sha256'})} }
    function Invoke-WebRequest($Uri,$OutFile,[switch]$UseBasicParsing) {
        if ($script:failDownload) { throw 'synthetic download failure' }
        if ($Uri.EndsWith('.sha256')) { Copy-Item $sum $OutFile } else { Copy-Item $zip $OutFile }
    }
    Write-Json (Join-Path $InstallRoot 'current.json') @{version='v1.0.0'}
    $script:failDownload = $true; Assert-Fails { Update-Managed }; Assert ($script:stops -eq 0) 'Download failure stopped the daemon.'
    $script:failDownload = $false; Assert-Fails { Update-Managed }
    Assert (($script:starts -join ',') -eq 'v1.0.1,v1.0.0') "Failed startup did not restart old version: $script:lastFailure"
    Assert ((Read-Json (Join-Path $InstallRoot 'current.json')).version -eq 'v1.0.0') 'Failed update changed current version.'
    function Start-Process { } # The updater must not open a real browser during this test.
    $script:failStartup = $false; Update-Managed
    Assert ((Read-Json (Join-Path $InstallRoot 'current.json')).version -eq 'v1.0.1') 'Successful update did not activate new version.'
    Assert ((Read-Json (Join-Path $InstallRoot 'previous.json')).version -eq 'v1.0.0') 'Previous binary reference was lost.'
    Write-Host 'PASS: manifest, checksum, archive safety, immutable install, PID guard, update success/failure recovery, and optional real daemon lifecycle.'
} finally {
    # Only this test's unique temporary directory is removed, never user data or installs.
    Remove-Item -LiteralPath $temp -Recurse -Force
}
