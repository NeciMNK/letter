# Собирает пакет MSIX из target/release/letter.exe: манифест
# AppxManifest.xml с версией и издателем, значки из Assets.
# Версия — из метки v1.2.3 (переменная VERSION) или из Cargo.toml;
# издатель — WINDOWS_PUBLISHER («CN=…» сертификата) или CN=letter.
param([Parameter(Mandatory = $true)][string]$Out)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path "$PSScriptRoot/../.."
if ($env:VERSION -match '^v(\d+)\.(\d+)\.(\d+)') {
    $version = "$($Matches[1]).$($Matches[2]).$($Matches[3]).0"
} elseif ((Get-Content "$root/Cargo.toml" -Raw) -match '(?m)^version\s*=\s*"(\d+)\.(\d+)\.(\d+)') {
    $version = "$($Matches[1]).$($Matches[2]).$($Matches[3]).0"
} else {
    $version = '0.0.0.0'
}
$publisher = if ($env:WINDOWS_PUBLISHER) { $env:WINDOWS_PUBLISHER } else { 'CN=letter' }
$stage = Join-Path $env:RUNNER_TEMP 'msix'
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $stage | Out-Null
Copy-Item "$root/target/release/letter.exe" $stage
Copy-Item "$PSScriptRoot/Assets" $stage -Recurse
$manifest = (Get-Content "$PSScriptRoot/AppxManifest.xml" -Raw).
    Replace('VERSION_PLACEHOLDER', $version).
    Replace('PUBLISHER_PLACEHOLDER', [Security.SecurityElement]::Escape($publisher))
[IO.File]::WriteAllText((Join-Path $stage 'AppxManifest.xml'), $manifest, [Text.UTF8Encoding]::new($false))
$makeappx = Join-Path (& "$PSScriptRoot/sdk.ps1") 'makeappx.exe'
& $makeappx pack /d $stage /p $Out /o
if ($LASTEXITCODE -ne 0) { throw "makeappx завершился с кодом $LASTEXITCODE" }
Write-Host "MSIX $version ($publisher): $Out"
