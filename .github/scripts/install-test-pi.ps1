# CI-only, explicit private test installation. Never touches PATH or user/global Pi configuration.
param([Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference = 'Stop'
if (-not [IO.Path]::IsPathRooted($Destination) -or (Test-Path -LiteralPath $Destination)) { throw 'Select a new absolute test installation directory.' }
$null = New-Item -ItemType Directory -Path $Destination
$zip = Join-Path $Destination 'pi-windows-x64.zip'
Invoke-WebRequest -Uri 'https://github.com/earendil-works/pi/releases/download/v0.85.1/pi-windows-x64.zip' -OutFile $zip
if ((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant() -cne '002fa95b90d521245b9985d8f168caebc237ad56e7e30b319807dee1b2e17e1c') { throw 'Pi 0.85.1 archive SHA256 mismatch.' }
Expand-Archive -LiteralPath $zip -DestinationPath (Join-Path $Destination 'unpacked')
$files = @(Get-ChildItem -LiteralPath (Join-Path $Destination 'unpacked') -Recurse -File -Filter pi.exe)
if ($files.Count -ne 1) { throw 'Expected exactly one Pi executable.' }
$binary = $files[0].FullName
$version = @(& $binary --version)
if ($LASTEXITCODE -ne 0 -or $version.Count -ne 1 -or $version[0].Trim() -cne '0.85.1') { throw 'Unexpected Pi runtime version.' }
Write-Output $binary
