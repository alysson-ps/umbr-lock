#![allow(warnings)]

// extern crate gdk;
// extern crate gtk;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use gtk::prelude::*;
use gtk::{Box as GtkBox, Button, Label, OffscreenWindow, Orientation};
use smithay_client_toolkit::seat::keyboard::Keysym;
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::shm::slot::SlotPool;
use umbr_core::{Anyresult, UmbrError};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_surface::WlSurface;

use self::types::{EventKeys, UiMessage, WindowingMessage};

pub mod types;
pub mod win;

pub struct UiRuntime {
    sender: Sender<UiMessage>,
    receiver: Receiver<WindowingMessage>,
    running: bool,
}

impl UiRuntime {
    pub fn new(sender: Sender<UiMessage>, receiver: Receiver<WindowingMessage>) -> Anyresult<Self> {
        let (w, h) = wait_configure_and_render(&receiver)?;

        let pixbuf = convert_to_pixels(w, h);

        sender
            .send(UiMessage::Render {
                width: pixbuf.width,
                height: pixbuf.height,
                stride: pixbuf.stride,
                n_channels: pixbuf.n_channels,
                pixels: pixbuf.pixels,
            })
            .unwrap();

        Ok(Self {
            sender,
            receiver,
            running: true,
        })
    }

    pub fn process_messages(&mut self) -> Anyresult<()> {
        if !self.running {
            return Ok(());
        }

        match receive_messages(&self.receiver, &self.sender)? {
            MessageLoopState::Continue => Ok(()),
            MessageLoopState::Stop => {
                self.running = false;
                Ok(())
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }
}

pub fn mount_ui(
    sender: Sender<UiMessage>,
    receiver: Receiver<WindowingMessage>,
) -> Anyresult<UiRuntime> {
    UiRuntime::new(sender, receiver)
}

enum MessageLoopState {
    Continue,
    Stop,
}

fn handle_message(
    message: WindowingMessage,
    sender: &Sender<UiMessage>,
) -> Anyresult<MessageLoopState> {
    dbg!("Received message:", &message);
    match message {
        WindowingMessage::GtkEvent(e) => match e {
            EventKeys::Pressed { event } => {
                dbg!("Key pressed:", &event);
                if event.keysym == Keysym::Return {
                    dbg!("Enter key pressed");
                    sender
                        .send(UiMessage::UnlockWithPassword {
                            password: "test".into(),
                        })
                        .unwrap();
                }
            }
            EventKeys::Released { event } => {
                dbg!("Key released:", event);
            }
        },
        WindowingMessage::UnlockFailed => {
            dbg!("Unlock failed");
        }
        WindowingMessage::Quit => {
            dbg!("Quitting windowing thread");
            return Ok(MessageLoopState::Stop);
        }
        WindowingMessage::Ready { .. } => panic!("surface already configured"),
    }
    Ok(MessageLoopState::Continue)
}

fn receive_messages(
    receiver: &Receiver<WindowingMessage>,
    sender: &Sender<UiMessage>,
) -> Anyresult<MessageLoopState> {
    loop {
        let message = receiver.try_recv();
        match message {
            Ok(message) => {
                if let MessageLoopState::Stop = handle_message(message, sender)? {
                    return Ok(MessageLoopState::Stop);
                }
            }
            Err(TryRecvError::Empty) => {
                // No message available, continue the loop
                return Ok(MessageLoopState::Continue);
            }
            Err(TryRecvError::Disconnected) => {
                dbg!("Channel disconnected, exiting loop.");
                return Err(UmbrError::WindowingThreadQuit);
            }
        }
    }
}

fn wait_configure_and_render(receiver: &Receiver<WindowingMessage>) -> Anyresult<(u32, u32)> {
    let (width, height) = match receiver.recv().unwrap() {
        WindowingMessage::Ready { width, height } => (width, height),
        _ => panic!("Failed to receive render message"),
    };

    Ok((width, height))
}

struct PixbufSnapshot {
    pixels: Vec<u8>,
    width: i32,
    height: i32,
    stride: i32,
    n_channels: i32,
}

fn convert_to_pixels(width: u32, height: u32) -> PixbufSnapshot {
    gtk::init().expect("Failed to initialize GTK.");

    // 1. Crie o OffscreenWindow
    let offscreen = OffscreenWindow::new();
    offscreen.set_default_size(width as i32, height as i32);

    // 2. Monte sua interface normalmente
    let vbox = GtkBox::new(Orientation::Vertical, 10);
    let label = Label::new(Some("Olá, Locker!"));
    let button = Button::with_label("Clique!");
    vbox.pack_start(&label, false, false, 0);
    vbox.pack_start(&button, false, false, 0);

    offscreen.add(&vbox);

    // 3. Mostre tudo (mesmo fora da tela)
    offscreen.show_all();

    // 4. Force o GTK a processar eventos para garantir o render
    while gtk::events_pending() {
        gtk::main_iteration();
    }

    let buffer = offscreen.get_pixbuf().unwrap();

    let width = buffer.get_width();
    let height = buffer.get_height();
    let stride = buffer.get_rowstride();
    let n_channels = buffer.get_n_channels();
    let pixels = unsafe { buffer.get_pixels().to_vec() };

    PixbufSnapshot {
        pixels,
        width,
        height,
        stride,
        n_channels,
    }
}
