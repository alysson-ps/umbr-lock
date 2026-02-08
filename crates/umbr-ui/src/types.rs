use slint::platform::WindowEvent;
use wayland_client::backend::ObjectId;

#[derive(Debug)]
pub enum WindowingMessage {
    Event(WindowEvent),
    Ready {
        display_id: ObjectId,
        surface_id: ObjectId,
        width: u32,
        height: u32,
    },
    UnlockFailed(String),
    Quit,
}

#[derive(Debug)]
pub enum UiMessage {
    UnlockWithPassword {
        password: String,
    },
}

pub enum MessageLoopState {
    Continue,
    Stop,
}
