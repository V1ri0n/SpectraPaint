//! Opening web addresses from the UI. Spectra Paint has no community, website or issue-tracker
//! links of its own yet, so the Help menu, the About dialog and the start screen show none
//! ([`COMMANDS`] is empty; the upstream project's Discord, website and GitHub links were removed
//! with the rebranding). [`open`] stays for what does link out, such as a contributor's profile in
//! the credits (`credits.rs`). To bring a link back: add its `(command id, url)` to [`COMMANDS`]
//! and a `("<id>", "<label>", &["Help"], None)` row to `menus::UI_COMMANDS`.

use serde_json::{Value, json};

use crate::PhotocraftApp;

/// Help-menu link commands: (id, url). Labels live in `menus::UI_COMMANDS`. None at present.
pub const COMMANDS: &[(&str, &str)] = &[];

pub fn url_for(id: &str) -> Option<&'static str> {
    COMMANDS.iter().find(|c| c.0 == id).map(|c| c.1)
}

/// Open `url` in the system browser (a new tab on the web) and note it in the status bar. Prefers
/// the platform `open_url` service (reliable on Windows/macOS/Linux); falls back to `ctx.open_url`.
pub fn open(app: &mut PhotocraftApp, ctx: &egui::Context, url: &str) -> Value {
    let opened = app.services.open_url.as_ref().map(|f| f(url).is_ok()).unwrap_or(false);
    if !opened {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
    let template = "Opened {url}";
    app.ui.status = crate::i18n::fmt(tl!(template), &[("url", url)]);
    json!({"url": url})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_goes_through_the_platform_service() {
        // Issue #14: links must open in the browser. They route through the `open_url` service
        // rather than the unreliable `ctx.open_url`.
        use std::sync::{Arc, Mutex};
        let opened: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let rec = opened.clone();
        let services = crate::Services {
            open_url: Some(Box::new(move |u: &str| {
                rec.lock().unwrap().push(u.to_string());
                Ok(())
            })),
            ..Default::default()
        };
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), services);
        let ctx = egui::Context::default();
        open(&mut app, &ctx, "https://example.com/profile");
        assert_eq!(opened.lock().unwrap().last().map(String::as_str), Some("https://example.com/profile"));
        assert_eq!(app.ui.status, "Opened https://example.com/profile");
    }

    #[test]
    fn help_menu_has_no_outside_links() {
        let app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        let help: Vec<String> = crate::menus::menu_items(&app).into_iter().filter(|i| i.path == ["Help"]).map(|i| i.id).collect();
        assert_eq!(help, ["help.systemInfo", "help.about"]);
        assert!(COMMANDS.is_empty() && url_for("help.discord").is_none());
    }
}
