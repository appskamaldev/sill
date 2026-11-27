//! The catalog the device's Apps drawer shows: the apps Windows lists in Start (`shell:AppsFolder`),
//! each by its AppUserModelID or parsing name (an opaque `bundleID` to the client) and display
//! name, with a 64×64 PNG icon, and launching one of them (kind 7). Windows only; the list rules
//! are portable and tested.
//!
//! Upstream sends the icons (kind 4), then the list (kind 5), after the catalog of a new session,
//! and none while the Mac is locked (LockPolicy). A launch request is honoured only for an ID this
//! host listed: never an arbitrary string.

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub mod window_ops;
#[cfg(windows)]
pub mod windows_list;
#[cfg(windows)]
pub use win::{app_path, icon_png, launch, scan};

/// One app of the drawer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    /// What `shell:AppsFolder\<id>` opens: a packaged app's AppUserModelID
    /// ("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App"), a desktop app's ID or path.
    pub id: String,
    pub name: String,
}

/// At most this many apps are listed (the device's drawer is a scroll list, and every icon costs a
/// message at connect).
pub const MAX_APPS: usize = 400;
/// The longest ID or name taken (longer ones are dropped, never cut: an ID must stay whole).
pub const MAX_ID: usize = 512;
pub const MAX_NAME: usize = 128;

/// The drawer's list from what the shell enumerated: entries that are not launchable apps dropped
/// (empty, too long, control characters, URLs, documents and uninstallers), duplicate IDs once,
/// sorted by name ignoring case, at most `MAX_APPS`.
pub fn tidy(mut apps: Vec<App>) -> Vec<App> {
    apps.retain(|a| {
        let id = a.id.trim();
        let name = a.name.trim();
        !id.is_empty()
            && !name.is_empty()
            && id.len() <= MAX_ID
            && name.chars().count() <= MAX_NAME
            && !id.chars().chain(name.chars()).any(char::is_control)
            && !id.contains("://")
            && !is_document(id)
            && !name.to_lowercase().starts_with("uninstall")
    });
    for a in &mut apps {
        a.name = a.name.trim().to_string();
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    let mut seen = std::collections::HashSet::new();
    apps.retain(|a| seen.insert(a.id.clone()));
    apps.truncate(MAX_APPS);
    apps
}

/// A Start-menu entry that opens a document or web page rather than an app.
fn is_document(id: &str) -> bool {
    let l = id.to_lowercase();
    [".url", ".txt", ".pdf", ".chm", ".htm", ".html", ".rtf", ".lnk"].iter().any(|e| l.ends_with(e))
}

/// Whether `id` is one of `listed` (exactly): the only IDs a device may launch.
pub fn may_launch(listed: &[App], id: &str) -> bool {
    listed.iter().any(|a| a.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str) -> App {
        App { id: id.into(), name: name.into() }
    }

    #[test]
    fn tidy_keeps_apps_sorted_once() {
        let got = tidy(vec![
            app("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "Notepad"),
            app("{6D809377-6AF0-444B-8957-A3773F02200E}\\Mozilla Firefox\\firefox.exe", "Firefox"),
            app("MSEdge", " Microsoft Edge "),
            app("MSEdge", "Microsoft Edge"),
            app("https://example.com", "A web link"),
            app("{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}\\App\\readme.txt", "Read me"),
            app("{6D809377}\\App\\unins000.exe", "Uninstall App"),
            app("", "Nameless"),
            app("NoName", ""),
            app("Bad\u{7}Id", "Bell"),
        ]);
        let names: Vec<&str> = got.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["Firefox", "Microsoft Edge", "Notepad"]);
    }

    #[test]
    fn at_most_max_apps() {
        let many: Vec<App> = (0..1000).map(|i| app(&format!("App{i:04}"), &format!("App {i:04}"))).collect();
        assert_eq!(tidy(many).len(), MAX_APPS);
    }

    #[test]
    fn only_listed_ids_launch() {
        let listed = tidy(vec![app("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App", "Calculator")]);
        assert!(may_launch(&listed, "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"));
        assert!(!may_launch(&listed, "cmd.exe"));
        assert!(!may_launch(&listed, "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App "));
        assert!(!may_launch(&listed, ""));
    }
}
