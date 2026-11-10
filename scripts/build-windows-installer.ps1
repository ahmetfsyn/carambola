[CmdletBinding()]
param(
    [string]$Version,
    [string]$OutputDirectory,
    [string]$ExecutablePath,
    [string]$ISCCPath,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$nativeAppRoot = Join-Path $repositoryRoot 'native-app'
$manifestPath = Join-Path $nativeAppRoot 'Cargo.toml'
$metadataModule = Join-Path $nativeAppRoot 'tools/ReleaseMetadata.psm1'
$issPath = Join-Path $repositoryRoot 'installer/windows/carambola.iss'
$iconPath = Join-Path $nativeAppRoot 'assets/carambola.ico'
$installReadmePath = Join-Path $repositoryRoot 'installer/windows/INSTALLATION.txt'

function Resolve-IsccPath {
    param([string]$RequestedPath)

    if ($RequestedPath) {
        if (Test-Path -LiteralPath $RequestedPath -PathType Leaf) {
            return (Resolve-Path -LiteralPath $RequestedPath).Path
        }
        $command = Get-Command -Name $RequestedPath -CommandType Application -ErrorAction SilentlyContinue
        if ($command) { return $command.Source }
        throw "The requested ISCC compiler was not found: $RequestedPath"
    }

    $command = Get-Command -Name 'ISCC.exe' -CommandType Application -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }

    $candidates = [System.Collections.Generic.List[string]]::new()
    foreach ($programFiles in @($env:ProgramFiles, ${env:ProgramFiles(x86)})) {
        if ($programFiles) {
            foreach ($edition in @('Inno Setup 7', 'Inno Setup 6')) {
                $candidates.Add((Join-Path $programFiles "$edition/ISCC.exe"))
            }
        }
    }
    if ($env:LOCALAPPDATA) {
        foreach ($edition in @('Inno Setup 7', 'Inno Setup 6')) {
            $candidates.Add((Join-Path $env:LOCALAPPDATA "Programs/$edition/ISCC.exe"))
        }
    }
    # Some managed shells remap LOCALAPPDATA; the user profile still points at
    # the normal per-user Inno Setup installation shown in the Start Menu.
    if ($env:USERPROFILE) {
        foreach ($edition in @('Inno Setup 7', 'Inno Setup 6')) {
            $candidates.Add((Join-Path $env:USERPROFILE "AppData/Local/Programs/$edition/ISCC.exe"))
        }
    }

    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }

    return $null
}

$iscc = Resolve-IsccPath -RequestedPath $ISCCPath
if (!$iscc) {
    Write-Error @'
Inno Setup's ISCC.exe compiler was not found.
Install the 64-bit Inno Setup 7 compiler from https://jrsoftware.org/isdl.php,
then rerun this command. You can also pass -ISCCPath with the full ISCC.exe path.
This script does not download or execute third-party installers.
'@
    exit 1
}

Import-Module $metadataModule -Force
$metadataJson = & cargo metadata --manifest-path $manifestPath --no-deps --format-version 1
if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed; cannot determine the authoritative package version.' }
$metadata = $metadataJson | ConvertFrom-Json
$packages = @($metadata.packages | Where-Object { $_.name -eq 'carambola' })
if ($packages.Count -ne 1) { throw "Expected exactly one carambola Cargo package; found $($packages.Count)." }
$packageVersion = $packages[0].version
$canonicalVersion = ConvertTo-CarambolaPublicVersion -PackageVersion $packageVersion

if ($Version) {
    $requestedVersion = $Version.TrimStart('v')
    $publicVersion = Get-CarambolaReleaseVersion -Tag "v$requestedVersion" -PackageVersion $packageVersion
} else {
    $publicVersion = $canonicalVersion
}

if (!(Test-Path -LiteralPath $issPath -PathType Leaf)) { throw "Inno Setup script not found: $issPath" }
foreach ($requiredFile in @($iconPath, $installReadmePath)) {
    if (!(Test-Path -LiteralPath $requiredFile -PathType Leaf)) { throw "Installer support file not found: $requiredFile" }
}

if (!$SkipBuild) {
    Push-Location $nativeAppRoot
    try {
        & cargo build --locked --release
        if ($LASTEXITCODE -ne 0) { throw "cargo build --locked --release failed with exit code $LASTEXITCODE." }
    } finally {
        Pop-Location
    }
}

if ($ExecutablePath) {
    if ([System.IO.Path]::IsPathRooted($ExecutablePath)) {
        $builtExe = [System.IO.Path]::GetFullPath($ExecutablePath)
    } else {
        $builtExe = [System.IO.Path]::GetFullPath((Join-Path (Get-Location).Path $ExecutablePath))
    }
} else {
    if ($env:CARGO_TARGET_DIR) {
        $targetRoot = $env:CARGO_TARGET_DIR
        if (![System.IO.Path]::IsPathRooted($targetRoot)) {
            $targetRoot = Join-Path $nativeAppRoot $targetRoot
        }
    } else {
        $targetRoot = Join-Path $nativeAppRoot 'target'
    }
    $builtExe = Join-Path $targetRoot 'release/carambola.exe'

    if (!(Test-Path -LiteralPath $builtExe -PathType Leaf) -and $SkipBuild) {
        $packagedExe = Join-Path $repositoryRoot "release-artifacts/v$publicVersion/Carambola.exe"
        if (Test-Path -LiteralPath $packagedExe -PathType Leaf) { $builtExe = $packagedExe }
    }
}

if (!(Test-Path -LiteralPath $builtExe -PathType Leaf)) {
    throw "Release executable not found: $builtExe. Build native-app in release mode or pass -ExecutablePath."
}
$builtExe = (Resolve-Path -LiteralPath $builtExe).Path
$versionInfo = (Get-Item -LiteralPath $builtExe).VersionInfo
if (([string]$versionInfo.ProductName).Trim() -ne 'Carambola' -or
    ([string]$versionInfo.ProductVersion).Trim() -ne $publicVersion -or
    ([string]$versionInfo.FileVersion).Trim() -ne $publicVersion -or
    ([string]$versionInfo.OriginalFilename).Trim() -ne 'Carambola.exe') {
    throw "Release executable metadata does not match Carambola ${publicVersion}: $builtExe"
}

$bytes = [System.IO.File]::ReadAllBytes($builtExe)
if ($bytes.Length -lt 128 -or [BitConverter]::ToUInt16($bytes, 0) -ne 0x5a4d) {
    throw "Release executable is not a valid Windows PE file: $builtExe"
}
$peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
if ($peOffset -lt 0x40 -or $peOffset + 90 -gt $bytes.Length -or
    [BitConverter]::ToUInt32($bytes, $peOffset) -ne 0x00004550) {
    throw "Release executable has an invalid PE header: $builtExe"
}
$machine = [BitConverter]::ToUInt16($bytes, $peOffset + 4)
$subsystem = [BitConverter]::ToUInt16($bytes, $peOffset + 24 + 68)
if ($machine -ne 0x8664 -or $subsystem -ne 2) {
    throw ('Release executable must be x64 GUI (machine 0x8664, subsystem 2); got machine 0x{0:x4}, subsystem {1}.' -f $machine, $subsystem)
}

if (!$OutputDirectory) {
    $OutputDirectory = Join-Path $repositoryRoot "release-artifacts/v$publicVersion"
}
$outputDirectoryFull = [System.IO.Path]::GetFullPath($OutputDirectory)
$null = New-Item -ItemType Directory -Path $outputDirectoryFull -Force
$installerName = "Carambola-Setup-$publicVersion.exe"
$installerPath = Join-Path $outputDirectoryFull $installerName
if (Test-Path -LiteralPath $installerPath) {
    throw "Refusing to overwrite an existing installer: $installerPath"
}
$stageDirectory = Join-Path ([System.IO.Path]::GetTempPath()) "carambola-installer-$([guid]::NewGuid().ToString('N'))"
$stageDirectoryFull = [System.IO.Path]::GetFullPath($stageDirectory)
$stagedInstallerPath = Join-Path $stageDirectoryFull $installerName
$null = New-Item -ItemType Directory -Path $stageDirectoryFull

$environmentValues = @{
    CARAMBOLA_VERSION = $publicVersion
    CARAMBOLA_EXE = $builtExe
    CARAMBOLA_ICON = (Resolve-Path -LiteralPath $iconPath).Path
    CARAMBOLA_INSTALL_README = (Resolve-Path -LiteralPath $installReadmePath).Path
}
$oldEnvironmentValues = @{}
foreach ($name in $environmentValues.Keys) {
    $oldEnvironmentValues[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    [Environment]::SetEnvironmentVariable($name, $environmentValues[$name], 'Process')
}

try {
    try {
        & $iscc "--output-dir=$stageDirectoryFull" "--output-filename=Carambola-Setup-$publicVersion" $issPath
        if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE." }
    } finally {
        foreach ($name in $environmentValues.Keys) {
            [Environment]::SetEnvironmentVariable($name, $oldEnvironmentValues[$name], 'Process')
        }
    }

    if (!(Test-Path -LiteralPath $stagedInstallerPath -PathType Leaf)) {
        throw "ISCC reported success but did not create the installer: $stagedInstallerPath"
    }
    $installer = Get-Item -LiteralPath $stagedInstallerPath
    if ($installer.Length -le 0) { throw "Generated installer is empty: $stagedInstallerPath" }
    $installerVersionInfo = $installer.VersionInfo
    if (([string]$installerVersionInfo.ProductName).Trim() -ne 'Carambola' -or
        ([string]$installerVersionInfo.ProductVersion).Trim() -ne $publicVersion -or
        ([string]$installerVersionInfo.FileVersion).Trim() -ne $publicVersion -or
        ([string]$installerVersionInfo.OriginalFilename).Trim() -ne $installerName) {
        throw "Generated installer metadata does not match Carambola ${publicVersion}: $stagedInstallerPath"
    }
    $installerBytes = [System.IO.File]::ReadAllBytes($stagedInstallerPath)
    $installerPeOffset = [BitConverter]::ToInt32($installerBytes, 0x3c)
    if ($installerBytes.Length -lt 128 -or $installerPeOffset -lt 0x40 -or
        $installerPeOffset + 90 -gt $installerBytes.Length -or
        [BitConverter]::ToUInt32($installerBytes, $installerPeOffset) -ne 0x00004550) {
        throw "Generated installer has an invalid PE header: $stagedInstallerPath"
    }
    $installerMachine = [BitConverter]::ToUInt16($installerBytes, $installerPeOffset + 4)
    $installerSubsystem = [BitConverter]::ToUInt16($installerBytes, $installerPeOffset + 24 + 68)
    if ($installerMachine -ne 0x8664 -or $installerSubsystem -ne 2) {
        throw ('Generated installer must be x64 GUI (machine 0x8664, subsystem 2); got machine 0x{0:x4}, subsystem {1}.' -f $installerMachine, $installerSubsystem)
    }

    [System.IO.File]::Move($stagedInstallerPath, $installerPath)
} finally {
    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if (Test-Path -LiteralPath $stageDirectoryFull) {
        $actualStageDirectory = [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath $stageDirectoryFull).Path)
        if ($actualStageDirectory -ne $stageDirectoryFull -or
            !$actualStageDirectory.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to clean an unexpected installer staging path: $actualStageDirectory"
        }
        Remove-Item -LiteralPath $actualStageDirectory -Recurse -Force
    }
}

if (!(Test-Path -LiteralPath $installerPath -PathType Leaf)) {
    throw "Validated installer was not moved into the release directory: $installerPath"
}
$outputInstaller = Get-Item -LiteralPath $installerPath
if ($outputInstaller.Length -le 0) { throw "Installer in the release directory is empty: $installerPath" }
$sha256 = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()

Write-Output "Installer: $([System.IO.Path]::GetFullPath($installerPath))"
Write-Output "SHA-256: $sha256"
