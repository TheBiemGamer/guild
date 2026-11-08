//! GOG's own login page in a second, private Tauri window. After login GOG redirects to
//! `embed.gog.com/on_login_success?code=…`; the navigation hook takes the code and cancels that load.

use tauri::{AppHandle, Url};

use crate::library::gog::LOGIN_URL;

/// Opens the login window and waits (call from a blocking thread) until it yields a code
/// or the user closes it (`Ok(None)`).
pub fn run(app: &AppHandle) -> Result<Option<String>, crate::msg::Msg> {
    let url = Url::parse(LOGIN_URL).expect("valid constant URL");
    crate::login_window::run(app, "gog-login", "logInGogTitle", url, None, login_code)
}

fn login_code(url: &Url) -> Option<String> {
    if url.host_str() != Some("embed.gog.com") || url.path() != "/on_login_success" {
        return None;
    }
    url.query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .filter(|c| !c.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_code_only_from_the_success_page() {
        let url = |s: &str| Url::parse(s).unwrap();
        assert_eq!(
            login_code(&url(
                "https://embed.gog.com/on_login_success?origin=client&code=abc%2B123"
            )),
            Some("abc+123".into())
        );
        assert_eq!(
            login_code(&url(
                "https://embed.gog.com/on_login_success?origin=client&code="
            )),
            None
        );
        assert_eq!(
            login_code(&url("https://embed.gog.com/on_login_success?origin=client")),
            None
        );
        assert_eq!(
            login_code(&url(
                "https://embed.gog.com.evil.com/on_login_success?code=abc"
            )),
            None
        );
        assert_eq!(
            login_code(&url("https://embed.gog.com/elsewhere?code=abc")),
            None
        );
        assert_eq!(
            login_code(&url("https://embed.gog.com/on_login_success/x?code=abc")),
            None
        );
        assert_eq!(login_code(&url("https://auth.gog.com/auth?code=abc")), None);
    }
}
