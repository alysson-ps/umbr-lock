use std::sync::mpsc::Sender;

use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::UiRuntime;
use crate::types::{EventKeys, UiMessage};

pub fn listen_for_keyboard_events(event: EventKeys, runtime: &mut UiRuntime) {
    match event {
        EventKeys::Pressed { event } => {
            let new_len = match event.keysym {
                Keysym::Return => {
                    let passwd = runtime
                        .buffer
                        .as_ref()
                        .map(|buffer| buffer.as_string())
                        .unwrap_or_default();

                    runtime
                        .sender
                        .as_ref()
                        .unwrap()
                        .send(UiMessage::UnlockWithPassword { password: passwd })
                        .unwrap();

                    runtime
                        .buffer
                        .as_ref()
                        .map(|buffer| buffer.len())
                        .unwrap_or_default()
                }
                Keysym::BackSpace => runtime
                    .buffer
                    .as_mut()
                    .map(|buffer| {
                        buffer.pop_char();
                        buffer.len()
                    })
                    .unwrap_or_default(),

                // Ignored keys
                Keysym::Escape
                | Keysym::Tab
                | Keysym::Delete
                | Keysym::Shift_L
                | Keysym::Shift_R
                | Keysym::Control_L
                | Keysym::Control_R
                | Keysym::Alt_L
                | Keysym::Alt_R
                | Keysym::Caps_Lock
                | Keysym::Up
                | Keysym::Down
                | Keysym::Left
                | Keysym::Right
                | Keysym::Insert
                | Keysym::Home
                | Keysym::End => runtime
                    .buffer
                    .as_ref()
                    .map(|buffer| buffer.len())
                    .unwrap_or_default(),

                // Handle printable characters
                _ => {
                    if let Some(utf8) = event.utf8 {
                        runtime
                            .buffer
                            .as_mut()
                            .map(|buffer| {
                                buffer.insert_char(utf8.chars().next().unwrap());
                                buffer.len()
                            })
                            .unwrap_or_default()
                    } else {
                        runtime
                            .buffer
                            .as_ref()
                            .map(|buffer| buffer.len())
                            .unwrap_or_default()
                    }
                }
            };

            runtime.update_count(new_len);
        }
        EventKeys::Released { .. } => {
            // Handle key release if needed
        }
    }
}
