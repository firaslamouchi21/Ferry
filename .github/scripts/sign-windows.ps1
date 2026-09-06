param([Parameter(Mandatory = $true)][string]$Dir)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrEmpty($env:WINDOWS_CERT_PFX)) {
  Write-Host "sign-windows: WINDOWS_CERT_PFX not set - skipping Authenticode signing"
  exit 0
}

$pfx = Join-Path $env:RUNNER_TEMP "ferry-cert.pfx"
[IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERT_PFX))

$signtool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe" |
  Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName

Get-ChildItem (Join-Path $Dir "ferry*") -File |
  Where-Object { $_.Name -notmatch "SHA256SUMS" } |
  ForEach-Object {
    & $signtool sign /f $pfx /p $env:WINDOWS_CERT_PASSWORD `
      /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $_.FullName
    & $signtool verify /pa $_.FullName
  }

Remove-Item $pfx -Force
