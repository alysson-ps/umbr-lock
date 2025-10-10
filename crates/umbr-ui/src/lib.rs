extern crate gdk;
extern crate gtk;

use gdk::keys::constants::{U, l};
use gio::prelude::*;
use gtk::{Overlay, Widget, prelude::*};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use gtk::{Box as GtkBox, Label, OffscreenWindow, Orientation};

use uheex::types::{Expr, Uheex, VNode, Value, WidgetKind};
use umbr_core::{Anyresult, UmbrError};

use self::keyboard::listen_for_keyboard_events;
use self::state::PasswordBuffer;
use self::types::*;

mod keyboard;
mod state;

pub mod types;
pub mod win;

pub struct UiRuntime {
    layout: Uheex,
    original_layout: Option<Uheex>,
    buffer: Option<PasswordBuffer>,
    sender: Option<Sender<UiMessage>>,
    receiver: Option<Receiver<WindowingMessage>>,
    running: bool,
}

impl UiRuntime {
    pub fn new() -> Self {
        Self {
            layout: Uheex {
                globals: vec![],
                root: VNode::Empty,
                stylesheet: None,
            },
            original_layout: None,
            buffer: None,
            sender: None,
            receiver: None,
            running: false,
        }
    }

    pub fn set_layout(&mut self, layout: Uheex) {
        self.original_layout = Some(layout.clone());

        self.update_count(0);
    }

    pub fn update_count(&mut self, count: usize) {
        if let Some(original) = &self.original_layout {
            let mut updated = original.clone();

            updated.globals.push(VNode::Variable {
                name: "count".into(),
                initial: Some(Expr::Value(Value::Number(0 as f64))),
                value: Expr::Value(Value::Number(count as f64)),
                interval: Duration::from_secs(1),
            });

            updated.evaluate();

            self.layout = updated;
        }
    }

    pub fn standard(
        &mut self,
        sender: Sender<UiMessage>,
        receiver: Receiver<WindowingMessage>,
    ) -> Anyresult<Self> {
        let (w, h) = wait_configure_and_render(&receiver)?;

        let pixbuf = convert_to_pixels(&self.layout, w, h);

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
            layout: self.layout.clone(),
            original_layout: Some(self.layout.clone()),
            buffer: Some(PasswordBuffer::new()),
            sender: Some(sender),
            receiver: Some(receiver),
            running: true,
        })
    }

    pub fn preview(&mut self) -> Anyresult<Self> {
        let app = gtk::Application::new(
            Some("com.example.UmbrPreview"),
            gio::ApplicationFlags::empty(),
        )
        .expect("Initialization failed...");

        let self_clone = self.layout.clone();

        app.connect_activate(move |app| {
            if let Some(css) = self_clone.generate_css() {
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
            window.set_default_size(1000, 560);
            window.set_resizable(false);
            window.set_decorated(false);

            if let Some(layout) = create_layout(&self_clone) {
                window.add(&layout);
            }

            window.connect_delete_event(|_, _| {
                gtk::main_quit();
                Inhibit(false)
            });

            window.show_all();
        });

        app.run(&[]);

        Ok(Self {
            layout: self.layout.clone(),
            original_layout: Some(self.layout.clone()),
            buffer: None,
            sender: None,
            receiver: None,
            running: false,
        })
    }

    pub fn process_messages(&mut self) -> Anyresult<()> {
        if !self.running {
            return Ok(());
        }

        match receive_messages(self)? {
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
    let mut runtime = UiRuntime::new();
    runtime.set_layout(styles);
    runtime.standard(sender, receiver)
}

pub fn mount_ui_preview(styles: Uheex) -> Anyresult<UiRuntime> {
    let mut runtime = UiRuntime::new();
    runtime.set_layout(styles);
    runtime.preview()
}

fn handle_message(
    runtime: &mut UiRuntime,
    message: WindowingMessage,
) -> Anyresult<MessageLoopState> {
    match message {
        WindowingMessage::GtkEvent(e) => {
            listen_for_keyboard_events(e, runtime);
        }
        WindowingMessage::UnlockFailed(message) => {
            dbg!(&message);
        }
        WindowingMessage::Quit => {
            dbg!("Quitting windowing thread");
            return Ok(MessageLoopState::Stop);
        }
        WindowingMessage::Ready { .. } => panic!("surface already configured"),
    }
    Ok(MessageLoopState::Continue)
}

fn receive_messages(runtime: &mut UiRuntime) -> Anyresult<MessageLoopState> {
    loop {
        let message = runtime.receiver.as_ref().unwrap().try_recv();
        match message {
            Ok(message) => {
                if let MessageLoopState::Stop = handle_message(runtime, message)? {
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

#[derive(Debug, Clone)]
struct Options {
    monitor: Option<i32>,
    anchor: Option<(gtk::Align, gtk::Align)>,
}

fn create_layout(uheex: &Uheex) -> Option<Widget> {
    if let VNode::Window { attributes, child } = &uheex.root {
        let monitor = attributes.get("monitor").and_then(|s| match s {
            Expr::Value(Value::Number(v)) => Some(*v as i32),
            _ => None,
        });

        let anchor = attributes.get("anchor").and_then(|s| match s {
            Expr::Array(v) if v.len() == 2 => {
                let h_anchor = v[0].clone();
                let v_anchor = v[1].clone();

                let h_anchor = match h_anchor {
                    Expr::Value(Value::String(ref s)) if s == "left" => gtk::Align::Start,
                    Expr::Value(Value::String(ref s)) if s == "center" => gtk::Align::Center,
                    Expr::Value(Value::String(ref s)) if s == "right" => gtk::Align::End,
                    _ => gtk::Align::Center,
                };

                let v_anchor = match v_anchor {
                    Expr::Value(Value::String(ref s)) if s == "top" => gtk::Align::Start,
                    Expr::Value(Value::String(ref s)) if s == "center" => gtk::Align::Center,
                    Expr::Value(Value::String(ref s)) if s == "bottom" => gtk::Align::End,
                    _ => gtk::Align::Center,
                };

                Some((h_anchor, v_anchor))
            }
            _ => None,
        });

        let overlay = Overlay::new();

        child.iter().for_each(|node| {
            if let Some(widget) = convert_vnode_to_widget::<GtkBox>(
                node.clone(),
                None,
                Rc::new(overlay.clone()),
                Options { monitor, anchor },
            ) {
                overlay.add(&widget);
            }
        });

        Some(overlay.upcast::<Widget>())
    } else {
        None
    }
}

fn convert_vnode_to_widget<W>(
    vnode: VNode,
    parent: Option<&W>,
    overlay: Rc<Overlay>,
    options: Options,
) -> Option<Widget>
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
                    let text = child.first().and_then(|node| match node {
                        VNode::String(value) => Some(value.as_str()),
                        _ => None,
                    });

                    let label = Label::new(text);
                    apply_attributes(&label, &attributes, &options);

                    Some(label.upcast::<Widget>())
                }
                WidgetKind::Row => {
                    let spacing = attributes.get("spacing").and_then(|s| match s {
                        Expr::Value(Value::Number(v)) => Some(*v as i32),
                        _ => None,
                    });

                    let row = GtkBox::new(Orientation::Horizontal, spacing.unwrap_or(0));

                    apply_attributes(&row, &attributes, &options);

                    if parent.is_none() {
                        row.set_hexpand(true);
                        row.set_vexpand(true);
                        row.set_halign(gtk::Align::Fill);
                        row.set_valign(gtk::Align::Fill);
                    }

                    child.iter().for_each(|node| {
                        if let Some(widget) = convert_vnode_to_widget(
                            node.clone(),
                            Some(&row),
                            overlay.clone(),
                            options.clone(),
                        ) {
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

                    let column = GtkBox::new(Orientation::Vertical, spacing.unwrap_or(0));

                    apply_attributes(&column, &attributes, &options);

                    if parent.is_none() {
                        column.set_hexpand(true);
                        column.set_vexpand(true);
                        column.set_halign(gtk::Align::Fill);
                        column.set_valign(gtk::Align::Fill);
                    }

                    child.iter().for_each(|node| {
                        if let Some(widget) = convert_vnode_to_widget(
                            node.clone(),
                            Some(&column),
                            overlay.clone(),
                            options.clone(),
                        ) {
                            column.add(&widget);
                        }
                    });

                    Some(column.upcast::<Widget>())
                }
                WidgetKind::Absolute => {
                    let x = attributes.get("x").and_then(|s| match s {
                        Expr::Value(Value::String(v)) => v.parse::<i32>().ok(),
                        Expr::Value(Value::Number(v)) => Some(*v as i32),
                        _ => None,
                    });

                    let y = attributes.get("y").and_then(|s| match s {
                        Expr::Value(Value::String(v)) => v.parse::<i32>().ok(),
                        Expr::Value(Value::Number(v)) => Some(*v as i32),
                        _ => None,
                    });

                    let fixed = gtk::Fixed::new();

                    if let Some(widget) = convert_vnode_to_widget::<W>(
                        child.first()?.clone(),
                        parent,
                        overlay.clone(),
                        options.clone(),
                    ) {
                        fixed.put(&widget, x.unwrap_or(0), y.unwrap_or(0));
                        fixed.set_valign(
                            options
                                .clone()
                                .anchor
                                .map(|(_, v)| v)
                                .unwrap_or(gtk::Align::Start),
                        );
                        fixed.set_halign(
                            options
                                .clone()
                                .anchor
                                .map(|(h, _)| h)
                                .unwrap_or(gtk::Align::Start),
                        );
                        overlay.add_overlay(&fixed);
                    }

                    None
                }
                // WidgetKind::Custom => {
                //     // Handle custom widgets or ignore
                //     unimplemented!("Custom widgets are not supported");
                // }
                _ => unimplemented!("Widget kind not supported"),
            }
        }

        VNode::Fragment(nodes) => {
            if let Some(parent) = parent {
                for node in *nodes {
                    if let Some(widget) = convert_vnode_to_widget::<W>(
                        node,
                        Some(parent),
                        overlay.clone(),
                        options.clone(),
                    ) {
                        parent.add(&widget);
                    }
                }
            }

            None
        }

        _ => None,
    }
}

fn apply_attributes<W>(widget: &W, attributes: &BTreeMap<String, Expr>, options: &Options)
where
    W: IsA<gtk::Widget> + 'static,
{
    // Get attributes
    let align = attributes.get("align").and_then(|s| match s {
        Expr::Array(v) if v.len() == 2 => {
            let h_align = v[0].clone();
            let v_align = v[1].clone();

            let h_align = match h_align {
                Expr::Value(Value::String(ref s)) if s == "start" => gtk::Align::Start,
                Expr::Value(Value::String(ref s)) if s == "center" => gtk::Align::Center,
                Expr::Value(Value::String(ref s)) if s == "end" => gtk::Align::End,
                _ => gtk::Align::Start,
            };

            let v_align = match v_align {
                Expr::Value(Value::String(ref s)) if s == "top" => gtk::Align::Start,
                Expr::Value(Value::String(ref s)) if s == "center" => gtk::Align::Center,
                Expr::Value(Value::String(ref s)) if s == "bottom" => gtk::Align::End,
                _ => gtk::Align::Start,
            };

            Some((h_align, v_align))
        }
        _ => None,
    });

    let flexible = attributes.get("flexible").and_then(|s| match s {
        Expr::Value(Value::String(v)) => Some(v == "true"),
        _ => None,
    });

    let class = attributes.get("class").and_then(|s| match s {
        Expr::Value(Value::String(v)) => Some(v.as_str()),
        _ => None,
    });

    let size = attributes.get("size").and_then(|s| match s {
        Expr::Array(v) if v.len() == 2 => {
            let width = v[0].clone();
            let height = v[1].clone();

            let width = match width {
                Expr::Value(Value::String(ref s)) => s.parse::<i32>().ok().unwrap_or(-1),
                Expr::Value(Value::Number(v)) => v as i32,
                _ => -1,
            };

            let height = match height {
                Expr::Value(Value::String(ref s)) => s.parse::<i32>().ok().unwrap_or(-1),
                Expr::Value(Value::Number(v)) => v as i32,
                _ => -1,
            };

            Some((width, height))
        }
        _ => None,
    });

    // Apply class
    if let Some(class) = class {
        widget.get_style_context().add_class(class);
    }

    // Apply flexibility
    if flexible.unwrap_or(false) {
        widget.set_hexpand(true);
        widget.set_vexpand(true);
    }

    // Apply size
    if let Some((width, height)) = size {
        widget.set_size_request(width, height);
    }

    // Apply alignment
    if let Some((h_align, v_align)) = align.or(options.anchor) {
        widget.set_halign(h_align);
        widget.set_valign(v_align);
    }
}

fn convert_to_pixels(layout: &Uheex, width: u32, height: u32) -> PixbufSnapshot {
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
