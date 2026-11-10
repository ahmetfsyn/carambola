$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'ReleaseMetadata.psm1') -Force

$validCases = @(
    @{ Tag = 'v1.0'; PackageVersion = '1.0.0'; PublicVersion = '1.0' },
    @{ Tag = 'v1.1'; PackageVersion = '1.1.0'; PublicVersion = '1.1' },
    @{ Tag = 'v2.0'; PackageVersion = '2.0.0'; PublicVersion = '2.0' }
)

foreach ($case in $validCases) {
    $publicVersion = ConvertTo-CarambolaPublicVersion -PackageVersion $case.PackageVersion
    if ($publicVersion -ne $case.PublicVersion) {
        throw "Unexpected public version derived from Cargo '$($case.PackageVersion)': $publicVersion"
    }
}

foreach ($case in $validCases) {
    $publicVersion = Get-CarambolaReleaseVersion -Tag $case.Tag -PackageVersion $case.PackageVersion
    if ($publicVersion -ne $case.PublicVersion) {
        throw "Unexpected public version for '$($case.Tag)': $publicVersion"
    }
}

$invalidCases = @(
    @{ Tag = '1.0'; PackageVersion = '1.0.0' },
    @{ Tag = 'v1.0.0'; PackageVersion = '1.0.0' },
    @{ Tag = 'v1.1'; PackageVersion = '1.0.0' },
    @{ Tag = 'v1.0-rc.1'; PackageVersion = '1.0.0' },
    @{ Tag = 'v1.0'; PackageVersion = '1.0.1' },
    @{ Tag = 'v01.0'; PackageVersion = '1.0.0' }
)

$invalidPackageVersions = @('1.0', '1.0.1', '01.0.0', '1.0.0-rc.1')
foreach ($packageVersion in $invalidPackageVersions) {
    $failed = $false
    try {
        ConvertTo-CarambolaPublicVersion -PackageVersion $packageVersion | Out-Null
    } catch {
        $failed = $true
    }
    if (!$failed) { throw "Public version mapping accepted invalid Cargo version '$packageVersion'." }
}

foreach ($case in $invalidCases) {
    $failed = $false
    try {
        Get-CarambolaReleaseVersion -Tag $case.Tag -PackageVersion $case.PackageVersion | Out-Null
    } catch {
        $failed = $true
    }
    if (!$failed) { throw "Release metadata guard accepted invalid case '$($case.Tag)' / '$($case.PackageVersion)'." }
}

Write-Output 'Release metadata validation tests passed.'
