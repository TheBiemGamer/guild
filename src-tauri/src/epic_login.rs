//! Epic's own login page in a second, private Tauri window. After login Epic shows its
//! redirect page with an `authorizationCode`; a small script forwards that code to a dead
//! URL (`launcher.invalid`), where the navigation hook catches it and cancels the load.

use std::sync::mpsc;

use tauri::{AppHandle, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// Where Legendary's `https://legendary.gl/epiclogin` redirects: log in, then land on
/// Epic's redirect API, which shows JSON with an `authorizationCode` for `legendary auth --code`.
const LOGIN_URL: &str = "https://www.epicgames.com/id/login?redirectUrl=https%3A//www.epicgames.com/id/api/redirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode";

/// `.invalid` never resolves, so a missed hook can't send the code anywhere.
const CODE_HOST: &str = "launcher.invalid";

/// Runs on every page; only Epic's redirect API page has the code.
const CAPTURE_JS: &str = r#"
window.addEventListener('DOMContentLoaded', () => {
  if (location.hostname !== 'www.epicgames.com' || !location.pathname.startsWith('/id/api/redirect')) return;
  try {
    const code = JSON.parse(document.body.innerText).authorizationCode;
    if (code) location.href = 'https://launcher.invalid/?code=' + encodeURIComponent(code);
  } catch (_) {}
});
"#;

/// Opens the login window and waits (call from a blocking thread) until it yields a code
/// or the user closes it (`Ok(None)`).
pub fn run(app: &AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = mpsc::channel();
    let closed = tx.clone();
    let url = Url::parse(LOGIN_URL).expect("valid constant URL");
    let window = WebviewWindowBuilder::new(app, "epic-login", WebviewUrl::External(url))
        .title("Log in to Epic Games")
        .inner_size(520.0, 760.0)
        // Fresh cookies every time, so the user picks the account (as Legendary's login does).
        .incognito(true)
        .initialization_script(CAPTURE_JS)
        .on_navigation(move |url| match login_code(url) {
            Some(code) => {
                let _ = tx.send(Some(code));
                false
            }
            None => true,
        })
        .build()
        .map_err(|e| format!("can't open the login window: {e}"))?;
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
    if url.host_str() != Some(CODE_HOST) {
        return None;
    }
    url.query_pairs().find(|(k, _)| k == "code").map(|(_, v)| v.into_owned()).filter(|c| !c.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_code_only_from_the_dead_host() {
        let url = |s: &str| Url::parse(s).unwrap();
        assert_eq!(login_code(&url("https://launcher.invalid/?code=abc%2B123")), Some("abc+123".into()));
        assert_eq!(login_code(&url("https://launcher.invalid/?code=")), None);
        assert_eq!(login_code(&url("https://www.epicgames.com/id/login?code=abc")), None);
        assert_eq!(login_code(&url("https://launcher.invalid.evil.com/?code=abc")), None);
    }
}
