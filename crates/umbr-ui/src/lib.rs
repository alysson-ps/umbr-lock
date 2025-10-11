extern crate gdk;
extern crate gtk;

use gdk::keys::constants::{U, l};
use gio::prelude::*;
use gtk::{Overlay, Widget, prelude::*};
use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use gtk::{Box as GtkBox, Label, Orientation};

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
    render_dimensions: Option<(u32, u32)>,
    count_global_index: Option<usize>,
    last_count: Option<usize>,
    renderer: Option<GtkRenderer>,
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
            render_dimensions: None,
            count_global_index: None,
            last_count: None,
            renderer: None,
        }
    }

    pub fn set_layout(&mut self, mut layout: Uheex) {
        let index = ensure_count_variable(&mut layout, 0);

        self.original_layout = Some(layout.clone());
        self.count_global_index = Some(index);
        self.last_count = Some(0);

        self.layout = layout;
        self.layout.evaluate();
    }

    pub fn update_count(&mut self, count: usize) {
        if self.last_count == Some(count) {
            return;
        }

        if let Some(original) = self.original_layout.as_mut() {
            let index = match self.count_global_index {
                Some(index)
                    if matches!(
                        original.globals.get(index),
                        Some(VNode::Variable { name, .. }) if name == "count"
                    ) =>
                {
                    index
                }
                _ => {
                    let index = ensure_count_variable(original, count);
                    self.count_global_index = Some(index);
                    index
                }
            };

            if let Some(VNode::Variable { value, .. }) = original.globals.get_mut(index) {
                *value = Expr::Value(Value::Number(count as f64));
            }

            self.layout = original.clone();
            self.layout.evaluate();
            self.last_count = Some(count);

            if self.running {
                if let (Some((width, height)), Some(sender), Some(renderer)) = (
                    self.render_dimensions,
                    self.sender.as_ref(),
                    self.renderer.as_mut(),
                ) {
                    match renderer.render(&self.layout, width, height) {
                        Ok(pixbuf) => {
                            if let Err(err) = sender.send(UiMessage::Render {
                                width: pixbuf.width,
                                height: pixbuf.height,
                                stride: pixbuf.stride,
                                n_channels: pixbuf.n_channels,
                                pixels: pixbuf.pixels,
                            }) {
                                eprintln!("Failed to send re-render message: {err}");
                            }
                        }
                        Err(err) => {
                            eprintln!("Failed to re-render layout: {err}");
                        }
                    }
                }
            }
        }
    }

    pub fn standard(
        &mut self,
        sender: Sender<UiMessage>,
        receiver: Receiver<WindowingMessage>,
    ) -> Anyresult<()> {
        let (w, h) = wait_configure_and_render(&receiver)?;

        self.render_dimensions = Some((w, h));

        if self.renderer.is_none() {
            self.renderer = Some(GtkRenderer::new(w, h)?);
        }

        if let Some(renderer) = self.renderer.as_mut() {
            let pixbuf = renderer.render(&self.layout, w, h)?;

            sender
                .send(UiMessage::Render {
                    width: pixbuf.width,
                    height: pixbuf.height,
                    stride: pixbuf.stride,
                    n_channels: pixbuf.n_channels,
                    pixels: pixbuf.pixels,
                })
                .map_err(|err| {
                    UmbrError::Generic(format!("failed to send render message: {err}"))
                })?;
        }

        self.buffer = Some(PasswordBuffer::new());
        self.sender = Some(sender);
        self.receiver = Some(receiver);
        self.running = true;

        Ok(())
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
            original_layout: self.original_layout.clone(),
            buffer: None,
            sender: None,
            receiver: None,
            running: false,
            render_dimensions: None,
            count_global_index: self.count_global_index,
            last_count: self.last_count,
            renderer: None,
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

fn ensure_count_variable(layout: &mut Uheex, count: usize) -> usize {
    if let Some((index, node)) = layout
        .globals
        .iter_mut()
        .enumerate()
        .find(|(_, node)| matches!(node, VNode::Variable { name, .. } if name == "count"))
    {
        if let VNode::Variable { value, initial, .. } = node {
            let expr = Expr::Value(Value::Number(count as f64));
            *value = expr.clone();
            *initial = Some(expr);
        }

        index
    } else {
        layout.globals.push(VNode::Variable {
            name: "count".into(),
            initial: Some(Expr::Value(Value::Number(count as f64))),
            value: Expr::Value(Value::Number(count as f64)),
            interval: Duration::from_secs(1),
        });

        layout.globals.len() - 1
    }
}

pub fn mount_ui_standard(
    styles: Uheex,
    sender: Sender<UiMessage>,
    receiver: Receiver<WindowingMessage>,
) -> Anyresult<UiRuntime> {
    let mut runtime = UiRuntime::new();
    runtime.set_layout(styles);
    runtime.standard(sender, receiver)?;
    Ok(runtime)
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

struct GtkRenderer {
    offscreen: gtk::OffscreenWindow,
    css_provider: gtk::CssProvider,
    css_signature: Option<u64>,
}

impl GtkRenderer {
    fn new(width: u32, height: u32) -> Anyresult<Self> {
        if !gtk::is_initialized() {
            gtk::init().map_err(|err| UmbrError::Generic(err.to_string()))?;
        }

        let offscreen = gtk::OffscreenWindow::new();
        offscreen.set_default_size(width as i32, height as i32);

        let css_provider = gtk::CssProvider::new();

        Ok(Self {
            offscreen,
            css_provider,
            css_signature: None,
        })
    }

    fn ensure_css(&mut self, css: Option<&str>) {
        match css {
            Some(css) => {
                let mut hasher = DefaultHasher::new();
                css.hash(&mut hasher);
                let signature = hasher.finish();

                if self.css_signature != Some(signature) {
                    if let Err(err) = self.css_provider.load_from_data(css.as_bytes()) {
                        eprintln!("Failed to load CSS: {err}");
                    } else if let Some(screen) = gdk::Screen::get_default() {
                        gtk::StyleContext::add_provider_for_screen(
                            &screen,
                            &self.css_provider,
                            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                        );
                        self.css_signature = Some(signature);
                    }
                }
            }
            None => {
                self.css_signature = None;
            }
        }
    }

    fn render(&mut self, layout: &Uheex, width: u32, height: u32) -> Anyresult<PixbufSnapshot> {
        self.offscreen.set_default_size(width as i32, height as i32);

        self.ensure_css(layout.generate_css().as_deref());

        for child in self.offscreen.get_children() {
            self.offscreen.remove(&child);
        }

        if let Some(widget) = create_layout(layout) {
            self.offscreen.add(&widget);
        }

        self.offscreen.show_all();

        while gtk::events_pending() {
            gtk::main_iteration();
        }

        let buffer = self
            .offscreen
            .get_pixbuf()
            .ok_or_else(|| UmbrError::Generic("offscreen window did not produce pixbuf".into()))?;

        let width = buffer.get_width();
        let height = buffer.get_height();
        let stride = buffer.get_rowstride();
        let n_channels = buffer.get_n_channels();
        let pixels = unsafe { buffer.get_pixels().to_vec() };

        Ok(PixbufSnapshot {
            pixels,
            width,
            height,
            stride,
            n_channels,
        })
    }
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
