function ConvertTo-CarambolaPublicVersion {
    param(
        [Parameter(Mandatory = $true)]
        [string]$PackageVersion
    )

    $packageMatch = [regex]::Match(
        $PackageVersion,
        '^(?<major>0|[1-9]\d*)\.(?<minor>0|[1-9]\d*)\.0$'
    )
    if (!$packageMatch.Success) {
        throw "Cargo package version '$PackageVersion' must map to a public MAJOR.MINOR version with a zero patch component."
    }

    return "{0}.{1}" -f $packageMatch.Groups['major'].Value, $packageMatch.Groups['minor'].Value
}

function Get-CarambolaReleaseVersion {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Tag,

        [Parameter(Mandatory = $true)]
        [string]$PackageVersion
    )

    if ($Tag -notmatch '^v(?<version>(?:0|[1-9]\d*)\.(?:0|[1-9]\d*))$') {
        throw "Release tag '$Tag' must use the vMAJOR.MINOR format."
    }

    $tagVersion = $Matches.version
    $packagePublicVersion = ConvertTo-CarambolaPublicVersion -PackageVersion $PackageVersion
    if ($tagVersion -ne $packagePublicVersion) {
        throw "Release tag '$Tag' does not match public version '$packagePublicVersion' derived from Cargo package version '$PackageVersion'."
    }

    return $packagePublicVersion
}

Export-ModuleMember -Function ConvertTo-CarambolaPublicVersion, Get-CarambolaReleaseVersion
