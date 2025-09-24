#![allow(warnings)]

extern crate gdk;
extern crate gtk;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::thread;

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

pub fn mount_ui(sender: Sender<UiMessage>, receiver: Receiver<WindowingMessage>) -> Anyresult<()> {
    let (w, h) = wait_configure_and_render(&receiver)?;

    let (pixels, stride, channels) = convert_to_pixels(w, h);

    sender
        .send(UiMessage::Render {
            width: w as i32,
            height: h as i32,
            stride,
            channels,
            pixels,
        })
        .unwrap();

    loop {
        if receive_messages(&receiver, &sender).is_err() {
            dbg!("Exiting mount_ui loop");
            return Ok(());
        }
    }

    Ok(())
}

fn handle_message(message: WindowingMessage, sender: &Sender<UiMessage>) -> Anyresult<()> {
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
            return Err(UmbrError::WindowingThreadQuit);
        }
        WindowingMessage::Ready { .. } => panic!("surface already configured"),
    }
    Ok(())
}

fn receive_messages(
    receiver: &Receiver<WindowingMessage>,
    sender: &Sender<UiMessage>,
) -> Anyresult<()> {
    loop {
        let message = receiver.try_recv();
        match message {
            Ok(message) => handle_message(message, sender)?,
            Err(TryRecvError::Empty) => {
                // No message available, continue the loop
                return Ok(());
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

fn convert_to_pixels(width: u32, height: u32) -> (Vec<u8>, i32, i32) {
    gtk::init().expect("Failed to initialize GTK.");

    // 1. Crie o OffscreenWindow
    let offscreen = OffscreenWindow::new();

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
    let stride = buffer.get_rowstride();
    let channels = buffer.get_n_channels();

    let pixels = unsafe { buffer.get_pixels().to_vec() };

    (pixels, stride, channels)
}
