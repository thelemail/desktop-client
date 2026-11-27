mod appearance;
pub mod bridge;
#[cfg(debug_assertions)]
mod gallery;
#[cfg(debug_assertions)]
mod icon_gallery;
mod instance;
mod language;
#[cfg(debug_assertions)]
mod language_gallery;
mod logging;
mod menus;
#[cfg(debug_assertions)]
mod motion_gallery;
pub mod nav;
pub mod router;
mod screens;
pub mod services;
mod shell;
mod tray;
pub mod window;

pub use boot::run;

mod boot;
#[cfg(debug_assertions)]
mod button_gallery;
#[cfg(debug_assertions)]
mod choice_gallery;
#[cfg(debug_assertions)]
mod field_gallery;
