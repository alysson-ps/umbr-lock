extern crate gdk;
extern crate gtk;

use gio::prelude::*;
use gtk::{Widget, prelude::*};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use gtk::{Box as GtkBox, Label, OffscreenWindow, Orientation};

use smithay_client_toolkit::seat::keyboard::Keysym;
use uheex::types::{Expr, Uheex, VNode, Value, WidgetKind};
use umbr_core::{Anyresult, UmbrError};

use self::types::*;

pub mod types;
pub mod win;

pub struct UiRuntime {
    sender: Option<Sender<UiMessage>>,
    receiver: Option<Receiver<WindowingMessage>>,
    running: bool,
}

impl UiRuntime {
    pub fn standard(
        styles: Uheex,
        sender: Sender<UiMessage>,
        receiver: Receiver<WindowingMessage>,
    ) -> Anyresult<Self> {
        let (w, h) = wait_configure_and_render(&receiver)?;

        let pixbuf = convert_to_pixels(styles, w, h);

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
            sender: Some(sender),
            receiver: Some(receiver),
            running: true,
        })
    }

    pub fn preview(styles: Uheex) -> Anyresult<Self> {
        let app = gtk::Application::new(
            Some("com.example.UmbrPreview"),
            gio::ApplicationFlags::empty(),
        )
        .expect("Initialization failed...");

        app.connect_activate(move |app| {
            if let Some(css) = styles.generate_css() {
                let provider = gtk::CssProvider::new();

                provider
                    .load_from_data(css.as_bytes())
                    .expect("Failed to load CSS");

                gtk::StyleContext::add_provider_for_screen(
                    &gdk::Screen::get_default().expect("Error initializing gtk css provider."),
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }

            let window = gtk::ApplicationWindow::new(app);
            window.set_title("Umbr Locker Preview");
            window.set_border_width(10);
            window.set_position(gtk::WindowPosition::Center);
            window.set_default_size(1000, 560);
            window.set_resizable(false);
            window.set_decorated(false);

            let layout = create_layout(styles.clone());

            window.add(&layout.unwrap());

            window.connect_delete_event(|_, _| {
                gtk::main_quit();
                Inhibit(false)
            });

            window.show_all();
        });

        app.run(&[]);

        Ok(Self {
            sender: None,
            receiver: None,
            running: false,
        })
    }

    pub fn process_messages(&mut self) -> Anyresult<()> {
        if !self.running {
            return Ok(());
        }

        match receive_messages(
            &self.receiver.as_ref().unwrap(),
            &self.sender.as_ref().unwrap(),
        )? {
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

pub fn mount_ui_standard(
    styles: Uheex,
    sender: Sender<UiMessage>,
    receiver: Receiver<WindowingMessage>,
) -> Anyresult<UiRuntime> {
    UiRuntime::standard(styles, sender, receiver)
}

pub fn mount_ui_preview(styles: Uheex) -> Anyresult<UiRuntime> {
    UiRuntime::preview(styles)
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

fn create_layout(uheex: Uheex) -> Option<Widget> {
    if let VNode::Window { attributes, child } = uheex.root {
        let window = GtkBox::new(Orientation::Vertical, 10);

        child.iter().for_each(|node| {
            if let Some(widget) = convert_vnode_to_widget::<GtkBox>(node.clone(), None) {
                window.add(&widget);
            }
        });

        Some(window.upcast::<Widget>())
    } else {
        None
    }
}

fn convert_vnode_to_widget<W>(vnode: VNode, parent: Option<&W>) -> Option<Widget>
where
    W: IsA<gtk::Container> + 'static,
{
    match vnode {
        VNode::Widget {
            kind,
            attributes,
            child,
        } => {
            match kind {
                WidgetKind::Label => {
                    let class = attributes.get("class").and_then(|s| match s {
                        Expr::Value(Value::String(v)) => Some(v.as_str()),
                        _ => None,
                    });

                    let text = child.first().and_then(|node| match node {
                        VNode::String(value) => Some(value.as_str()),
                        _ => None,
                    });

                    let label = Label::new(text);

                    if let Some(class) = class {
                        label.get_style_context().add_class(class);
                    }

                    Some(label.upcast::<Widget>())
                }
                WidgetKind::Row => {
                    let spacing = attributes.get("spacing").and_then(|s| match s {
                        Expr::Value(Value::Number(v)) => Some(*v as i32),
                        _ => None,
                    });

                    let class = attributes.get("class").and_then(|s| match s {
                        Expr::Value(Value::String(v)) => Some(v.as_str()),
                        _ => None,
                    });

                    let row = GtkBox::new(Orientation::Horizontal, spacing.unwrap_or(0));

                    if let Some(class) = class {
                        row.get_style_context().add_class(class);
                    }

                    child.iter().for_each(|node| {
                        if let Some(widget) = convert_vnode_to_widget(node.clone(), Some(&row)) {
                            row.add(&widget);
                        }
                    });

                    Some(row.upcast::<Widget>())
                }
                WidgetKind::Column => {
                    let spacing = attributes.get("spacing").and_then(|s| match s {
                        Expr::Value(Value::Number(v)) => Some(*v as i32),
                        _ => None,
                    });

                    let class = attributes.get("class").and_then(|s| match s {
                        Expr::Value(Value::String(v)) => Some(v.as_str()),
                        _ => None,
                    });

                    let column = GtkBox::new(Orientation::Vertical, spacing.unwrap_or(0));

                    if let Some(class) = class {
                        column.get_style_context().add_class(class);
                    }

                    child.iter().for_each(|node| {
                        if let Some(widget) = convert_vnode_to_widget(node.clone(), Some(&column)) {
                            column.add(&widget);
                        }
                    });

                    Some(column.upcast::<Widget>())
                }
                WidgetKind::Custom => {
                    // Handle custom widgets or ignore
                    unimplemented!("Custom widgets are not supported");
                }
            }
        }

        VNode::Fragment(nodes) => {
            if let Some(parent) = parent {
                for node in *nodes {
                    if let Some(widget) = convert_vnode_to_widget::<W>(node, Some(parent)) {
                        parent.add(&widget);
                    }
                }
            }

            None
        }

        _ => unimplemented!("Only widget nodes are supported"),
    }
}

fn convert_to_pixels(layout: Uheex, width: u32, height: u32) -> PixbufSnapshot {
    gtk::init().expect("Failed to initialize GTK.");

    let offscreen = OffscreenWindow::new();

    offscreen.set_default_size(width as i32, height as i32);

    if let Some(css) = layout.generate_css() {
        let provider = gtk::CssProvider::new();

        provider
            .load_from_data(css.as_bytes())
            .expect("Failed to load CSS");

        gtk::StyleContext::add_provider_for_screen(
            &gdk::Screen::get_default().expect("Error initializing gtk css provider."),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    if let Some(layout) = create_layout(layout) {
        offscreen.add(&layout);
    }

    offscreen.show_all();

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
