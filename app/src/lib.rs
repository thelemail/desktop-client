mod appearance;
pub mod bridge;
#[cfg(debug_assertions)]
mod gallery;
mod instance;
mod logging;
mod menus;
pub mod nav;
pub mod router;
mod screens;
pub mod services;
mod shell;
mod tray;
pub mod window;

pub use boot::run;

mod boot;
