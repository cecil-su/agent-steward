$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'update-local.ps1') -LibraryOnly
function Assert($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Assert-Fails([scriptblock]$Code) {
    $failed = $false
    try { & $Code } catch { $failed = $true }
    Assert $failed 'Expected failure.'
}
$temp = Join-Path ([IO.Path]::GetTempPath()) "steward-local-test-$([guid]::NewGuid().ToString('N'))"
$InstallRoot = Join-Path $temp 'app with spaces'
$repository = Join-Path $temp 'repo with spaces'
New-Item -ItemType Directory -Path $InstallRoot,(Join-Path $InstallRoot 'versions'),(Join-Path $repository 'distribution\windows') | Out-Null
try {
    [IO.File]::WriteAllText((Join-Path $repository 'Cargo.lock'), 'synthetic lock file')
    foreach ($file in @('steward.ps1','Start.cmd','Update.cmd','Stop.cmd','README.md')) {
        [IO.File]::WriteAllText((Join-Path $repository "distribution\windows\$file"), 'synthetic')
    }
    $script:failBuild = $true; $script:switches = 0
    function Invoke-LocalCargo([string]$Repository, [string]$Cache) {
        Assert ($Repository -eq $repository) 'Wrong source root.'
        Assert ($Cache -eq (Join-Path $InstallRoot 'build-cache')) 'Build did not use isolated cache.'
        if ($script:failBuild) { throw 'synthetic compilation failure' }
        $binaries = Join-Path $Cache 'x86_64-pc-windows-msvc\release'
        New-Item -ItemType Directory -Force -Path $binaries | Out-Null
        foreach ($file in @('taskd.exe','taskctl.exe','task-hook.exe')) { [IO.File]::WriteAllText((Join-Path $binaries $file), 'synthetic executable') }
    }
    function Switch-Managed([string]$Version) {
        $script:switches++
        Assert-BuildId $Version
        Assert ($Version.StartsWith('local-')) 'Local build disguised as official release.'
        Assert (Test-Path (Join-Path $InstallRoot "versions\$Version\taskd.exe")) 'Switch happened before complete staging.'
        return 'http://127.0.0.1:43123'
    }
    Write-Json (Join-Path $InstallRoot 'settings.json') @{bind='172.19.10.185';port=43123;database='C:\data\tasks.db';runtimeDir='C:\data\runtime';requireLocalAuth=$false}
    $settings = [IO.File]::ReadAllText((Join-Path $InstallRoot 'settings.json'))
    $lockBefore = [IO.File]::ReadAllText((Join-Path $repository 'Cargo.lock'))
    Assert-Fails { Update-LocalManaged $repository }
    Assert ($script:switches -eq 0) 'Compile failure touched running service.'
    $script:failBuild = $false
    $null = Update-LocalManaged $repository
    $null = Update-LocalManaged $repository
    Assert ($script:switches -eq 2) 'Successful builds did not switch.'
    Assert (@(Get-ChildItem (Join-Path $InstallRoot 'versions')).Count -eq 2) 'Repeated local builds reused a version directory.'
    Assert ([IO.File]::ReadAllText((Join-Path $InstallRoot 'settings.json')) -ceq $settings) 'Update changed settings.'
    Assert ([IO.File]::ReadAllText((Join-Path $repository 'Cargo.lock')) -ceq $lockBefore) 'Update changed source lockfile.'
    Assert-Fails { Assert-BuildId 'local-../../escape' }
    Write-Host 'PASS: local compile failure isolation, fresh build identities, stage-before-switch, source/config preservation.'
} finally { Remove-Item -LiteralPath $temp -Recurse -Force }
