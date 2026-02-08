use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use slint::SharedString;
use slint::platform::femtovg_renderer::FemtoVGRenderer;
use slint_interpreter::{Compiler, ComponentDefinition, ComponentHandle, ComponentInstance, Value};
use uheex::types::Uheex;
use umbr_core::{Anyresult, UmbrError};

use self::adapter::MinimalFemtoVGWindow;
use self::egl::Context;
use self::slint_types::{OptionalProperties, RequiredCallbacks, RequiredProperties, SlintProperty};
use self::types::*;

mod adapter;
mod egl;
mod plataform;
mod slint_types;

pub mod types;
pub mod win;

pub struct UiRuntime {
    sender: Option<Sender<UiMessage>>,
    receiver: Option<Receiver<WindowingMessage>>,
    window: Rc<MinimalFemtoVGWindow>,
    layout: ComponentInstance,
    running: bool,
}

const LAYOUT_STYLE: &str = r#"
import { LineEdit } from "std-widgets.slint";
export component HelloWorld inherits Window {
    in property<string> clock_text;
    in property<bool> checking_password;

    in-out property<string> passwd <=> password.text;

    callback submit <=> password.accepted;

    forward-focus: password;

    Rectangle {
        background: #151515;
    }
    
    Rectangle {
        opacity: 0;
        clip: true;
        width: 10px;
        height: 10px;

        password := LineEdit {
            enabled: true;
            placeholder-text: "Enter password";
            input-type: InputType.password;
        }
    }

    VerticalLayout {
        alignment: center;
        spacing: 10px;
        padding: 40px;
        
        Text {
            text: clock_text;
            horizontal-alignment: center;
            font-size: 60pt;
            color: white;
        }
    }
}
"#;

impl UiRuntime {
    pub async fn standard(
        ast: Uheex,
        sender: Sender<UiMessage>,
        receiver: Receiver<WindowingMessage>,
    ) -> Anyresult<Self> {
        let window = wait_for_configure(&receiver)?;

        if let Ok(layout) = Self::load_style(ast.to_slint(), true).await {
            let layout = mount_ui(sender.clone(), layout)?;
            layout.show().unwrap();

            Ok(Self {
                window,
                layout,
                running: true,
                sender: Some(sender),
                receiver: Some(receiver),
            })
        } else {
            return Err(UmbrError::Generic(
                "Failed to load the UI layout".to_string(),
            ));
        }
    }

    async fn load_style(style: String, supress_warnings: bool) -> Anyresult<ComponentDefinition> {
        let mut compiler = Compiler::default();
        compiler.set_include_paths(vec![]);

        let result = compiler.build_from_source(style, Default::default()).await;
        result.print_diagnostics();
        let definition = result.component(result.component_names().next().unwrap_or_default());
        let definition = definition.ok_or(UmbrError::Generic(
            "Compiling the Slint code failed".to_owned(),
        ))?;

        let slint_properties: Vec<_> = definition.properties().map(SlintProperty::from).collect();
        RequiredProperties::check_propreties(&slint_properties)?;
        if let Err(UmbrError::Generic(properties)) =
            OptionalProperties::check_propreties(&slint_properties)
        {
            if !supress_warnings {
                // log::info!("The following optional properties are not set: {properties:?}");
                panic!("Missing optional properties: {:?}", properties);
            }
        }

        let slint_callbacks: Vec<_> = definition.callbacks().collect();
        RequiredCallbacks::check_callbacks(&slint_callbacks)?;

        Ok(definition)
    }

    pub fn process_messages(&mut self) -> Anyresult<()> {
        slint::platform::update_timers_and_animations();

        if !self.running {
            return Ok(());
        }

        if let (Some(receiver), window, layout) = (&self.receiver, &self.window, &self.layout) {
            if receiver_try_recv(receiver, Rc::clone(window), layout).is_err() {
                self.running = false;
                return Ok(());
            }
        }

        let _ = self.layout.set_property(
            &OptionalProperties::ClockText,
            SharedString::from("20:00").into(),
        );

        self.window.draw_if_needed();

        if self.window.has_active_animations() {
            let duration = slint::platform::duration_until_next_timer_update()
                .map_or(Duration::from_millis(8), |d| {
                    d.min(Duration::from_millis(8))
                });
            std::thread::sleep(duration);
        }

        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.running
    }
}

pub fn mount_ui(
    sender: Sender<UiMessage>,
    layout: ComponentDefinition,
) -> Anyresult<ComponentInstance> {
    let ui = layout.create().unwrap();

    let sender_c = sender.clone();
    let ui_ref = ui.as_weak();

    ui.set_callback(&RequiredCallbacks::Submit, move |args| -> Value {
        let ui = ui_ref.upgrade().unwrap();

        if args.len() < 1 {
            // only debugging
            sender_c
                .send(UiMessage::UnlockWithPassword {
                    password: "secret".to_string(),
                })
                .unwrap();

            return Value::Void;
        }

        let Value::String(passwd) = &args[0].clone() else {
            panic!("Expected string argument");
        };

        let _ = ui.set_property(&OptionalProperties::CheckingPassword, true.into());

        sender_c
            .send(UiMessage::UnlockWithPassword {
                password: passwd.to_string(),
            })
            .unwrap();

        Value::Void
    })
    .unwrap();

    Ok(ui)
}

fn wait_for_configure(
    receiver: &Receiver<WindowingMessage>,
) -> Anyresult<Rc<MinimalFemtoVGWindow>> {
    let (display_id, surface_id, size) = match receiver.recv().unwrap() {
        WindowingMessage::Ready {
            display_id,
            surface_id,
            width,
            height,
        } => (display_id, surface_id, (width, height)),
        _ => return Err(UmbrError::Generic("Expected Ready message".into())),
    };

    let ctx = Context::new(display_id, surface_id, size);
    let renderer = FemtoVGRenderer::new(ctx).unwrap();
    let window = MinimalFemtoVGWindow::new(renderer);

    window.set_size(slint::WindowSize::Physical(slint::PhysicalSize::new(
        size.0, size.1,
    )));

    let platform = plataform::UmbrPlatform::new(window.clone());
    slint::platform::set_platform(Box::new(platform)).unwrap();

    Ok(window)
}

fn handle_windowing_message(
    message: WindowingMessage,
    window: Rc<MinimalFemtoVGWindow>,
    _ui: &ComponentInstance,
) -> Anyresult<MessageLoopState> {
    match message {
        WindowingMessage::Event(event) => {
            window.dispatch_event(event);
        }
        WindowingMessage::UnlockFailed(reason) => {
            dbg!("Received UnlockFailed message");
            dbg!(&reason);
        }
        WindowingMessage::Quit => {
            dbg!("Received Quit message");
            return Ok(MessageLoopState::Stop);
        }
        WindowingMessage::Ready { .. } => {
            return Err(UmbrError::Generic("Unexpected Ready message".into()));
        }
    }

    Ok(MessageLoopState::Continue)
}

fn receiver_try_recv(
    receiver: &Receiver<WindowingMessage>,
    window: Rc<MinimalFemtoVGWindow>,
    ui: &ComponentInstance,
) -> Anyresult<MessageLoopState> {
    loop {
        let message = receiver.try_recv();

        match message {
            Ok(msg) => {
                handle_windowing_message(msg, window.clone(), ui)?;
            }
            Err(TryRecvError::Empty) => return Ok(MessageLoopState::Continue),
            Err(TryRecvError::Disconnected) => return Err(UmbrError::WindowingThreadQuit),
        }
    }
}
