//! Amazon's sign-in page in a second, private Tauri window, for the device login Nile starts. After login Amazon sends
//! the browser to `www.amazon.com/?openid.assoc_handle=amzn_sonic_games_launcher&…&openid.oa2.authorization_code=…`;
//! the navigation hook takes the code and cancels that load.

use std::sync::mpsc;

use tauri::{AppHandle, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// Nile's sign-in page for this device; anything else is refused.
pub fn sign_in_page(url: &str) -> Option<Url> {
    let url = Url::parse(url).ok()?;
    let amazon = matches!(url.host_str(), Some("amazon.com" | "www.amazon.com"));
    (url.scheme() == "https" && amazon && url.path() == "/ap/signin").then_some(url)
}

/// Opens the login window on Nile's `url` and waits (call from a blocking thread) until it yields a code or the user
/// closes it (`Ok(None)`).
pub fn run(app: &AppHandle, url: &str) -> Result<Option<String>, crate::msg::Msg> {
    let url = sign_in_page(url).ok_or(msg!(nileOddLoginPage))?;
    let (tx, rx) = mpsc::channel();
    let closed = tx.clone();
    let window = WebviewWindowBuilder::new(app, "amazon-login", WebviewUrl::External(url))
        .title(crate::msg::native("logInAmazonTitle"))
        .inner_size(520.0, 760.0)
        // Fresh cookies every time, so the user picks the account.
        .incognito(true)
        .on_navigation(move |url| match login_code(url) {
            Some(code) => {
                let _ = tx.send(Some(code));
                false
            }
            None => true,
        })
        .build()
        .map_err(|e| msg!(cantOpenLoginWindow, detail = e.to_string()))?;
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            let _ = closed.send(None);
        }
    });
    let code = rx.recv().ok().flatten();
    let _ = window.close();
    Ok(code)
}

fn login_code(url: &Url) -> Option<String> {
    let amazon = matches!(url.host_str(), Some("www.amazon.com" | "amazon.com"));
    if url.scheme() != "https" || !amazon || url.path() != "/" {
        return None;
    }
    let value = |key: &str| url.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned());
    if value("openid.assoc_handle").as_deref() != Some("amzn_sonic_games_launcher") {
        return None;
    }
    value("openid.oa2.authorization_code").filter(|c| !c.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DONE: &str = "https://www.amazon.com/?openid.assoc_handle=amzn_sonic_games_launcher&openid.mode=id_res&openid.oa2.authorization_code=ANabc%2B123&openid.return_to=https%3A%2F%2Fwww.amazon.com";

    #[test]
    fn takes_code_only_from_the_return_page() {
        let url = |s: &str| Url::parse(s).unwrap();
        assert_eq!(login_code(&url(DONE)), Some("ANabc+123".into()));
        assert_eq!(login_code(&url(&DONE.replace("www.amazon.com/", "amazon.com/"))), Some("ANabc+123".into()));
        assert_eq!(login_code(&url(&DONE.replace("www.amazon.com/", "www.amazon.com.evil.com/"))), None);
        assert_eq!(login_code(&url(&DONE.replace("www.amazon.com/?", "www.amazon.com/ap/signin?"))), None);
        assert_eq!(login_code(&url(&DONE.replace("https://", "http://"))), None);
        assert_eq!(login_code(&url(&DONE.replace("assoc_handle=amzn_sonic_games_launcher", "assoc_handle=usflex"))), None);
        assert_eq!(login_code(&url(&DONE.replace("openid.assoc_handle=amzn_sonic_games_launcher&", ""))), None);
        assert_eq!(login_code(&url(&DONE.replace("authorization_code=ANabc%2B123", "authorization_code="))), None);
        assert_eq!(login_code(&url("https://www.amazon.com/?openid.assoc_handle=amzn_sonic_games_launcher")), None);
    }

    #[test]
    fn opens_only_nile_sign_in_page() {
        let stdout = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/amazon/sample-auth-login-stdout.txt")).unwrap();
        let answer: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert!(sign_in_page(answer["url"].as_str().unwrap()).is_some());
        assert!(sign_in_page("https://www.amazon.com/ap/signin?x=1").is_some());
        assert!(sign_in_page("http://amazon.com/ap/signin").is_none());
        assert!(sign_in_page("https://amazon.com.evil.com/ap/signin").is_none());
        assert!(sign_in_page("https://amazon.com/ap/other").is_none());
        assert!(sign_in_page("https://evil.com/ap/signin").is_none());
        assert!(sign_in_page("not a url").is_none());
    }
}
