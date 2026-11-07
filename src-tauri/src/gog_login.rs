//! GOG's own login page in a second, private Tauri window. After login GOG redirects to
//! `embed.gog.com/on_login_success?code=…`; the navigation hook takes the code and cancels that load.

use std::sync::mpsc;

use tauri::{AppHandle, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::library::gog::LOGIN_URL;

/// Opens the login window and waits (call from a blocking thread) until it yields a code
/// or the user closes it (`Ok(None)`).
pub fn run(app: &AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = mpsc::channel();
    let closed = tx.clone();
    let url = Url::parse(LOGIN_URL).expect("valid constant URL");
    let window = WebviewWindowBuilder::new(app, "gog-login", WebviewUrl::External(url))
        .title("Log in to GOG")
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
    if url.host_str() != Some("embed.gog.com") || url.path() != "/on_login_success" {
        return None;
    }
    url.query_pairs().find(|(k, _)| k == "code").map(|(_, v)| v.into_owned()).filter(|c| !c.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_code_only_from_the_success_page() {
        let url = |s: &str| Url::parse(s).unwrap();
        assert_eq!(
            login_code(&url("https://embed.gog.com/on_login_success?origin=client&code=abc%2B123")),
            Some("abc+123".into())
        );
        assert_eq!(login_code(&url("https://embed.gog.com/on_login_success?origin=client&code=")), None);
        assert_eq!(login_code(&url("https://embed.gog.com/on_login_success?origin=client")), None);
        assert_eq!(login_code(&url("https://embed.gog.com.evil.com/on_login_success?code=abc")), None);
        assert_eq!(login_code(&url("https://embed.gog.com/elsewhere?code=abc")), None);
        assert_eq!(login_code(&url("https://embed.gog.com/on_login_success/x?code=abc")), None);
        assert_eq!(login_code(&url("https://auth.gog.com/auth?code=abc")), None);
    }
}
