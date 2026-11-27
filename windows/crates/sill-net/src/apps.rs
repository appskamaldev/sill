//! The Apps drawer as sessions see it: the host binary gives `Host::apps` a source (Windows: Start's
//! apps), and each session sends its icons (kind 4) and then the list (kind 5) after its catalog,
//! then every change, and passes a launch request (kind 7) back. Nothing goes while the computer
//! is locked (upstream LockPolicy: icons and the app list say what is installed).

use sill_wire::AppInfo;
use std::sync::Arc;
use tokio::sync::broadcast;

/// An app's icon: bundle ID, 64×64 PNG.
pub type Icon = (String, Arc<[u8]>);

#[derive(Clone, Debug)]
pub enum AppEvent {
    /// The list changed (or came for the first time).
    List(Arc<Vec<AppInfo>>),
    /// An icon is ready: bundle ID, 64×64 PNG.
    Icon(String, Arc<[u8]>),
}

pub trait AppSource: Send + Sync {
    /// The list and the icons ready so far.
    fn snapshot(&self) -> (Arc<Vec<AppInfo>>, Vec<Icon>);
    fn subscribe(&self) -> broadcast::Receiver<AppEvent>;
    /// Launches `bundle_id` if it is on the list; the error in words otherwise.
    fn launch(&self, bundle_id: &str) -> Result<(), String>;
    /// The computer is locked (or another desktop has the input): send nothing.
    fn locked(&self) -> bool;
}
