param(
    [Parameter(Mandatory = $true)]
    [string]$Tag,

    [switch]$SkipGit
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'ReleaseMetadata.psm1') -Force

$metadataJson = cargo metadata --no-deps --format-version 1
if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed; cannot validate the release package.' }
$metadata = $metadataJson | ConvertFrom-Json
$packages = @($metadata.packages | Where-Object { $_.name -eq 'carambola-native' })
if ($packages.Count -ne 1) { throw "Expected exactly one carambola-native package; found $($packages.Count)." }
$package = $packages[0]
$version = Get-CarambolaReleaseVersion -Tag $Tag -PackageVersion $package.version

$binaryTargets = @($package.targets | Where-Object { $_.kind -contains 'bin' })
if ($binaryTargets.Count -ne 1 -or $binaryTargets[0].name -ne 'carambola-native') {
    throw 'Expected one carambola-native executable target; release naming would be ambiguous.'
}

if (!$SkipGit) {
    $head = (git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Could not determine the checked-out commit.' }
    $tagCommit = (git rev-parse ($Tag + '^{commit}')).Trim()
    if ($LASTEXITCODE -ne 0) { throw "Could not resolve release tag '$Tag' to a commit." }
    if ($head -ne $tagCommit) { throw "Checkout HEAD $head does not match tag $Tag ($tagCommit)." }
}

Write-Output "Validated native release tag $Tag, Cargo version $version, and executable target $($binaryTargets[0].name)."
