use semver::Version;
use serde::Deserialize;
use windows::{
    core::PCWSTR,
    Win32::Security::Cryptography::{
        BCryptCloseAlgorithmProvider, BCryptHash, BCryptOpenAlgorithmProvider, BCRYPT_ALG_HANDLE,
        BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS, BCRYPT_SHA256_ALGORITHM,
    },
};

const REPOSITORY: &str = "ahmetfsyn/carambola";
const APPLICATION_EXE: &str = "Carambola.exe";
const CHECKSUMS_FILE: &str = "SHA256SUMS.txt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOffer {
    pub version: Version,
    pub tag: String,
    pub installer_name: String,
    pub installer_url: String,
    pub checksums_url: String,
}

#[derive(Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

pub fn compatible_update(
    current_version: &str,
    releases: &[Release],
) -> Result<Option<UpdateOffer>, String> {
    let current = Version::parse(current_version)
        .map_err(|error| format!("The installed Carambola version is invalid: {error}"))?;
    if !current.pre.is_empty() || !current.build.is_empty() {
        return Err("The installed Carambola version must be a stable semantic version".into());
    }

    let mut candidates = releases
        .iter()
        .filter_map(|release| {
            if release.draft || release.prerelease {
                return None;
            }
            let version = parse_public_version(&release.tag_name)?;
            if version <= current {
                return None;
            }
            let installer_name = format!("Carambola-Setup-{}.{}.exe", version.major, version.minor);
            let portable_name = format!(
                "Carambola-{}.{}-Windows-x64.zip",
                version.major, version.minor
            );
            let expected_assets = [
                APPLICATION_EXE,
                portable_name.as_str(),
                installer_name.as_str(),
                CHECKSUMS_FILE,
            ];
            if !expected_assets.iter().all(|name| {
                release.assets.iter().any(|asset| {
                    asset.name == *name
                        && asset.browser_download_url == release_asset_url(&release.tag_name, name)
                })
            }) {
                return None;
            }

            Some(UpdateOffer {
                version,
                tag: release.tag_name.clone(),
                installer_url: release_asset_url(&release.tag_name, &installer_name),
                checksums_url: release_asset_url(&release.tag_name, CHECKSUMS_FILE),
                installer_name,
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.version.cmp(&left.version));
    Ok(candidates.into_iter().next())
}

pub fn parse_releases_json(json: &[u8]) -> Result<Vec<Release>, String> {
    serde_json::from_slice(json)
        .map_err(|error| format!("GitHub returned invalid release data: {error}"))
}

pub fn expected_installer_checksum(manifest: &[u8], filename: &str) -> Result<String, String> {
    let text = std::str::from_utf8(manifest)
        .map_err(|_| "The release checksum manifest is not valid UTF-8".to_string())?;
    let mut match_value = None;
    for line in text.lines() {
        let mut parts = line.split_ascii_whitespace();
        let Some(checksum) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else { continue };
        if parts.next().is_some() || name != filename {
            continue;
        }
        if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("The checksum for {filename} is malformed"));
        }
        if match_value.is_some() {
            return Err(format!(
                "The release checksum manifest lists {filename} more than once"
            ));
        }
        match_value = Some(checksum.to_ascii_lowercase());
    }
    match_value.ok_or_else(|| format!("The release checksum manifest does not list {filename}"))
}

pub fn sha256_hex(bytes: &[u8]) -> Result<String, String> {
    let mut algorithm = BCRYPT_ALG_HANDLE::default();
    let status = unsafe {
        BCryptOpenAlgorithmProvider(
            &mut algorithm,
            BCRYPT_SHA256_ALGORITHM,
            PCWSTR::null(),
            BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
        )
    };
    if status.0 < 0 {
        return Err(format!("Could not initialize Windows SHA-256: {status:?}"));
    }
    let mut digest = [0u8; 32];
    let hash_status = unsafe { BCryptHash(algorithm, None, bytes, &mut digest) };
    let close_status = unsafe { BCryptCloseAlgorithmProvider(algorithm, 0) };
    if hash_status.0 < 0 {
        return Err(format!("Windows SHA-256 failed: {hash_status:?}"));
    }
    if close_status.0 < 0 {
        return Err(format!("Could not close Windows SHA-256: {close_status:?}"));
    }
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn verify_installer(bytes: &[u8], expected_checksum: &str) -> Result<(), String> {
    let expected = expected_checksum.to_ascii_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("The published installer checksum is malformed".into());
    }
    let actual = sha256_hex(bytes)?;
    if actual != expected {
        return Err("The downloaded installer failed SHA-256 verification".into());
    }
    verify_x64_gui_installer(bytes)
}

fn verify_x64_gui_installer(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 128 || bytes.get(..2) != Some(b"MZ") {
        return Err("The downloaded update is not a valid Windows installer".into());
    }
    let pe_offset = u32::from_le_bytes(
        bytes[0x3c..0x40]
            .try_into()
            .expect("the minimum PE header length was checked"),
    ) as usize;
    if pe_offset < 0x40
        || pe_offset
            .checked_add(94)
            .is_none_or(|end| end > bytes.len())
    {
        return Err("The downloaded update has an invalid Windows executable header".into());
    }
    if bytes.get(pe_offset..pe_offset + 4) != Some(b"PE\0\0") {
        return Err("The downloaded update has an invalid Windows executable signature".into());
    }
    let machine = read_u16(bytes, pe_offset + 4)?;
    let subsystem = read_u16(bytes, pe_offset + 24 + 68)?;
    if machine != 0x8664 || subsystem != 2 {
        return Err(format!(
            "The downloaded update is not a Windows x64 GUI installer (machine 0x{machine:04x}, subsystem {subsystem})"
        ));
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let pair = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| "The downloaded update has a truncated executable header".to_string())?;
    Ok(u16::from_le_bytes([pair[0], pair[1]]))
}

fn parse_public_version(tag: &str) -> Option<Version> {
    let version = tag.strip_prefix('v')?;
    let mut parts = version.split('.');
    let major_text = parts.next()?;
    let minor_text = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let parse_component = |text: &str| {
        if text.is_empty() || (text.len() > 1 && text.starts_with('0')) {
            None
        } else {
            text.parse::<u64>().ok()
        }
    };
    Some(Version::new(
        parse_component(major_text)?,
        parse_component(minor_text)?,
        0,
    ))
}

fn release_asset_url(tag: &str, name: &str) -> String {
    format!("https://github.com/{REPOSITORY}/releases/download/{tag}/{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, draft: bool, prerelease: bool, include_setup: bool) -> Release {
        let version = parse_public_version(tag).expect("public tag");
        let installer = format!("Carambola-Setup-{}.{}.exe", version.major, version.minor);
        let portable = format!(
            "Carambola-{}.{}-Windows-x64.zip",
            version.major, version.minor
        );
        let mut names = vec![APPLICATION_EXE, portable.as_str(), CHECKSUMS_FILE];
        if include_setup {
            names.push(installer.as_str());
        }
        Release {
            tag_name: tag.into(),
            draft,
            prerelease,
            assets: names
                .into_iter()
                .map(|name| ReleaseAsset {
                    name: name.into(),
                    browser_download_url: release_asset_url(tag, name),
                })
                .collect(),
        }
    }

    #[test]
    fn offers_only_the_newest_complete_stable_windows_release() {
        let releases = vec![
            release("v1.4", false, false, true),
            release("v1.2", false, false, true),
            release("v1.5", true, false, true),
            release("v1.6", false, true, true),
            release("v1.7", false, false, false),
        ];
        let offer = compatible_update("1.0.0", &releases)
            .expect("valid installed version")
            .expect("compatible update");
        assert_eq!(offer.version, Version::new(1, 4, 0));
        assert_eq!(offer.tag, "v1.4");
        assert_eq!(offer.installer_name, "Carambola-Setup-1.4.exe");
        assert_eq!(
            offer.installer_url,
            "https://github.com/ahmetfsyn/carambola/releases/download/v1.4/Carambola-Setup-1.4.exe"
        );
    }

    #[test]
    fn reports_no_update_for_current_or_older_versions() {
        let releases = vec![release("v1.0", false, false, true)];
        assert_eq!(compatible_update("1.0.0", &releases).unwrap(), None);
        assert_eq!(compatible_update("1.3.0", &releases).unwrap(), None);
    }

    #[test]
    fn skips_historical_three_component_tags_and_missing_assets() {
        let mut historical_release = release("v1.1", false, false, true);
        historical_release.tag_name = "v1.1.0".into();
        let releases = vec![historical_release, release("v1.2", false, false, false)];
        assert_eq!(compatible_update("1.0.0", &releases).unwrap(), None);
    }

    #[test]
    fn checksum_parser_requires_one_well_formed_entry_for_the_installer() {
        let data = b"a3f7868516cf4a07a8224b3190b376cd98e5f5d5312ca86973b873100d4af2af  Carambola-Setup-1.2.exe\n";
        assert_eq!(
            expected_installer_checksum(data, "Carambola-Setup-1.2.exe").unwrap(),
            "a3f7868516cf4a07a8224b3190b376cd98e5f5d5312ca86973b873100d4af2af"
        );
        assert!(expected_installer_checksum(data, "Carambola-Setup-1.3.exe").is_err());
        assert!(expected_installer_checksum(
            b"bad  Carambola-Setup-1.2.exe\n",
            "Carambola-Setup-1.2.exe"
        )
        .is_err());
    }

    #[test]
    fn rejects_installer_checksum_mismatch() {
        let installer = x64_gui_pe();
        let expected = sha256_hex(&installer).unwrap();
        assert!(verify_installer(&installer, &expected).is_ok());
        let modified = b"modified installer";
        assert!(verify_installer(modified, &expected).is_err());
    }

    #[test]
    fn rejects_installers_for_other_architectures_or_subsystems() {
        let mut installer = x64_gui_pe();
        installer[0x44..0x46].copy_from_slice(&0x014cu16.to_le_bytes());
        let expected = sha256_hex(&installer).unwrap();
        assert!(verify_installer(&installer, &expected).is_err());

        let mut installer = x64_gui_pe();
        installer[0x40 + 24 + 68..0x40 + 24 + 70].copy_from_slice(&3u16.to_le_bytes());
        let expected = sha256_hex(&installer).unwrap();
        assert!(verify_installer(&installer, &expected).is_err());
    }

    fn x64_gui_pe() -> Vec<u8> {
        let mut bytes = vec![0u8; 256];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        bytes[0x40..0x44].copy_from_slice(b"PE\0\0");
        bytes[0x44..0x46].copy_from_slice(&0x8664u16.to_le_bytes());
        bytes[0x40 + 24 + 68..0x40 + 24 + 70].copy_from_slice(&2u16.to_le_bytes());
        bytes
    }

    #[test]
    fn parses_a_release_list_without_relying_on_the_live_release_api() {
        let json = br#"[{"tag_name":"v1.3","draft":false,"prerelease":false,"assets":[]}]"#;
        assert_eq!(parse_releases_json(json).unwrap().len(), 1);
        assert!(parse_releases_json(b"not json").is_err());
    }
}
