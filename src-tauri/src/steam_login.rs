//! Steam QR sign-in over Steam's public web API (`IAuthenticationService`, protobuf over HTTPS).
//! The refresh token is returned to the caller and never logged, printed or put in an error.

use std::fmt;

use crate::msg::Msg;

use protobuf::{EnumOrUnknown, Message, MessageField};
use steam_vent_proto_steam::steammessages_auth_steamclient::{
    CAuthentication_BeginAuthSessionViaQR_Request, CAuthentication_BeginAuthSessionViaQR_Response,
    CAuthentication_DeviceDetails, CAuthentication_PollAuthSessionStatus_Request,
    CAuthentication_PollAuthSessionStatus_Response, EAuthTokenPlatformType,
};

/// A QR login in progress. Steam rotates `client_id` and the challenge while it waits.
pub struct QrSession {
    client_id: u64,
    request_id: Vec<u8>,
    pub challenge_url: String,
}

pub enum PollStep {
    Waiting,
    /// The QR code changed; show `challenge_url` again.
    Rotated,
    /// Scanned in the Steam app, not yet approved.
    Scanned,
    Done(Approved),
}

pub struct Approved {
    pub account: String,
    pub refresh_token: String,
}

impl fmt::Debug for Approved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Approved").field("account", &self.account).field("refresh_token", &"<redacted>").finish()
    }
}

/// A throwaway session for tests elsewhere.
#[cfg(test)]
pub fn test_session() -> QrSession {
    QrSession { client_id: 1, request_id: vec![1], challenge_url: "https://s.team/q/1/x".into() }
}

/// Applies one poll answer to the session.
pub fn apply_poll(session: &mut QrSession, resp: &CAuthentication_PollAuthSessionStatus_Response) -> PollStep {
    if resp.has_new_client_id() {
        session.client_id = resp.new_client_id();
    }
    if !resp.refresh_token().is_empty() {
        return PollStep::Done(Approved {
            account: resp.account_name().to_string(),
            refresh_token: resp.refresh_token().to_string(),
        });
    }
    if resp.has_new_challenge_url() {
        session.challenge_url = resp.new_challenge_url().to_string();
        return PollStep::Rotated;
    }
    if resp.had_remote_interaction() { PollStep::Scanned } else { PollStep::Waiting }
}

/// Steam answers with its result code in the `x-eresult` header; only 1 is success.
pub fn eresult_ok(header: Option<&str>) -> Result<(), Msg> {
    match header {
        Some("1") => Ok(()),
        other => Err(msg!(steamRefusedLogin, code = other.unwrap_or("?"))),
    }
}

/// The QR code as an SVG: black modules on white, with a quiet zone.
pub fn qr_svg(url: &str) -> Result<String, Msg> {
    let code = qrcode::QrCode::new(url.as_bytes()).map_err(|e| msg!(cantDrawQr, detail = e.to_string()))?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .quiet_zone(true)
        .min_dimensions(240, 240)
        .dark_color(qrcode::render::svg::Color("#000"))
        .light_color(qrcode::render::svg::Color("#fff"))
        .build())
}

pub async fn begin(http: &reqwest::Client) -> Result<QrSession, Msg> {
    let req = CAuthentication_BeginAuthSessionViaQR_Request {
        website_id: Some("Client".into()),
        device_details: MessageField::some(CAuthentication_DeviceDetails {
            device_friendly_name: Some("Guild".into()),
            platform_type: Some(EnumOrUnknown::new(EAuthTokenPlatformType::k_EAuthTokenPlatformType_SteamClient)),
            ..Default::default()
        }),
        ..Default::default()
    };
    let resp: CAuthentication_BeginAuthSessionViaQR_Response = call(http, "BeginAuthSessionViaQR", &req).await?;
    Ok(QrSession {
        client_id: resp.client_id(),
        request_id: resp.request_id().to_vec(),
        challenge_url: resp.challenge_url().to_string(),
    })
}

pub async fn poll(http: &reqwest::Client, session: &mut QrSession) -> Result<PollStep, Msg> {
    let req = CAuthentication_PollAuthSessionStatus_Request {
        client_id: Some(session.client_id),
        request_id: Some(session.request_id.clone()),
        ..Default::default()
    };
    let resp: CAuthentication_PollAuthSessionStatus_Response = call(http, "PollAuthSessionStatus", &req).await?;
    Ok(apply_poll(session, &resp))
}

/// One `IAuthenticationService` call: protobuf in (base64 form field), protobuf out.
async fn call<Req: Message, Resp: Message>(http: &reqwest::Client, method: &str, req: &Req) -> Result<Resp, Msg> {
    let body = base64(&req.write_to_bytes().map_err(Msg::system)?);
    let resp = http
        .post(format!("https://api.steampowered.com/IAuthenticationService/{method}/v1/"))
        .form(&[("input_protobuf_encoded", body)])
        .send()
        .await
        .map_err(|e| msg!(cantReachSteamDetail, detail = e.to_string()))?;
    eresult_ok(resp.headers().get("x-eresult").and_then(|v| v.to_str().ok()))?;
    let bytes = resp.bytes().await.map_err(|e| msg!(cantReadSteamAnswer, detail = e.to_string()))?;
    Resp::parse_from_bytes(&bytes).map_err(|_| msg!(steamAnswerUnreadable))
}

/// RFC 4648 base64 with padding.
fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use steam_vent_proto_steam::steammessages_auth_steamclient::CAuthentication_PollAuthSessionStatus_Response as Resp;

    fn session() -> QrSession {
        QrSession { client_id: 1, request_id: vec![1, 2], challenge_url: "https://s.team/q/1/old".into() }
    }

    #[test]
    fn poll_rotates_challenge_and_switches_client_id() {
        let mut s = session();
        let mut r = Resp::new();
        r.set_new_client_id(7);
        r.set_new_challenge_url("https://s.team/q/1/new".into());
        assert!(matches!(apply_poll(&mut s, &r), PollStep::Rotated));
        assert_eq!(s.client_id, 7);
        assert_eq!(s.challenge_url, "https://s.team/q/1/new");
    }

    #[test]
    fn poll_with_token_and_new_challenge_is_done() {
        let mut s = session();
        let resp = Resp {
            new_client_id: Some(9),
            new_challenge_url: Some("https://s.team/q/1/new".into()),
            refresh_token: Some("tok".into()),
            account_name: Some("acct".into()),
            ..Default::default()
        };
        assert!(matches!(apply_poll(&mut s, &resp), PollStep::Done(a) if a.account == "acct"));
        assert_eq!(s.client_id, 9);
    }

    #[test]
    fn poll_done_carries_account_and_token() {
        let mut r = Resp::new();
        r.set_refresh_token("r".into());
        r.set_account_name("acc".into());
        match apply_poll(&mut session(), &r) {
            PollStep::Done(a) => assert_eq!((a.account.as_str(), a.refresh_token.as_str()), ("acc", "r")),
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn poll_scanned_then_waiting() {
        let mut r = Resp::new();
        r.set_had_remote_interaction(true);
        assert!(matches!(apply_poll(&mut session(), &r), PollStep::Scanned));
        assert!(matches!(apply_poll(&mut session(), &Resp::new()), PollStep::Waiting));
    }

    #[test]
    fn approved_debug_hides_the_token() {
        let a = Approved { account: "acc".into(), refresh_token: "secret-token".into() };
        let text = format!("{a:?}");
        assert!(!text.contains("secret-token"));
        assert!(text.contains("<redacted>"));
    }

    #[test]
    fn eresult_ok_only_for_1() {
        assert!(eresult_ok(Some("1")).is_ok());
        assert_eq!(eresult_ok(Some("84")), Err(msg!(steamRefusedLogin, code = "84")));
        assert!(eresult_ok(None).is_err());
    }

    #[test]
    fn qr_svg_is_an_svg() {
        let s = qr_svg("https://s.team/q/1/abc").unwrap();
        assert!(s.starts_with("<?xml") || s.starts_with("<svg"));
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }
}
