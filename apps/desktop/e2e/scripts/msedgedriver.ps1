#Requires -Version 7.3
<#
.SYNOPSIS
  Fetches the msedgedriver that matches the installed WebView2 runtime, for the end-to-end tests
  on Windows (docs/adr/0009-e2e-tooling-and-test-seams.md).

.DESCRIPTION
  tauri-driver drives the app through msedgedriver, which must be the exact version of the
  WebView2 runtime the app runs in. WebView2 updates itself, so the version is read on every run:

  1. the runtime's version from its EdgeUpdate client key (value "pv"), machine-wide or per user;
  2. msedgedriver of that version from Microsoft's official download endpoint, over HTTPS;
  3. only msedgedriver.exe is taken from the archive, into a folder of its own (nothing unchecked,
     such as a DLL, ends up next to it);
  4. its Authenticode signature must be valid, its signer exactly Microsoft Corporation
     (the whole subject, compared exactly) and its issuer one of Microsoft's own certificate
     authorities (organisation "Microsoft Corporation", read from the parsed name); its own
     "--version" must name exactly the same version.

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

# The subject of Microsoft's code-signing certificates, exactly as .NET writes it. A value that
# holds a comma or an equals sign is quoted there, so no other name can produce this text.
$microsoftSigner = 'CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US'

# The OID of the organisation attribute (O) of a distinguished name.
$organizationOid = '2.5.4.10'

<#
.SYNOPSIS
  The values of attribute $Oid in distinguished name $Name, one per relative name that holds only
  that attribute. The name is parsed (.NET 7 or later), so text inside another attribute's value
  can never pass for one.
#>
function Get-NameAttribute {
  param(
    [Parameter(Mandatory = $true)]
    [System.Security.Cryptography.X509Certificates.X500DistinguishedName] $Name,
    [Parameter(Mandatory = $true)]
    [string] $Oid
  )
  foreach ($relativeName in $Name.EnumerateRelativeDistinguishedNames($true)) {
    if (-not $relativeName.HasMultipleElements -and
        $relativeName.GetSingleElementType().Value -ceq $Oid) {
      $relativeName.GetSingleElementValue()
    }
  }
}

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

# Only the executable leaves the archive, into a folder of its own: Windows looks for DLLs in an
# executable's folder first, and nothing else from the download is checked.
if (Test-Path $extracted) {
  Remove-Item -Recurse -Force $extracted
}
New-Item -ItemType Directory -Path $extracted | Out-Null
$driver = Join-Path $extracted 'msedgedriver.exe'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($zip)
try {
  $entries = @($archive.Entries | Where-Object { $_.FullName -ceq 'msedgedriver.exe' })
  if ($entries.Count -ne 1) {
    throw "The download has $($entries.Count) msedgedriver.exe entries at its top, not one."
  }
  [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entries[0], $driver)
} finally {
  $archive.Dispose()
}

$signature = Get-AuthenticodeSignature -FilePath $driver
if ($signature.Status -ne 'Valid') {
  throw "msedgedriver.exe has no valid signature (status: $($signature.Status))."
}
$certificate = $signature.SignerCertificate
$issuerOrganizations = @(Get-NameAttribute -Name $certificate.IssuerName -Oid $organizationOid)
if ($certificate.Subject -cne $microsoftSigner -or
    $issuerOrganizations.Count -ne 1 -or
    $issuerOrganizations[0] -cne 'Microsoft Corporation') {
  throw "msedgedriver.exe is not signed by Microsoft (signer: $($certificate.Subject); issuer: $($certificate.Issuer))."
}

# The output names the version as a word of its own ("MSEdgeDriver 141.0.3537.57 (...)" or
# "Microsoft Edge WebDriver 141.0.3537.57 (...)"): 141.0.3537.5 must not pass for 141.0.3537.57.
$reported = & $driver --version
$words = @("$reported" -split '\s+')
if ($LASTEXITCODE -ne 0 -or $words -cnotcontains $version) {
  throw "msedgedriver reports '$reported', not version $version."
}
Write-Host "msedgedriver: $reported"

if ($env:GITHUB_ENV) {
  Add-Content -Path $env:GITHUB_ENV -Value "B2C_E2E_NATIVE_DRIVER=$driver"
}
Write-Output $driver
