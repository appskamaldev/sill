//! When each window's thumbnail (kind 3) is due, as upstream's `WindowCatalog`: right after each
//! 2 s window read, again for the streamed window and any window whose list entry changed, else
//! every 6 s. The stamp is set before the capture, so a failed one also waits 6 s. A new device
//! (or an unlock) clears the stamps: every thumbnail goes at once.

use sill_wire::WindowInfo;
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const STATIC_INTERVAL: Duration = Duration::from_secs(6);
pub const MAX_W: u32 = 208;
pub const MAX_H: u32 = 132;
pub const QUALITY: f32 = 0.6;

#[derive(Default)]
pub struct Thumbs {
    last: HashMap<u32, (Instant, WindowInfo)>,
}

impl Thumbs {
    /// The windows whose thumbnail is due now (stamped as taken).
    pub fn due(&mut self, list: &[WindowInfo], active: Option<u32>, now: Instant) -> Vec<WindowInfo> {
        self.last.retain(|id, _| list.iter().any(|w| w.id == *id));
        let mut out = Vec::new();
        for w in list {
            let due = match self.last.get(&w.id) {
                None => true,
                Some((t, info)) => {
                    Some(w.id) == active || info != w || now.saturating_duration_since(*t) >= STATIC_INTERVAL
                }
            };
            if due {
                self.last.insert(w.id, (now, w.clone()));
                out.push(w.clone());
            }
        }
        out
    }

    pub fn reset(&mut self) {
        self.last.clear();
    }
}

/// The window's thumbnail as JPEG, or `None` if it could not be taken.
#[cfg(windows)]
pub fn thumbnail(h: isize) -> Option<Vec<u8>> {
    sill_catalog::window_ops::thumbnail(h, MAX_W, MAX_H, QUALITY).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(id: u32, title: &str) -> WindowInfo {
        WindowInfo { id, title: title.into(), app_name: "A".into(), bundle_id: "a".into(), width: 800, height: 600 }
    }

    #[test]
    fn active_every_pass_changed_at_once_others_every_6_s() {
        let mut t = Thumbs::default();
        let t0 = Instant::now();
        let list = vec![w(1, "one"), w(2, "two"), w(3, "three")];
        assert_eq!(t.due(&list, Some(2), t0).len(), 3, "all at first");
        let ids = |v: Vec<WindowInfo>| v.into_iter().map(|w| w.id).collect::<Vec<_>>();
        assert_eq!(ids(t.due(&list, Some(2), t0 + Duration::from_secs(2))), vec![2], "the streamed one again");
        let changed = vec![w(1, "one, edited"), w(2, "two"), w(3, "three")];
        assert_eq!(ids(t.due(&changed, Some(2), t0 + Duration::from_secs(4))), vec![1, 2]);
        assert_eq!(ids(t.due(&changed, None, t0 + Duration::from_secs(6))), vec![3], "6 s since its last");
        t.reset();
        assert_eq!(t.due(&changed, None, t0 + Duration::from_secs(7)).len(), 3, "a new device gets all");
    }
}
