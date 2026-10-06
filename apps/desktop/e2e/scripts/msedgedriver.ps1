<#
.SYNOPSIS
  Fetches the msedgedriver that matches the installed WebView2 runtime, for the end-to-end tests
  on Windows (docs/adr/0009-e2e-tooling-and-test-seams.md).

.DESCRIPTION
  tauri-driver drives the app through msedgedriver, which must be the exact version of the
  WebView2 runtime the app runs in. WebView2 updates itself, so the version is read on every run:

  1. the runtime's version from its EdgeUpdate client key (value "pv"), machine-wide or per user;
  2. msedgedriver of that version from Microsoft's official download endpoint, over HTTPS;
  3. the executable's Authenticode signature must be valid and Microsoft's, and its own
     "--version" must name the same version.

  It prints the driver's path. In GitHub Actions it also sets B2C_E2E_NATIVE_DRIVER for later steps
  (the harness passes it to tauri-driver as --native-driver).

.PARAMETER OutDir
  The folder to put msedgedriver.exe in (created if missing).

.EXAMPLE
  pwsh apps/desktop/e2e/scripts/msedgedriver.ps1 -OutDir $env:RUNNER_TEMP\msedgedriver
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [string] $OutDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

# The WebView2 runtime's product code in EdgeUpdate.
$runtimeClient = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
$clientKeys = @(
  "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$runtimeClient",
  "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$runtimeClient",
  "HKCU:\Software\Microsoft\EdgeUpdate\Clients\$runtimeClient"
)

$version = $null
foreach ($key in $clientKeys) {
  $entry = Get-ItemProperty -Path $key -Name 'pv' -ErrorAction SilentlyContinue
  if ($null -ne $entry -and $entry.pv -match '^\d+\.\d+\.\d+\.\d+$' -and $entry.pv -ne '0.0.0.0') {
    $version = $entry.pv
    break
  }
}
if ($null -eq $version) {
  throw 'The WebView2 runtime is not installed (no EdgeUpdate "pv" value was found).'
}
Write-Host "WebView2 runtime: $version"

$architecture = if ([Environment]::Is64BitOperatingSystem) { 'win64' } else { 'win32' }
$url = "https://msedgedriver.microsoft.com/$version/edgedriver_$architecture.zip"

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$zip = Join-Path $OutDir "edgedriver_$version.zip"
$extracted = Join-Path $OutDir "edgedriver_$version"

Write-Host "Downloading $url"
$attempt = 0
while ($true) {
  $attempt += 1
  try {
    Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing -MaximumRedirection 2
    break
  } catch {
    if ($attempt -ge 3) {
      throw "msedgedriver $version could not be downloaded: $($_.Exception.Message)"
    }
    Start-Sleep -Seconds (5 * $attempt)
  }
}

if (Test-Path $extracted) {
  Remove-Item -Recurse -Force $extracted
}
Expand-Archive -Path $zip -DestinationPath $extracted
$driver = Join-Path $extracted 'msedgedriver.exe'
if (-not (Test-Path $driver)) {
  throw 'The download has no msedgedriver.exe.'
}

$signature = Get-AuthenticodeSignature -FilePath $driver
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
  throw "msedgedriver.exe is not signed by Microsoft (status: $($signature.Status))."
}

$reported = & $driver --version
if ($LASTEXITCODE -ne 0 -or "$reported" -notmatch [regex]::Escape($version)) {
  throw "msedgedriver reports '$reported', not version $version."
}
Write-Host "msedgedriver: $reported"

if ($env:GITHUB_ENV) {
  Add-Content -Path $env:GITHUB_ENV -Value "B2C_E2E_NATIVE_DRIVER=$driver"
}
Write-Output $driver
