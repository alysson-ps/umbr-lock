use std::sync::mpsc::Sender;

use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::state::PasswordBuffer;
use crate::types::UiMessage;

pub fn listen_for_keyboard_events(
    event: EventKeys,
    password: &mut PasswordBuffer,
    sender: &Sender<UiMessage>,
) {
    match event {
        EventKeys::Pressed { event } => match event.keysym {
            Keysym::Return => {
                let passwd = String::from_utf8_lossy(&password.bytes).to_string();

                sender
                    .send(UiMessage::UnlockWithPassword { password: passwd })
                    .unwrap();
            }
            Keysym::BackSpace => {
                // Handle backspace
            }

            // Ignored keys
            Keysym::Escape => {}
            Keysym::Tab => {}
            Keysym::Delete => {}
            Keysym::Shift_L | Keysym::Shift_R => {}
            Keysym::Control_L | Keysym::Control_R => {}
            Keysym::Alt_L | Keysym::Alt_R => {}
            Keysym::Caps_Lock => {}
            Keysym::Up => {}
            Keysym::Down => {}
            Keysym::Left => {}
            Keysym::Right => {}
            Keysym::Insert => {}
            Keysym::Home => {}
            Keysym::End => {}

            // Handle printable characters
            _ => {
                if let Some(utf8) = event.utf8 {
                    password.insert_char(utf8.chars().next().unwrap());
                }
            }
        },
        EventKeys::Released { .. } => {
            // Handle key release if needed
        }
    }
}
