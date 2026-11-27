//! The window list as sessions see it (kind 2's `windows`), thumbnails (kind 3) and window
//! commands (kind 15). The host binary gives `Host::windows` a source (Windows: the window
//! catalog); without one the list is empty and only the Desktop streams.

use sill_wire::{WindowCommand, WindowInfo};
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Clone, Debug)]
pub enum WindowEvent {
    /// The list changed (any field of any window, or the set, or the order).
    List(Arc<Vec<WindowInfo>>),
    /// A window's thumbnail: id, JPEG.
    Thumbnail(u32, Arc<[u8]>),
}

pub trait WindowSource: Send + Sync {
    fn list(&self) -> Arc<Vec<WindowInfo>>;
    fn subscribe(&self) -> broadcast::Receiver<WindowEvent>;
    /// Close, minimize or full screen; the outcome in words.
    fn command(&self, c: &WindowCommand) -> Result<(), String>;
    /// A device joined: send every thumbnail again soon (upstream `resendAllThumbnails`).
    fn resend_thumbnails(&self);
}
