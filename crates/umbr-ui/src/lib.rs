use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use renderer::{WgpuRenderer, run_preview};
use uheex::types::{Expr, Uheex, VNode, Value, WidgetKind};
use umbr_core::{Anyresult, UmbrError};

use self::keyboard::listen_for_keyboard_events;
use self::state::PasswordBuffer;
use self::types::*;

mod renderer;

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
    renderer: Option<WgpuRenderer>,
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
                        Ok(frame) => {
                            if let Err(err) = sender.send(UiMessage::Render {
                                width: frame.width,
                                height: frame.height,
                                stride: frame.stride,
                                n_channels: frame.n_channels,
                                pixels: frame.pixels,
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
            self.renderer = Some(WgpuRenderer::new()?);
        }

        if let Some(renderer) = self.renderer.as_mut() {
            let frame = renderer.render(&self.layout, w, h)?;

            sender
                .send(UiMessage::Render {
                    width: frame.width,
                    height: frame.height,
                    stride: frame.stride,
                    n_channels: frame.n_channels,
                    pixels: frame.pixels,
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
        run_preview(&self.layout)?;

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
