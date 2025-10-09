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
use std::{
    convert::TryFrom,
    sync::mpsc::{Receiver, Sender},
};
use wayland_client::{
    Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop,
    globals::registry_queue_init,
    protocol::{
        wl_buffer::{self},
        wl_callback::{self, WlCallback},
        wl_compositor, wl_display,
        wl_keyboard::{self},
        wl_output, wl_pointer, wl_seat,
        wl_shm::{self, WlShm},
        wl_surface::{self},
        wl_touch,
    },
};
use wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_manager_v1::{self},
    ext_session_lock_surface_v1, ext_session_lock_v1,
};

use crate::types::{EventKeys, UiMessage, WindowingMessage};

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
            conn,
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
                UiMessage::Render {
                    width,
                    height,
                    stride,
                    n_channels,
                    pixels,
                } => {
                    self.state
                        .render(&pixels, width, height, stride, n_channels);
                }
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
    connection: Connection,

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
        connection: Connection,
        registry_state: RegistryState,
        display: wl_display::WlDisplay,
        surface: wl_surface::WlSurface,
        session_lock: ext_session_lock_v1::ExtSessionLockV1,
        shm: Shm,
        seat_state: SeatState,
        sender: Sender<WindowingMessage>,
    ) -> Self {
        Self {
            connection,
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

    fn render(&mut self, pixels: &[u8], width: i32, height: i32, stride: i32, n_channels: i32) {
        if width <= 0 || height <= 0 {
            eprintln!("Invalid pixbuf dimensions: {}x{}", width, height);
            return;
        }

        if stride <= 0 {
            eprintln!("Invalid pixbuf stride: {}", stride);
            return;
        }

        if n_channels != 3 && n_channels != 4 {
            eprintln!("Unsupported channel count: {}", n_channels);
            return;
        }

        let (w, h) = (width as usize, height as usize);
        let stride_usize = stride as usize;
        let expected_len = match stride_usize.checked_mul(h) {
            Some(len) => len,
            None => {
                eprintln!("Overflow expected_len");
                return;
            }
        };

        if pixels.len() != expected_len {
            eprintln!(
                "Unexpected pixel buffer length: expected {}, got {}",
                expected_len,
                pixels.len()
            );
            return;
        }

        let channels = n_channels as usize;
        let row_bytes = match w.checked_mul(channels) {
            Some(v) => v,
            None => {
                eprintln!("Overflow row_bytes");
                return;
            }
        };

        if row_bytes > stride_usize {
            eprintln!(
                "Row byte count ({}) exceeds stride ({}).",
                row_bytes, stride_usize
            );
            return;
        }

        let dst_stride = match w.checked_mul(4) {
            Some(v) => v,
            None => {
                eprintln!("Overflow dst_stride");
                return;
            }
        };
        let buffer_size = match dst_stride.checked_mul(h) {
            Some(v) => v,
            None => {
                eprintln!("Overflow buffer_size");
                return;
            }
        };

        let mut pool = SlotPool::new(buffer_size, &self.shm_state).expect("create slot pool");
        let dst_stride_i32 = i32::try_from(dst_stride).unwrap_or_else(|_| {
            eprintln!("dst_stride doesn't fit i32");
            0
        });
        if dst_stride_i32 == 0 {
            return;
        }

        let (wlbuf, canvas) = pool
            .create_buffer(width, height, dst_stride_i32, wl_shm::Format::Argb8888)
            .expect("create buffer");

        #[inline(always)]
        fn premul(c: u8, a: u8) -> u8 {
            // arredondado
            ((c as u16 * a as u16 + 127) / 255) as u8
        }

        let has_alpha = channels == 4;

        for y in 0..h {
            let src_offset = y * stride_usize;
            let dst_offset = y * dst_stride;

            let src_line = &pixels[src_offset..src_offset + row_bytes];
            let dst_line = &mut canvas[dst_offset..dst_offset + dst_stride];

            if has_alpha {
                // RGBA -> ARGB (premult)
                for x in 0..w {
                    let base = x * 4;
                    let r = src_line[base + 0];
                    let g = src_line[base + 1];
                    let b = src_line[base + 2];
                    let a = src_line[base + 3];

                    let rp = premul(r, a);
                    let gp = premul(g, a);
                    let bp = premul(b, a);

                    let px: u32 =
                        ((a as u32) << 24) | ((rp as u32) << 16) | ((gp as u32) << 8) | (bp as u32);

                    dst_line[x * 4..x * 4 + 4].copy_from_slice(&px.to_ne_bytes());
                }
            } else {
                // RGB -> ARGB (a=255, sem premul efetivo)
                for x in 0..w {
                    let base = x * 3;
                    let r = src_line[base + 0];
                    let g = src_line[base + 1];
                    let b = src_line[base + 2];
                    let a = 255u8;

                    let px: u32 =
                        ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);

                    dst_line[x * 4..x * 4 + 4].copy_from_slice(&px.to_ne_bytes());
                }
            }
        }

        self.buffers.push((wlbuf, pool));

        self.wl_surface
            .attach(Some(&self.buffers.last().unwrap().0.wl_buffer()), 0, 0);
        self.wl_surface.damage_buffer(0, 0, width, height);
        self.wl_surface.commit();
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
        dbg!(&event);
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
                state.configured = true;
                surface.ack_configure(serial);

                sender
                    .send(WindowingMessage::Ready { width, height })
                    .unwrap();
            }
        }
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
        self.render_thread_sender
            .send(WindowingMessage::GtkEvent(EventKeys::Pressed { event }))
            .unwrap();
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.render_thread_sender
            .send(WindowingMessage::GtkEvent(EventKeys::Released { event }))
            .unwrap();
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
