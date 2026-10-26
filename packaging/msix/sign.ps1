# Подписывает файлы сертификатом из переменных WINDOWS_CERTIFICATE (.pfx
# в base64) и WINDOWS_CERTIFICATE_PASSWORD. Без сертификата ничего не
# делает: файлы остаются неподписанными.
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Files)
$ErrorActionPreference = 'Stop'
if (-not $env:WINDOWS_CERTIFICATE) {
    Write-Host 'Сертификата нет — подпись пропущена.'
    exit 0
}
$signtool = Join-Path (& "$PSScriptRoot/sdk.ps1") 'signtool.exe'
$pfx = Join-Path $env:RUNNER_TEMP 'letter-cert.pfx'
[IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
try {
    & $signtool sign /fd SHA256 /f $pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD `
        /tr http://timestamp.digicert.com /td SHA256 $Files
    if ($LASTEXITCODE -ne 0) { throw "signtool завершился с кодом $LASTEXITCODE" }
} finally {
    Remove-Item $pfx -Force
}
