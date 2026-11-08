//! A store's own login page in a second, private Tauri window. The navigation hook watches for the page that carries
//! the login code, takes the code and cancels that load.

use std::sync::mpsc;

use tauri::{AppHandle, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// Opens the login window on `url` and waits (call from a blocking thread) until `code` finds a code in a navigation
/// or the user closes the window (`Ok(None)`). `title` is a `msg::native` code; `init_script` runs on every page.
pub fn run(
    app: &AppHandle,
    label: &str,
    title: &str,
    url: Url,
    init_script: Option<&str>,
    code: fn(&Url) -> Option<String>,
) -> Result<Option<String>, crate::msg::Msg> {
    let (tx, rx) = mpsc::channel();
    let closed = tx.clone();
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::External(url))
        .title(crate::msg::native(title))
        .inner_size(520.0, 760.0)
        // Fresh cookies every time, so the user picks the account (as Legendary's login does).
        .incognito(true)
        .on_navigation(move |url| match code(url) {
            Some(code) => {
                let _ = tx.send(Some(code));
                false
            }
            None => true,
        });
    if let Some(script) = init_script {
        builder = builder.initialization_script(script);
    }
    let window = builder
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
