use std::cell::Cell;
use std::rc::{Rc, Weak};

use slint::platform::femtovg_renderer::FemtoVGRenderer;
use slint::platform::{WindowAdapter, WindowEvent};
use slint::{PhysicalSize, Window};

pub struct MinimalFemtoVGWindow {
    window: Window,
    renderer: FemtoVGRenderer,
    needs_redraw: Cell<bool>,
    size: Cell<PhysicalSize>,
}

impl MinimalFemtoVGWindow {
    pub fn new(renderer: FemtoVGRenderer) -> Rc<Self> {
        Rc::new_cyclic(|w: &Weak<Self>| Self {
            window: Window::new(w.clone()),
            renderer,
            needs_redraw: Default::default(),
            size: Default::default(),
        })
    }

    pub fn draw_if_needed(&self) {
        if self.needs_redraw.get() {
            self.renderer.render().unwrap();
            self.needs_redraw.set(false);
        }
    }
}

impl WindowAdapter for MinimalFemtoVGWindow {
    fn window(&self) -> &slint::Window {
        &self.window
    }

    fn renderer(&self) -> &dyn slint::platform::Renderer {
        &self.renderer
    }

    fn size(&self) -> slint::PhysicalSize {
        self.size.get()
    }

    fn set_size(&self, size: slint::WindowSize) {
        self.size.set(size.to_physical(1.0));

        self.window.dispatch_event(WindowEvent::Resized {
            size: size.to_logical(1.0),
        });
    }

    fn request_redraw(&self) {
        self.needs_redraw.set(true);
    }
}

impl core::ops::Deref for MinimalFemtoVGWindow {
    type Target = slint::Window;

    fn deref(&self) -> &Self::Target {
        &self.window
    }
}
