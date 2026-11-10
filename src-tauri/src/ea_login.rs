//! EA's sign-in page in a second, private Tauri window, for the login `maxima-cli guild login` starts. After login EA
//! sends the browser to `qrc:///html/login_successful.html?code=…`; the navigation hook takes the code and cancels that
//! load, and `library::ea::log_in` hands it to Maxima's listener on 127.0.0.1:31033.

use tauri::Url;

/// Maxima's sign-in page; anything else is refused.
pub fn sign_in_page(url: &str) -> Option<Url> {
    let url = Url::parse(url).ok()?;
    (url.scheme() == "https"
        && url.host_str() == Some("accounts.ea.com")
        && url.path() == "/connect/auth")
        .then_some(url)
}

/// The code on EA's return page, which only Maxima's (absent) Qt shell would load.
pub fn login_code(url: &Url) -> Option<String> {
    // shortcut: spike logging (no query, so no code), removed once the redirect is known to arrive here.
    tracing::info!(
        "ea login: navigation to {}://{}{}",
        url.scheme(),
        url.host_str().unwrap_or(""),
        url.path()
    );
    if url.scheme() != "qrc" || url.path() != "/html/login_successful.html" {
        return None;
    }
    let code = url
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .filter(|c| !c.is_empty());
    tracing::info!("ea login: return page seen, code {}", code.is_some());
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_code_only_from_the_return_page() {
        let url = |s: &str| Url::parse(s).unwrap();
        let done = "qrc:///html/login_successful.html?code=QUOAA%2Babc&state=x";
        assert_eq!(login_code(&url(done)), Some("QUOAA+abc".into()));
        assert_eq!(login_code(&url(&done.replace("qrc:", "https:"))), None);
        assert_eq!(
            login_code(&url(&done.replace("login_successful", "login_failed"))),
            None
        );
        assert_eq!(
            login_code(&url("qrc:///html/login_successful.html?state=x")),
            None
        );
        assert_eq!(
            login_code(&url("qrc:///html/login_successful.html?code=")),
            None
        );
    }

    #[test]
    fn opens_only_ea_sign_in_page() {
        assert!(
            sign_in_page("https://accounts.ea.com/connect/auth?client_id=JUNO_PC_CLIENT&x=1")
                .is_some()
        );
        assert!(sign_in_page("http://accounts.ea.com/connect/auth").is_none());
        assert!(sign_in_page("https://accounts.ea.com.evil.com/connect/auth").is_none());
        assert!(sign_in_page("https://accounts.ea.com/connect/other").is_none());
        assert!(sign_in_page("https://evil.com/connect/auth").is_none());
        assert!(sign_in_page("not a url").is_none());
    }
}
