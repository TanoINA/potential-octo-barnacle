//! Renderer-independent native reactive state for Aether.
mod controller;
mod events;
pub mod history;
mod message_edit;
mod model;
mod reducer;
pub use controller::Controller;
pub use events::{BackendEvent, EventQueue, Waker, EVENT_CAPACITY};
pub use model::{CommandResult, CompletedRun, Model, RunPhase, RunStatus};

pub mod commands;
pub use commands::{InputAction, SendOptions, UiAction};
