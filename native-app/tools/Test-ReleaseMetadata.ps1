$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'ReleaseMetadata.psm1') -Force

$valid = Get-CarambolaReleaseVersion -Tag 'v1.1.1' -PackageVersion '1.1.1'
if ($valid -ne '1.1.1') { throw "Unexpected parsed release version: $valid" }

$invalidCases = @(
    @{ Tag = '1.1.1'; Version = '1.1.1' },
    @{ Tag = 'v1.1'; Version = '1.1' },
    @{ Tag = 'v1.1.1'; Version = '1.1.0' },
    @{ Tag = 'v1.1.1-rc.1'; Version = '1.1.1-rc.1' }
)

foreach ($case in $invalidCases) {
    $failed = $false
    try {
        Get-CarambolaReleaseVersion -Tag $case.Tag -PackageVersion $case.Version | Out-Null
    } catch {
        $failed = $true
    }
    if (!$failed) { throw "Release metadata guard accepted invalid case '$($case.Tag)' / '$($case.Version)'." }
}

Write-Output 'Release metadata validation tests passed.'
