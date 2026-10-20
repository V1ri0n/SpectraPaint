//! Work snapshots: every few minutes a document with unsaved changes is copied whole to disk,
//! the newest few kept per document (Preferences › File Handling › Snapshots), and File › Work
//! Snapshots opens an earlier state as a new document. It is the way back from a mistake noticed
//! after Undo's reach, a save over the good file or a crash.
//!
//! The files and the timer are desktop only; on the web this module does nothing.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
mod web {
    use crate::PhotocraftApp;
    use photocraft_engine::jobs::JobEvent;

    /// No snapshots on the web.
    #[derive(Default)]
    pub struct State;

    pub fn tick(_app: &mut PhotocraftApp) {}
    pub fn show(_app: &mut PhotocraftApp, _ctx: &egui::Context) {}
    pub fn toggle(_app: &mut PhotocraftApp) {}
    pub fn on_event(_app: &mut PhotocraftApp, _e: &JobEvent) -> bool {
        false
    }
}
#[cfg(target_arch = "wasm32")]
pub use web::*;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
