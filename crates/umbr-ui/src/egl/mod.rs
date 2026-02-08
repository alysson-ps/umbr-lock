use std::ffi::{CStr, c_void};
use std::num::NonZeroU32;
use std::ptr::NonNull;

use glutin::api::egl::context::PossiblyCurrentContext;
use glutin::api::egl::display::Display;
use glutin::api::egl::surface::Surface;
use glutin::config::{ConfigTemplateBuilder, GlConfig};
use glutin::context::{ContextAttributesBuilder, NotCurrentGlContext};
use glutin::display::GetGlDisplay;
use glutin::prelude::{GlDisplay, GlSurface, PossiblyCurrentGlContext};
use glutin::surface::{SurfaceAttributesBuilder, WindowSurface};
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};

use slint::platform::femtovg_renderer::OpenGLInterface;
use wayland_client::backend::ObjectId;

pub struct Context {
    ctx: PossiblyCurrentContext,
    surface: Surface<WindowSurface>,
}

impl Context {
    pub fn new(display_id: ObjectId, surface_id: ObjectId, size: (u32, u32)) -> Self {
        let handle = WaylandDisplayHandle::new(
            NonNull::<c_void>::new(display_id.as_ptr() as *mut _).expect("Display handle is null"),
        );

        let display_handle = RawDisplayHandle::Wayland(handle);

        let template = ConfigTemplateBuilder::new().with_alpha_size(8).build();

        let display = unsafe { Display::new(display_handle).expect("Failed to create GL display") };

        let config = unsafe { display.find_configs(template) }
            .unwrap()
            .reduce(|config, acc| {
                if config.num_samples() > acc.num_samples() {
                    config
                } else {
                    acc
                }
            })
            .expect("No suitable EGL config found");

        let attrs = ContextAttributesBuilder::new().build(None);

        let not_current_ctx = unsafe {
            display
                .create_context(&config, &attrs)
                .expect("Failed to create EGL context")
        };

        let w_handle = WaylandWindowHandle::new(
            NonNull::<c_void>::new(surface_id.as_ptr() as *mut _).expect("Surface handle is null"),
        );

        let surface_handle = RawWindowHandle::Wayland(w_handle);

        let (width, height) = size;

        let surface_attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            surface_handle,
            NonZeroU32::new(width).unwrap(),
            NonZeroU32::new(height).unwrap(),
        );

        let surface = unsafe {
            display
                .create_window_surface(&config, &surface_attrs)
                .expect("Failed to create EGL surface")
        };

        let ctx = not_current_ctx
            .make_current(&surface)
            .expect("Failed to make EGL context current");

        Self { ctx, surface }
    }
}

unsafe impl OpenGLInterface for Context {
    fn ensure_current(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.ctx.is_current() {
            self.ctx.make_current(&self.surface)?;
        }

        Ok(())
    }

    fn swap_buffers(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.surface.swap_buffers(&self.ctx)?;
        Ok(())
    }

    fn resize(
        &self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.ensure_current()?;

        self.surface.resize(&self.ctx, width, height);
        Ok(())
    }

    fn get_proc_address(&self, addr: &CStr) -> *const c_void {
        self.ctx.display().get_proc_address(addr)
    }
}
