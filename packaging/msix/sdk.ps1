# Папка утилит Windows SDK (makeappx.exe, signtool.exe) — самая новая.
$kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$found = Get-ChildItem $kits -Directory -Filter '10.*' |
    Sort-Object { [version]$_.Name } |
    Where-Object { Test-Path (Join-Path $_.FullName 'x64\makeappx.exe') } |
    Select-Object -Last 1
if (-not $found) { throw "Windows SDK не найден в $kits" }
Join-Path $found.FullName 'x64'
