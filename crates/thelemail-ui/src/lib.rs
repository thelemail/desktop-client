pub mod a11y;
pub mod badge;
pub mod button;
pub mod choice;
pub mod empty_state;
pub mod feedback;
pub mod field;
pub mod fonts;
pub mod icons;
pub mod load_state;
pub mod motion;
pub mod overlay;
pub mod rich;
pub mod scroll_area;
pub mod separator;
pub mod shape;
pub mod skeleton;
pub mod spinner;
pub mod state;
pub mod text;
pub mod theme;
pub mod tokens;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../build/fa.rs"]
mod build_fa;
