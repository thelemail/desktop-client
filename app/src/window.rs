use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Global, Task, TitlebarOptions, Window,
    WindowBounds, WindowOptions, open_window, point, px, size,
};
use serde::{Deserialize, Serialize};

use crate::shell::Shell;

pub const DEFAULT_SIZE: (f32, f32) = (1280.0, 840.0);
pub const MIN_SIZE: (f32, f32) = (960.0, 600.0);
const TITLE_GRIP: f32 = 20.0;
const SAVE_DELAY: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub maximized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

impl Frame {
    pub fn restorable(self, displays: &[Rect]) -> Option<Self> {
        let values = [self.x, self.y, self.width, self.height];
        if values.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let grip_x = self.x + self.width / 2.0;
        let grip_y = self.y + TITLE_GRIP.min(self.height / 2.0);
        if !displays.iter().any(|d| d.contains(grip_x, grip_y)) {
            return None;
        }
        Some(Self {
            width: self.width.max(MIN_SIZE.0),
            height: self.height.max(MIN_SIZE.1),
            ..self
        })
    }
}

fn state_path() -> PathBuf {
    thelemail_core::paths::data_root().join("window.json")
}

fn load() -> Option<Frame> {
    let bytes = std::fs::read(state_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save(frame: Frame) {
    let path = state_path();
    let result = serde_json::to_vec(&frame)
        .map_err(std::io::Error::other)
        .and_then(|bytes| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, &path)
        });
    if let Err(err) = result {
        tracing::warn!("window: saving the frame failed: {err}");
    }
}

fn frame_of(window: &Window) -> Frame {
    let (bounds, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(b) => (b, false),
        WindowBounds::Maximized(b) => (b, true),
        WindowBounds::Fullscreen(b) => (b, false),
    };
    Frame {
        x: bounds.origin.x.as_f32(),
        y: bounds.origin.y.as_f32(),
        width: bounds.size.width.as_f32(),
        height: bounds.size.height.as_f32(),
        maximized,
    }
}

fn initial_bounds(cx: &App) -> WindowBounds {
    let displays: Vec<Rect> = cx
        .displays()
        .iter()
        .map(|d| {
            let b = d.bounds();
            Rect {
                x: b.origin.x.as_f32(),
                y: b.origin.y.as_f32(),
                width: b.size.width.as_f32(),
                height: b.size.height.as_f32(),
            }
        })
        .collect();
    match load().and_then(|f| f.restorable(&displays)) {
        Some(frame) => {
            let bounds = Bounds::new(
                point(px(frame.x), px(frame.y)),
                size(px(frame.width), px(frame.height)),
            );
            if frame.maximized {
                WindowBounds::Maximized(bounds)
            } else {
                WindowBounds::Windowed(bounds)
            }
        }
        None => WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(DEFAULT_SIZE.0), px(DEFAULT_SIZE.1)),
            cx,
        )),
    }
}

#[derive(Default)]
struct MainWindow(Option<AnyWindowHandle>);

impl Global for MainWindow {}

pub fn init(cx: &mut App) {
    cx.set_global(MainWindow::default());
    cx.on_window_closed(|cx, id| {
        let main = cx.global_mut::<MainWindow>();
        if main.0.is_some_and(|handle| handle.window_id() == id) {
            main.0 = None;
        }
    })
    .detach();
}

pub fn show_main(cx: &mut App) {
    cx.activate(true);
    if let Some(handle) = cx.global::<MainWindow>().0 {
        let shown = handle.update(cx, |_, window, _| window.activate_window());
        if shown.is_ok() {
            return;
        }
    }
    open_main(cx);
}

pub fn open_main(cx: &mut App) {
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("Thelemail".into()),
            ..Default::default()
        }),
        window_bounds: Some(initial_bounds(cx)),
        window_min_size: Some(size(px(MIN_SIZE.0), px(MIN_SIZE.1))),
        app_id: Some("com.thelemail.desktop".into()),
        ..Default::default()
    };
    match open_window(options, cx, |window, cx| {
        cx.new(|cx| Shell::new(window, cx))
    }) {
        Ok((handle, _)) => {
            cx.global_mut::<MainWindow>().0 = Some(handle);
            let _ = handle.update(cx, |_, window, cx| {
                window.on_window_should_close(cx, |window, _| {
                    save(frame_of(window));
                    true
                });
            });
        }
        Err(err) => {
            tracing::error!("window: opening the main window failed: {err}");
            cx.quit();
        }
    }
}

#[derive(Default)]
pub struct FrameSaver {
    pending: Option<Task<()>>,
}

impl FrameSaver {
    pub fn changed<V: 'static>(&mut self, window: &Window, cx: &mut Context<V>) {
        let frame = frame_of(window);
        self.pending = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            cx.background_spawn(async move { save(frame) }).await;
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1512.0,
        height: 982.0,
    };
    const LEFT: Rect = Rect {
        x: -1920.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    };

    fn frame(x: f32, y: f32, width: f32, height: f32) -> Frame {
        Frame {
            x,
            y,
            width,
            height,
            maximized: false,
        }
    }

    #[test]
    fn a_frame_on_a_display_is_restored_as_saved() {
        let f = frame(100.0, 80.0, 1280.0, 840.0);
        assert_eq!(f.restorable(&[MAIN]), Some(f));
    }

    #[test]
    fn a_frame_on_a_second_display_is_kept_while_that_display_exists() {
        let f = frame(-1700.0, 60.0, 1280.0, 840.0);
        assert_eq!(f.restorable(&[MAIN, LEFT]), Some(f));
        assert_eq!(f.restorable(&[MAIN]), None);
    }

    #[test]
    fn a_frame_whose_title_bar_is_off_screen_is_dropped() {
        assert_eq!(
            frame(100.0, -500.0, 1280.0, 400.0).restorable(&[MAIN]),
            None
        );
        assert_eq!(
            frame(5000.0, 100.0, 1280.0, 840.0).restorable(&[MAIN]),
            None
        );
    }

    #[test]
    fn a_frame_below_the_minimum_grows_to_it() {
        let restored = frame(10.0, 10.0, 400.0, 300.0).restorable(&[MAIN]).unwrap();
        assert_eq!((restored.width, restored.height), MIN_SIZE);
    }

    #[test]
    fn corrupt_numbers_are_dropped() {
        assert_eq!(
            frame(f32::NAN, 0.0, 1280.0, 840.0).restorable(&[MAIN]),
            None
        );
        assert_eq!(
            frame(0.0, 0.0, f32::INFINITY, 840.0).restorable(&[MAIN]),
            None
        );
    }

    #[test]
    fn no_displays_means_nothing_to_restore() {
        assert_eq!(frame(0.0, 0.0, 1280.0, 840.0).restorable(&[]), None);
    }

    #[test]
    fn the_maximized_flag_survives_a_round_trip() {
        let f = Frame {
            maximized: true,
            ..frame(0.0, 25.0, 1512.0, 957.0)
        };
        let json = serde_json::to_vec(&f).unwrap();
        let back: Frame = serde_json::from_slice(&json).unwrap();
        assert_eq!(back, f);
        assert_eq!(back.restorable(&[MAIN]), Some(f));
    }
}
