//! M3G (JSR-184) loader, animation and renderer, as a library.
//!
//! The `m3g-viewer` binary is a thin window around this crate. Nothing here
//! needs a window except [`render`] and [`gfx`], which draw through miniquad.

pub mod anim;
pub mod camera;
pub mod geometry;
pub mod gfx;
pub mod lighting;
pub mod loader;
pub mod math;
pub mod pipe;
pub mod render;
pub mod scene;
