use slint::{
    SharedString,
    platform::{Key, WindowEvent},
};
use smithay_client_toolkit::{
    delegate_keyboard, delegate_pointer, delegate_registry, delegate_seat, delegate_shm,
    reexports::client::Connection,
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerHandler},
    },
    shm::{
        Shm, ShmHandler,
        slot::{Buffer, SlotPool},
    },
};
use std::sync::mpsc::{Receiver, Sender};
use wayland_client::{
    Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop,
    globals::registry_queue_init,
    protocol::{
        wl_buffer::{self},
        wl_callback::{self, WlCallback},
        wl_compositor, wl_display,
        wl_keyboard::{self},
        wl_output, wl_pointer, wl_seat,
        wl_shm::WlShm,
        wl_surface::{self},
        wl_touch,
    },
};
use wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_manager_v1::{self},
    ext_session_lock_surface_v1, ext_session_lock_v1,
};

use crate::types::{UiMessage, WindowingMessage};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct WindowingApp {
    event_queue: EventQueue<AppData>,
    state: AppData,
    receiver: Receiver<UiMessage>,
}

impl WindowingApp {
    pub fn initialize(
        sender: Sender<WindowingMessage>,
        receiver: Receiver<UiMessage>,
    ) -> Result<Self> {
        let conn = match Connection::connect_to_env() {
            Ok(conn) => conn,
            Err(e) => {
                dbg!(&e);
                eprintln!("Wayland connect failed: {e:?}");
                eprintln!("WAYLAND_DISPLAY={:?}", std::env::var("WAYLAND_DISPLAY"));
                eprintln!("XDG_RUNTIME_DIR={:?}", std::env::var("XDG_RUNTIME_DIR"));
                panic!("erro");
            }
        };

        let display = conn.display();

        let event_queue = conn.new_event_queue();
        let qh = event_queue.handle();

        let (globals, _queue) = registry_queue_init::<AppData>(&conn).unwrap();

        let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 1..=5, ()).unwrap();
        let shm_state = Shm::bind(&globals, &qh).expect("wl_shm not available");
        let wl_surface = compositor.create_surface(&qh, ());
        let output: wl_output::WlOutput = globals.bind(&qh, 1..=1, ()).unwrap();
        let session_lock_manager: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = globals
            .bind(&qh, 1..=1, ())
            .map_err(|_| {
                "Could not bind ext-session-lock-v1. Your compositor probably does not support this."
            })?;
        let session_lock = session_lock_manager.lock(&qh, ());
        // set surface role as session lock surface
        session_lock.get_lock_surface(&wl_surface, &output, &qh, ());

        let state = AppData::new(
            RegistryState::new(&globals),
            display,
            wl_surface.clone(),
            session_lock,
            shm_state,
            SeatState::new(&globals, &qh),
            sender,
        );

        Ok(Self {
            event_queue,
            state,
            receiver,
        })
    }

    pub fn initial_roundtrip(&mut self) -> Result<()> {
        self.event_queue.roundtrip(&mut self.state)?;
        Ok(())
    }

    pub fn dispatch_blocking(&mut self) -> Result<()> {
        self.process_ui_messages()?;
        if self.state.running {
            self.event_queue.blocking_dispatch(&mut self.state)?;
        }
        self.process_ui_messages()?;
        Ok(())
    }

    pub fn dispatch_pending(&mut self) -> Result<()> {
        self.event_queue.dispatch_pending(&mut self.state)?;
        self.process_ui_messages()?;
        Ok(())
    }

    pub fn process_ui_messages(&mut self) -> Result<()> {
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                UiMessage::UnlockWithPassword { password } => {
                    if password.is_empty() {
                        self.state
                            .render_thread_sender
                            .send(WindowingMessage::UnlockFailed(
                                "Password cannot be empty".into(),
                            ))
                            .unwrap();
                        continue;
                    }

                    dbg!("password received:", &password);

                    // Flow to unlock
                    {
                        self.state.session_lock.unlock_and_destroy();

                        self.state.wl_surface.attach(None, 0, 0);
                        self.state.wl_surface.damage_buffer(
                            0,
                            0,
                            self.state.width as i32,
                            self.state.height as i32,
                        );
                        self.state.wl_surface.commit();

                        self.event_queue
                            .roundtrip(&mut self.state)
                            .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;

                        self.event_queue
                            .dispatch_pending(&mut self.state)
                            .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;
                        self.event_queue
                            .flush()
                            .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;
                        self.state
                            .render_thread_sender
                            .send(WindowingMessage::Quit)
                            .unwrap();
                        self.state.running = false;
                        self.state.locked = false;
                    }

                    // Flow error: wrong password
                    // {
                    //     self.state
                    //         .render_thread_sender
                    //         .send(WindowingMessage::UnlockFailed("Invalid password".into()))
                    //         .unwrap();
                    // }
                }
            }
        }

        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.state.running
    }
}

// TODO: Support multiple outputs
// FIXME: Remove touch support for now, and Connection should be
struct AppData {
    running: bool,
    locked: bool,
    configured: bool,

    width: u32,
    height: u32,
    buffers: Vec<(Buffer, SlotPool)>,

    registry_state: RegistryState,
    wl_surface: wl_surface::WlSurface,
    wl_display: wl_display::WlDisplay,
    session_lock: ext_session_lock_v1::ExtSessionLockV1,

    shm_state: Shm,
    seat_state: SeatState,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    touch: Option<wl_touch::WlTouch>,

    render_thread_sender: Sender<WindowingMessage>,
}

impl AppData {
    fn new(
        registry_state: RegistryState,
        display: wl_display::WlDisplay,
        surface: wl_surface::WlSurface,
        session_lock: ext_session_lock_v1::ExtSessionLockV1,
        shm: Shm,
        seat_state: SeatState,
        sender: Sender<WindowingMessage>,
    ) -> Self {
        Self {
            running: true,
            locked: false,
            configured: false,
            registry_state,
            wl_surface: surface,
            width: 0,
            height: 0,
            session_lock,
            wl_display: display,
            shm_state: shm,
            seat_state,
            buffers: Vec::new(),
            keyboard: None,
            pointer: None,
            touch: None,
            render_thread_sender: sender,
        }
    }
}

// Ignore events from these object types
delegate_noop!(AppData: ignore wl_compositor::WlCompositor);
delegate_noop!(AppData: ignore wl_surface::WlSurface);
// delegate_noop!(AppData: ignore wl_buffer::WlBuffer);
delegate_noop!(AppData: ignore wl_output::WlOutput);
delegate_noop!(AppData: ignore ext_session_lock_manager_v1::ExtSessionLockManagerV1);
// Delegate input
delegate_seat!(AppData);
delegate_keyboard!(AppData);
delegate_pointer!(AppData);
delegate_registry!(AppData);
delegate_shm!(AppData);

impl Dispatch<wl_buffer::WlBuffer, ()> for AppData {
    fn event(
        state: &mut Self,
        buffer: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_buffer::Event::Release => {
                // Handle buffer release
                state.buffers.retain(|(b, _)| b.wl_buffer() != buffer);
                dbg!("Buffer released and removed from tracking.");
            }
            _ => {}
        }
    }
}

impl ShmHandler for AppData {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

impl Dispatch<WlShm, ()> for AppData {
    fn event(
        _state: &mut Self,
        _proxy: &WlShm,
        _event: <WlShm as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl ProvidesRegistryState for AppData {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![SeatState,];
}

impl Dispatch<WlCallback, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ext_session_lock_v1::ExtSessionLockV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _: &ext_session_lock_v1::ExtSessionLockV1,
        event: ext_session_lock_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        dbg!(&event);
        match event {
            ext_session_lock_v1::Event::Locked => {
                state.locked = true;
            }
            ext_session_lock_v1::Event::Finished => {
                state.running = false;
            }
            _ => {}
        };
    }
}

impl Dispatch<ext_session_lock_surface_v1::ExtSessionLockSurfaceV1, ()> for AppData {
    fn event(
        state: &mut Self,
        surface: &ext_session_lock_surface_v1::ExtSessionLockSurfaceV1,
        event: ext_session_lock_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
        {
            state.width = width;
            state.height = height;

            let sender = &state.render_thread_sender;
            if !state.configured {
                sender
                    .send(WindowingMessage::Ready {
                        display_id: state.wl_display.id(),
                        surface_id: state.wl_surface.id(),
                        width,
                        height,
                    })
                    .unwrap();

                state.configured = true;
                surface.ack_configure(serial);
            }
        }
    }
}

fn sctk_key_event_to_slint(event: KeyEvent) -> Option<SharedString> {
    match event.keysym {
        Keysym::BackSpace => Some(Key::Backspace.into()),
        Keysym::Tab => Some(Key::Tab.into()),
        Keysym::Return => Some(Key::Return.into()),
        Keysym::Delete => Some(Key::Delete.into()),
        Keysym::Shift_L | Keysym::Shift_R => Some(Key::Shift.into()),
        Keysym::Control_L | Keysym::Control_R => Some(Key::Control.into()),
        Keysym::Alt_L | Keysym::Alt_R => Some(Key::Alt.into()),
        Keysym::Caps_Lock => Some(Key::CapsLock.into()),
        Keysym::Up => Some(Key::UpArrow.into()),
        Keysym::Down => Some(Key::DownArrow.into()),
        Keysym::Left => Some(Key::LeftArrow.into()),
        Keysym::Right => Some(Key::RightArrow.into()),
        Keysym::Insert => Some(Key::Insert.into()),
        Keysym::Home => Some(Key::Home.into()),
        Keysym::End => Some(Key::End.into()),
        _ => event.utf8.map(String::into),
    }
}

impl SeatHandler for AppData {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let keyboard = self
                .seat_state
                .get_keyboard(qh, &seat, None)
                .expect("Failed to create keyboard");

            self.keyboard = Some(keyboard);
        }

        if capability == Capability::Pointer && self.pointer.is_none() {
            let pointer = self
                .seat_state
                .get_pointer(qh, &seat)
                .expect("Failed to create pointer");
            self.pointer = Some(pointer);
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_some() {
            self.keyboard.take().unwrap().release();
        }

        if capability == Capability::Pointer && self.pointer.is_some() {
            self.pointer.take().unwrap().release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for AppData {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _keysyms: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if let Some(key) = sctk_key_event_to_slint(event) {
            self.render_thread_sender
                .send(WindowingMessage::Event(WindowEvent::KeyPressed {
                    text: key,
                }))
                .unwrap();
        }
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if let Some(key) = sctk_key_event_to_slint(event) {
            self.render_thread_sender
                .send(WindowingMessage::Event(WindowEvent::KeyReleased {
                    text: key,
                }))
                .unwrap();
        }
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _: Modifiers,
        _layout: u32,
    ) {
    }
}

impl PointerHandler for AppData {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        _events: &[PointerEvent],
    ) {
    }
}
