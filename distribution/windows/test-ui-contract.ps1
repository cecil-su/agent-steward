# Offline contract tests with synthetic assets; no service, official package or user install.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'ui.ps1') -LibraryOnly
function Assert($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Assert-Fails([scriptblock]$Code) {
    $failed = $false
    try { & $Code } catch { $failed = $true }
    Assert $failed 'Expected rejection.'
}
$temp = Join-Path ([IO.Path]::GetTempPath()) ('steward-ui-contract-' + [guid]::NewGuid().ToString('N'))
try {
    $source = Join-Path $temp 'source'
    $assets = Join-Path $source 'web/dist'
    $null = New-Item -ItemType Directory -Path $assets
    [IO.File]::WriteAllText((Join-Path $assets 'index.html'), '<html><head><link href="/style.css"></head><body><script src="/app.js"></script></body></html>')
    [IO.File]::WriteAllText((Join-Path $assets 'app.js'), '// synthetic')
    [IO.File]::WriteAllText((Join-Path $assets 'style.css'), '/* synthetic */')
    $package = Join-Path $temp 'package'
    $built = Build-UiPackage $source $package 'contract-test'
    Assert ($built.Manifest.requiredApiContract -eq 5) 'Build mislabeled the API contract.'
    foreach ($contract in @(1,2,3,4,6)) {
        $manifest = $built.Manifest.PSObject.Copy()
        $manifest.requiredApiContract = $contract
        Write-UiJson (Join-Path $package 'manifest.json') $manifest
        Assert-Fails { Read-UiPackage $package }
    }
    Write-UiJson (Join-Path $package 'manifest.json') $built.Manifest
    Assert ((Read-UiPackage $package).Id -ceq $built.Id) 'Contract rejection changed assets.'
    $script:contract = 5
    function Invoke-RestMethod { @{packageFormat=1;apiContract=$script:contract;externalEnabled=$true} }
    $null = Get-UiStatus 'http://172.19.10.185:43123'
    $script:contract = 4
    Assert-Fails { Get-UiStatus 'http://172.19.10.185:43123' }
    Write-Host 'PASS: API 5 package/status contract and rejection of incompatible contracts; no listener started.'
} finally {
    if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Recurse -Force }
}
