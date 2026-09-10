param(
    [Parameter(Mandatory=$true)][string]$Version,
    [string]$Binaries = (Join-Path $PSScriptRoot '../../target/release'),
    [Parameter(Mandatory=$true)][string]$Output
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'steward.ps1') -LibraryOnly
Assert-Version $Version
$Output = [IO.Path]::GetFullPath($Output)
# Verify trusted binary metadata before creating any release output.
$probe = Join-Path ([IO.Path]::GetTempPath()) ('steward-package-probe-' + [guid]::NewGuid().ToString('N') + '\absent.db')
$result = @(& (Join-Path $Binaries 'taskd.exe') --check-database-schema --database $probe)
if ($LASTEXITCODE -ne 0 -or $result.Count -ne 1 -or $result[0] -cne 'databaseSchema=6') { throw 'Selected taskd does not match Schema 6; no package created.' }
if (Test-Path -LiteralPath ([IO.Path]::GetDirectoryName($probe))) { throw 'Schema probe unexpectedly initialized a directory.' }
New-Item -ItemType Directory -Path $Output | Out-Null
$bundle = Join-Path $Output 'bundle'
New-Item -ItemType Directory -Path $bundle | Out-Null
foreach ($file in @('taskd.exe','taskctl.exe','task-hook.exe')) { Copy-Item -LiteralPath (Join-Path $Binaries $file) -Destination $bundle }
foreach ($file in @('steward.ps1','Start.cmd','Update.cmd','Stop.cmd','README.md')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination $bundle }
Write-Json (Join-Path $bundle 'manifest.json') @{version=$Version;databaseSchema=6;launcherProtocol=1;uiPackageProtocol=1;target='x86_64-pc-windows-msvc'}
$zip = Join-Path $Output 'agent-steward-windows-x64.zip'
Compress-Archive -Path (Join-Path $bundle '*') -DestinationPath $zip
$hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$zip.sha256", "$hash  agent-steward-windows-x64.zip`n", [Text.UTF8Encoding]::new($false))
Write-Host $zip
