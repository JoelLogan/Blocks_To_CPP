#!/usr/bin/env pwsh
<#
.SYNOPSIS
Checks the application manifest embedded in Windows executables of the desktop app.

.DESCRIPTION
Reads the process manifest (the RT_MANIFEST resource with ID 1) embedded in each
executable and checks the two entries the app needs (docs/spec/08-security.md, 8.6):

  * the Microsoft.Windows.Common-Controls 6.0.0.0 dependency: without it the native
    dialogs (TaskDialogIndirect) cannot be loaded and the executable does not start;
  * longPathAware = true under application/windowsSettings, so paths longer than
    MAX_PATH (260 characters) work wherever Windows allows them (LongPathsEnabled).

The manifest comes from apps/desktop/src-tauri/windows-app-manifest.xml, which the
linker merges into every executable of the desktop crate (build.rs). The executable is
read as a file and its resource table parsed here, without Win32 calls, so the script
also runs with PowerShell on Linux or macOS. Namespaces are resolved, so a manifest
that a tool rewrote with prefixes (mt.exe does) still passes. The same checks run in
Rust in apps/desktop/src-tauri/tests/hardening.rs (B2C_CHECK_EXE=<path> checks any
executable there).

Exit status: 0 when every executable has both entries, 1 otherwise (each problem is
reported as a GitHub Actions error annotation, followed by the manifest's text).

.PARAMETER Path
One or more executables, for example target/release/blocks2cpp-desktop.exe.

.EXAMPLE
pwsh -NoProfile -File tools/check-windows-manifest.ps1 target/release/blocks2cpp-desktop.exe
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
    [string[]] $Path
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = 'Stop'

# RT_MANIFEST, and the ID of an executable's own manifest
# (CREATEPROCESS_MANIFEST_RESOURCE_ID).
$RtManifest = 24
$ProcessManifestId = 1
# Nothing the app ships comes close; a bigger file is not one of ours.
$MaxImageBytes = 1GB
# The high bit of a resource directory entry's offset marks a subdirectory.
$SubdirectoryBit = 2147483648

$AsmV1 = 'urn:schemas-microsoft-com:asm.v1'
$AsmV3 = 'urn:schemas-microsoft-com:asm.v3'
$WindowsSettings2016 = 'http://schemas.microsoft.com/SMI/2016/WindowsSettings'

if (-not [BitConverter]::IsLittleEndian) {
    throw 'this script reads PE fields as little-endian numbers and needs a little-endian machine'
}

function Read-UInt16 {
    param([byte[]] $Image, [long] $Offset)
    if ($Offset -lt 0 -or $Offset + 2 -gt $Image.LongLength) {
        throw [System.IO.InvalidDataException]::new("offset $Offset is outside the file")
    }
    return [long][BitConverter]::ToUInt16($Image, [int]$Offset)
}

function Read-UInt32 {
    param([byte[]] $Image, [long] $Offset)
    if ($Offset -lt 0 -or $Offset + 4 -gt $Image.LongLength) {
        throw [System.IO.InvalidDataException]::new("offset $Offset is outside the file")
    }
    return [long][BitConverter]::ToUInt32($Image, [int]$Offset)
}

# The file offset of a relative virtual address, through the section table; $null when
# no section holds it.
function ConvertTo-FileOffset {
    param([byte[]] $Image, [long] $SectionTable, [long] $Sections, [long] $Rva)
    for ($index = 0; $index -lt $Sections; $index++) {
        $header = $SectionTable + 40 * $index
        $virtualSize = Read-UInt32 $Image ($header + 8)
        $address = Read-UInt32 $Image ($header + 12)
        $rawSize = Read-UInt32 $Image ($header + 16)
        $raw = Read-UInt32 $Image ($header + 20)
        if ($Rva -ge $address -and $Rva -lt $address + [Math]::Max($virtualSize, $rawSize)) {
            return $raw + ($Rva - $address)
        }
    }
    return $null
}

# One level of the resource tree: the entry with ID $Id (the first entry when $Id is
# $null), as @{ IsDirectory; Offset }, or $null when there is none.
function Find-ResourceEntry {
    param([byte[]] $Image, [long] $Base, [long] $Directory, $Id)
    $count = (Read-UInt16 $Image ($Directory + 12)) + (Read-UInt16 $Image ($Directory + 14))
    for ($index = 0; $index -lt $count; $index++) {
        $at = $Directory + 16 + 8 * $index
        $name = Read-UInt32 $Image $at
        if ($null -ne $Id -and $name -ne $Id) {
            continue
        }
        $target = Read-UInt32 $Image ($at + 4)
        return @{
            IsDirectory = $target -ge $SubdirectoryBit
            Offset      = $Base + ($target % $SubdirectoryBit)
        }
    }
    return $null
}

# The process manifest embedded in a PE image, as text; $null when it has none.
function Get-EmbeddedManifest {
    param([byte[]] $Image)
    if ($Image.LongLength -lt 64 -or $Image[0] -ne 0x4D -or $Image[1] -ne 0x5A) {
        throw [System.IO.InvalidDataException]::new('not a Windows executable (no MZ header)')
    }
    $pe = Read-UInt32 $Image 0x3C
    # "PE\0\0"
    if ((Read-UInt32 $Image $pe) -ne 0x4550) {
        throw [System.IO.InvalidDataException]::new('not a PE image (no PE signature)')
    }
    $coff = $pe + 4
    $sections = Read-UInt16 $Image ($coff + 2)
    $optionalSize = Read-UInt16 $Image ($coff + 16)
    $optional = $coff + 20
    $magic = Read-UInt16 $Image $optional
    if ($magic -eq 0x10B) {
        $countAt = $optional + 92
        $directories = $optional + 96
    } elseif ($magic -eq 0x20B) {
        $countAt = $optional + 108
        $directories = $optional + 112
    } else {
        throw [System.IO.InvalidDataException]::new("unknown optional header magic $magic")
    }
    # The resource table is data directory 2.
    if ((Read-UInt32 $Image $countAt) -le 2) {
        return $null
    }
    $resourcesRva = Read-UInt32 $Image ($directories + 2 * 8)
    $sectionTable = $optional + $optionalSize
    $base = ConvertTo-FileOffset $Image $sectionTable $sections $resourcesRva
    if ($resourcesRva -eq 0 -or $null -eq $base) {
        return $null
    }
    $names = Find-ResourceEntry $Image $base $base $RtManifest
    if ($null -eq $names -or -not $names.IsDirectory) {
        return $null
    }
    $languages = Find-ResourceEntry $Image $base $names.Offset $ProcessManifestId
    if ($null -eq $languages -or -not $languages.IsDirectory) {
        return $null
    }
    $data = Find-ResourceEntry $Image $base $languages.Offset $null
    if ($null -eq $data -or $data.IsDirectory) {
        return $null
    }
    $start = ConvertTo-FileOffset $Image $sectionTable $sections (Read-UInt32 $Image $data.Offset)
    $size = Read-UInt32 $Image ($data.Offset + 4)
    if ($null -eq $start -or $start + $size -gt $Image.LongLength) {
        throw [System.IO.InvalidDataException]::new('the manifest resource lies outside the file')
    }
    return [System.Text.Encoding]::UTF8.GetString($Image, [int]$start, [int]$size)
}

# What is wrong with a manifest: an empty list when it has both entries.
function Get-ManifestProblem {
    param([string] $Text)
    $problems = [System.Collections.Generic.List[string]]::new()
    $settings = [System.Xml.XmlReaderSettings]::new()
    $settings.DtdProcessing = [System.Xml.DtdProcessing]::Prohibit
    $settings.XmlResolver = $null
    $document = [System.Xml.XmlDocument]::new()
    $document.XmlResolver = $null
    $reader = [System.Xml.XmlReader]::Create([System.IO.StringReader]::new($Text.TrimStart([char]0xFEFF)), $settings)
    try {
        $document.Load($reader)
    } catch {
        $problems.Add("the manifest is not well-formed XML: $($_.Exception.Message)")
        return , $problems
    } finally {
        $reader.Dispose()
    }
    $namespaces = [System.Xml.XmlNamespaceManager]::new($document.NameTable)
    $namespaces.AddNamespace('v1', $AsmV1)
    $namespaces.AddNamespace('v3', $AsmV3)
    $namespaces.AddNamespace('ws', $WindowsSettings2016)
    $commonControls = $document.SelectNodes(
        '/v1:assembly/v1:dependency/v1:dependentAssembly/v1:assemblyIdentity' +
        "[@type='win32' and @name='Microsoft.Windows.Common-Controls' and @version='6.0.0.0'" +
        " and @publicKeyToken='6595b64144ccf1df' and @processorArchitecture='*']",
        $namespaces)
    if ($commonControls.Count -eq 0) {
        $problems.Add('no Microsoft.Windows.Common-Controls 6.0.0.0 dependency')
    }
    $longPaths = @(
        $document.SelectNodes('/v1:assembly/v3:application/v3:windowsSettings/ws:longPathAware', $namespaces) |
            Where-Object { $_.InnerText.Trim() -eq 'true' }
    )
    if ($longPaths.Count -eq 0) {
        $problems.Add('no longPathAware = true under application/windowsSettings')
    }
    return , $problems
}

$failed = $false
foreach ($file in $Path) {
    $problems = [System.Collections.Generic.List[string]]::new()
    $manifest = $null
    try {
        $full = (Resolve-Path -LiteralPath $file).ProviderPath
        $length = (Get-Item -LiteralPath $full).Length
        if ($length -gt $MaxImageBytes) {
            throw [System.IO.InvalidDataException]::new("the file is $length bytes, more than $MaxImageBytes")
        }
        $manifest = Get-EmbeddedManifest ([System.IO.File]::ReadAllBytes($full))
        if ($null -eq $manifest) {
            $problems.Add('no embedded process manifest (RT_MANIFEST resource 1)')
        } else {
            $problems.AddRange((Get-ManifestProblem $manifest))
        }
    } catch {
        $problems.Add($_.Exception.Message)
    }
    if ($problems.Count -eq 0) {
        Write-Host "${file}: the manifest has Common Controls 6.0.0.0 and longPathAware = true"
        continue
    }
    $failed = $true
    foreach ($problem in $problems) {
        Write-Host "::error::${file}: $problem"
    }
    if ($null -ne $manifest) {
        Write-Host "The embedded manifest of ${file}:"
        Write-Host $manifest
    }
}
if ($failed) {
    exit 1
}
exit 0
