use smithay_client_toolkit::seat::keyboard::KeyEvent;

#[derive(Debug)]
pub enum WindowingMessage {
    GtkEvent(EventKeys),
    Ready { width: u32, height: u32 },
    UnlockFailed,
    Quit,
}
#[derive(Debug)]
pub enum EventKeys {
    Pressed { event: KeyEvent },
    Released { event: KeyEvent },
}

#[derive(Debug)]
pub enum UiMessage {
    UnlockWithPassword {
        password: String,
    },
    Render {
        width: i32,
        height: i32,
        stride: i32,
        n_channels: i32,
        pixels: Vec<u8>,
    },
}

pub enum MessageLoopState {
    Continue,
    Stop,
}