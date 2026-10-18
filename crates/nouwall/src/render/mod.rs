//! Wallpaper renderers. Each host window owns one renderer; the app loop calls
//! `frame` whenever the renderer's deadline passes.

use std::time::Instant;

use nouwall_core::Kind;

pub mod shader;
pub mod solid;
pub mod video;
pub mod web;

pub trait Renderer {
    fn kind(&self) -> Option<Kind>;
    fn resize(&mut self, width: u32, height: u32);
    fn set_paused(&mut self, paused: bool);
    /// Render if due. Returns the next time `frame` should be called, or `None`
    /// when the renderer drives itself (web) or is paused.
    fn frame(&mut self, now: Instant) -> Option<Instant>;
    /// Short human-readable status for the settings window.
    fn status(&self) -> String;
    /// Apply audio settings (video only).
    fn set_audio(&mut self, _mute: bool, _volume: f32) {}
}

/// Options every renderer is created with.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub fps: u32,
    pub render_scale: f32,
    pub auto_downgrade: bool,
    pub mute: bool,
    pub volume: f32,
    pub monitor_x: i32,
    pub monitor_y: i32,
}
