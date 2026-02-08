use slint::{
    PlatformError,
    platform::{Platform, WindowAdapter},
};
use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use crate::adapter::MinimalFemtoVGWindow;

pub struct UmbrPlatform {
    window: Rc<MinimalFemtoVGWindow>,

    start_time: Instant,
}

impl UmbrPlatform {
    pub fn new(window: Rc<MinimalFemtoVGWindow>) -> Self {
        Self {
            window,
            start_time: Instant::now(),
        }
    }
}

impl Platform for UmbrPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        self.start_time.elapsed()
    }
}
