//! Download size of a Steam game that isn't installed, from its app info (PICS) on Steam's servers.

use std::time::Duration;

use keyvalues_parser::Vdf;
use steam_vent::ConnectionTrait;
use steam_vent_proto_steam::steammessages_clientserver_appinfo::{
    CMsgClientPICSProductInfoRequest, CMsgClientPICSProductInfoResponse,
    cmsg_client_picsproduct_info_request::AppInfo,
};

use crate::library::steam::child;
use crate::steam_owned;

const TIMEOUT: Duration = Duration::from_secs(60);

/// Bytes a Windows install downloads: the depots for Windows (or any OS), no DLC, English or no language.
/// Each depot counts its compressed `download` size, else its `size`.
pub fn parse_download_size(appinfo_vdf: &str) -> Option<u64> {
    let vdf = Vdf::from(keyvalues_parser::parse(appinfo_vdf).ok()?);
    let depots = child(&vdf.value, "depots")?.get_obj()?;
    let text = |v, keys: &[&str]| {
        keys.iter()
            .try_fold(v, |v, k| child(v, k))?
            .get_str()
            .map(str::to_owned)
    };
    let total: u64 = depots
        .0
        .iter()
        .filter(|(id, _)| id.parse::<u32>().is_ok())
        .filter_map(|(_, v)| v.first())
        .filter(|d| {
            let windows = text(d, &["config", "oslist"])
                .is_none_or(|os| os.split(',').any(|o| o.trim() == "windows"));
            let english =
                text(d, &["config", "language"]).is_none_or(|l| l.is_empty() || l == "english");
            windows && english && child(d, "dlcappid").is_none()
        })
        .map(|d| {
            let bytes = |key| text(d, &["manifests", "public", key])?.parse::<u64>().ok();
            bytes("download")
                .filter(|&b| b > 0)
                .or_else(|| bytes("size"))
                .unwrap_or(0)
        })
        .sum();
    (total > 0).then_some(total)
}

/// The app's info as text VDF. Logs on with the saved token like the owned-games fetch and drops the
/// connection when done.
pub async fn fetch_appinfo(token: &str, appid: u32) -> Result<String, String> {
    tokio::time::timeout(TIMEOUT, async {
        let session = steam_owned::connect(token)
            .await
            .map_err(|e| format!("{e:?}"))?;
        let request = CMsgClientPICSProductInfoRequest {
            apps: vec![AppInfo {
                appid: Some(appid),
                only_public_obsolete: Some(true),
                ..Default::default()
            }],
            meta_data_only: Some(false),
            single_response: Some(true),
            ..Default::default()
        };
        let response: CMsgClientPICSProductInfoResponse =
            session.job(request).await.map_err(|e| e.to_string())?;
        let buffer = response
            .apps
            .first()
            .and_then(|a| a.buffer.as_deref())
            .ok_or("no app info")?;
        // The buffer is text VDF with a trailing NUL.
        Ok(String::from_utf8_lossy(buffer)
            .trim()
            .trim_matches('\0')
            .to_owned())
    })
    .await
    .unwrap_or_else(|_| Err("timed out".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_size_sums_windows_depots() {
        let vdf = include_str!("../tests/fixtures/steam/appinfo-202750.vdf");
        // All four depots report download 0, so their sizes count.
        assert_eq!(
            parse_download_size(vdf),
            Some(28_096_012 + 1_217_681_172 + 1_133_136_180 + 108_169_848)
        );
        let with_download = r#""appinfo" { "depots" { "1" { "manifests" { "public" { "download" "100" "size" "900" } } } "2" { "manifests" { "public" { "size" "50" } } } } }"#;
        assert_eq!(parse_download_size(with_download), Some(150));
        assert_eq!(parse_download_size("not vdf {"), None);
    }

    #[test]
    fn download_size_skips_dlc_and_other_os_depots() {
        let vdf = r#""appinfo" { "depots" {
            "1" { "manifests" { "public" { "size" "10" } } }
            "2" { "config" { "oslist" "windows,macos" } "manifests" { "public" { "size" "20" } } }
            "3" { "config" { "oslist" "macos" } "manifests" { "public" { "size" "400" } } }
            "4" { "config" { "oslist" "linux" } "manifests" { "public" { "size" "800" } } }
            "5" { "dlcappid" "99" "manifests" { "public" { "size" "1600" } } }
            "6" { "config" { "language" "french" } "manifests" { "public" { "size" "3200" } } }
            "7" { "config" { "language" "english" } "manifests" { "public" { "size" "5" } } }
            "branches" { "public" { "buildid" "1" } }
        } }"#;
        assert_eq!(parse_download_size(vdf), Some(35));
    }
}
