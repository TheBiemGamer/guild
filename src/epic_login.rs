//! `launcher --epic-login`: Epic's own login page in a WebView2 window.
//! Prints the authorization code from Epic's redirect page to stdout and exits;
//! prints nothing if the window is closed first. Runs as its own process because
//! tao and iced each need the process's one event loop.

use std::path::PathBuf;

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{WebContext, WebViewBuilder};

/// Where Legendary's `https://legendary.gl/epiclogin` redirects: log in, then land on
/// Epic's redirect API, which shows JSON with an `authorizationCode` for `legendary auth --code`.
const LOGIN_URL: &str = "https://www.epicgames.com/id/login?redirectUrl=https%3A//www.epicgames.com/id/api/redirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode";

/// Runs on every page; only the redirect API's JSON page posts its text back.
const CAPTURE_JS: &str = r#"
window.addEventListener('DOMContentLoaded', () => {
  if (location.hostname === 'www.epicgames.com' && location.pathname.startsWith('/id/api/redirect')) {
    window.ipc.postMessage(document.body.innerText);
  }
});
"#;

pub fn run(data_dir: PathBuf) -> ! {
    let event_loop = EventLoopBuilder::<String>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title("Log in to Epic Games")
        .with_inner_size(LogicalSize::new(520.0, 760.0))
        .build(&event_loop)
        .expect("a window can be created");

    // Our own WebView2 profile, so its cookies stay out of the install folder and the user's browser.
    let mut context = WebContext::new(Some(data_dir));
    let webview = WebViewBuilder::new_with_web_context(&mut context)
        .with_initialization_script(CAPTURE_JS)
        .with_ipc_handler(move |request| {
            // Only trust messages from Epic's own page.
            if request.uri().host() == Some("www.epicgames.com")
                && let Some(code) = authorization_code(request.body())
            {
                let _ = proxy.send_event(code);
            }
        })
        .build(&window);
    let webview = match webview {
        Ok(webview) => webview,
        Err(e) => {
            eprintln!("epic login: can't open WebView2 (is the WebView2 runtime installed?): {e}");
            std::process::exit(1);
        }
    };
    // Start signed out so the user picks the account, as Legendary's own login does.
    let _ = webview.clear_all_browsing_data();
    let _ = webview.load_url(LOGIN_URL);

    event_loop.run(move |event, _, flow| {
        let _keep_alive = (&webview, &context);
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(code) => {
                println!("{code}");
                *flow = ControlFlow::Exit;
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *flow = ControlFlow::Exit,
            _ => {}
        }
    })
}

/// `{"redirectUrl":…,"authorizationCode":"…","sid":null}`; the code is null until logged in.
fn authorization_code(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    json["authorizationCode"].as_str().filter(|c| !c.is_empty()).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_code_only_when_present() {
        let page = r#"{"warning":"Do not share this code","redirectUrl":"https://x","authorizationCode":"abc123","sid":null}"#;
        assert_eq!(authorization_code(page), Some("abc123".into()));
        assert_eq!(authorization_code(r#"{"authorizationCode":null,"sid":null}"#), None);
        assert_eq!(authorization_code(r#"{"authorizationCode":""}"#), None);
        assert_eq!(authorization_code("<html>login</html>"), None);
    }
}
