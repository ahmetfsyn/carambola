function Get-CarambolaReleaseVersion {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Tag,

        [Parameter(Mandatory = $true)]
        [string]$PackageVersion
    )

    if ($Tag -notmatch '^v(?<version>\d+\.\d+\.\d+)$') {
        throw "Release tag '$Tag' must use the vMAJOR.MINOR.PATCH format."
    }

    if ($Matches.version -ne $PackageVersion) {
        throw "Release tag '$Tag' does not match Cargo package version '$PackageVersion'."
    }

    return $Matches.version
}

Export-ModuleMember -Function Get-CarambolaReleaseVersion
