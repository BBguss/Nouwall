//! Frame-time governor: when frames keep running late, lower the render scale
//! first and then the frame rate. It never steps back up on its own; a new
//! wallpaper or settings change creates a fresh governor.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quality {
    pub fps: u32,
    pub render_scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change {
    RenderScale { from: f32, to: f32 },
    Fps { from: u32, to: u32 },
}

#[derive(Debug, Clone)]
pub struct Governor {
    quality: Quality,
    window: Duration,
    elapsed: Duration,
    late: u32,
    frames: u32,
    pub min_scale: f32,
    pub min_fps: u32,
}

impl Governor {
    pub fn new(fps: u32, render_scale: f32) -> Self {
        Self {
            quality: Quality { fps: fps.max(1), render_scale },
            window: Duration::from_secs(3),
            elapsed: Duration::ZERO,
            late: 0,
            frames: 0,
            min_scale: 0.5,
            min_fps: 15,
        }
    }

    pub fn quality(&self) -> Quality {
        self.quality
    }

    pub fn frame_budget(&self) -> Duration {
        Duration::from_secs_f64(1.0 / f64::from(self.quality.fps))
    }

    /// Record how long rendering one frame took (CPU submit + GPU present).
    /// Returns a change when the last window had too many late frames.
    pub fn record(&mut self, frame_time: Duration) -> Option<Change> {
        let budget = self.frame_budget();
        self.frames += 1;
        if frame_time.as_secs_f64() > budget.as_secs_f64() * 1.5 {
            self.late += 1;
        }
        self.elapsed += budget.max(frame_time);
        if self.elapsed < self.window {
            return None;
        }
        let late_ratio = f64::from(self.late) / f64::from(self.frames.max(1));
        self.elapsed = Duration::ZERO;
        self.late = 0;
        self.frames = 0;
        if late_ratio < 0.5 {
            return None;
        }
        self.step_down()
    }

    fn step_down(&mut self) -> Option<Change> {
        let q = &mut self.quality;
        if q.render_scale > self.min_scale + f32::EPSILON {
            let from = q.render_scale;
            q.render_scale = (q.render_scale - 0.25).max(self.min_scale);
            return Some(Change::RenderScale { from, to: q.render_scale });
        }
        if q.fps > self.min_fps {
            let from = q.fps;
            q.fps = (q.fps / 2).max(self.min_fps);
            return Some(Change::Fps { from, to: q.fps });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(g: &mut Governor, ms: u64, n: usize) -> Vec<Change> {
        (0..n).filter_map(|_| g.record(Duration::from_millis(ms))).collect()
    }

    #[test]
    fn fast_frames_never_change() {
        let mut g = Governor::new(60, 1.0);
        assert!(run(&mut g, 5, 1000).is_empty());
    }

    #[test]
    fn slow_frames_drop_scale_then_fps() {
        let mut g = Governor::new(60, 1.0);
        let changes = run(&mut g, 50, 400);
        assert_eq!(changes[0], Change::RenderScale { from: 1.0, to: 0.75 });
        assert_eq!(changes[1], Change::RenderScale { from: 0.75, to: 0.5 });
        assert_eq!(changes[2], Change::Fps { from: 60, to: 30 });
        assert_eq!(g.quality(), Quality { fps: 15, render_scale: 0.5 });
    }

    #[test]
    fn occasional_spikes_are_tolerated() {
        let mut g = Governor::new(30, 1.0);
        let changes: Vec<_> = (0..600)
            .filter_map(|i| g.record(Duration::from_millis(if i % 10 == 0 { 100 } else { 10 })))
            .collect();
        assert!(changes.is_empty());
    }
}
