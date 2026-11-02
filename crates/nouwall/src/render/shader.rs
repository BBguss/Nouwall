//! Shader wallpapers on wgpu.
//!
//! GLSL files use the Shadertoy convention (`void mainImage(out vec4 fragColor,
//! in vec2 fragCoord)` with `iResolution`, `iTime`, `iTimeDelta`, `iFrame`,
//! `iMouse`, `iDate`). WGSL files define `fn main_image(frag_coord: vec2<f32>) -> vec4<f32>`
//! and read the same values from the `u` uniform.
//!
//! Frames are rendered at the governor's render scale into an offscreen
//! texture, read back and shown through a blt-model swap chain (see `blt`)
//! that DXGI stretches to the window. wgpu itself never presents to the
//! window: its swap chains are flip-model, which DWM may put on an overlay
//! plane that makes the taskbar blink.

use std::rc::Rc;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use nouwall_core::governor::Change;
use nouwall_core::{tr, Governor, Kind};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use super::blt::Presenter;
use super::{RenderOptions, Renderer};
use crate::util::Hwnd;

/// Matches the presenter's `DXGI_FORMAT_R8G8B8A8_UNORM` swap chain.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// One wgpu device shared by every shader host.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub fn new(probe_surface_hwnd: Hwnd) -> Result<Rc<Gpu>, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN | wgpu::Backends::GL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance.create_surface(probe_surface_hwnd).map_err(|e| e.to_string())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .map_err(|e| e.to_string())?;
        let info = adapter.get_info();
        log::info!("wgpu adapter: {} ({:?})", info.name, info.backend);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("nouwall"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| log::error!("wgpu: {e}")));
        drop(surface);
        Ok(Rc::new(Gpu { instance, adapter, device, queue }))
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    resolution: [f32; 3],
    time: f32,
    mouse: [f32; 4],
    time_delta: f32,
    frame: i32,
    _pad: [f32; 2],
    date: [f32; 4],
}

const VERTEX_WGSL: &str = r#"
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;

const GLSL_PRELUDE: &str = "#version 450
layout(std140, set = 0, binding = 0) uniform NouwallUniforms {
    vec3 iResolution;
    float iTime;
    vec4 iMouse;
    float iTimeDelta;
    int iFrame;
    vec2 nouwall_pad;
    vec4 iDate;
};
layout(location = 0) out vec4 nouwall_frag_color;
#define iChannelTime float[4](iTime, iTime, iTime, iTime)
#define iSampleRate 44100.0
";

const GLSL_EPILOGUE: &str = "
void main() {
    vec2 nouwall_coord = vec2(gl_FragCoord.x, iResolution.y - gl_FragCoord.y);
    vec4 nouwall_c = vec4(0.0, 0.0, 0.0, 1.0);
    mainImage(nouwall_c, nouwall_coord);
    nouwall_frag_color = vec4(nouwall_c.rgb, 1.0);
}
";

const WGSL_PRELUDE: &str = "
struct NouwallUniforms {
    resolution: vec3<f32>,
    time: f32,
    mouse: vec4<f32>,
    time_delta: f32,
    frame: i32,
    _pad: vec2<f32>,
    date: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: NouwallUniforms;
";

const WGSL_EPILOGUE: &str = "
@fragment
fn nouwall_fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = main_image(vec2<f32>(pos.x, u.resolution.y - pos.y));
    return vec4<f32>(c.rgb, 1.0);
}
";

pub enum Language {
    Glsl,
    Wgsl,
}

impl Language {
    pub fn from_path(path: &std::path::Path) -> Language {
        match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("wgsl") => Language::Wgsl,
            _ => Language::Glsl,
        }
    }
}

/// Compile the fragment shader inside an error scope so a broken shader
/// becomes an error message instead of a device-lost.
fn fragment_module(device: &wgpu::Device, lang: &Language, source: &str) -> Result<(wgpu::ShaderModule, &'static str), String> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let (module, entry) = match lang {
        Language::Glsl => {
            let code = format!("{GLSL_PRELUDE}\n{source}\n{GLSL_EPILOGUE}");
            let m = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("wallpaper.glsl"),
                source: wgpu::ShaderSource::Glsl {
                    shader: code.into(),
                    stage: wgpu::naga::ShaderStage::Fragment,
                    defines: Default::default(),
                },
            });
            (m, "main")
        }
        Language::Wgsl => {
            let code = format!("{WGSL_PRELUDE}\n{source}\n{WGSL_EPILOGUE}");
            let m = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("wallpaper.wgsl"),
                source: wgpu::ShaderSource::Wgsl(code.into()),
            });
            (m, "nouwall_fs")
        }
    };
    match pollster::block_on(scope.pop()) {
        Some(e) => Err(e.to_string()),
        None => Ok((module, entry)),
    }
}

/// Render target at the current render scale plus its read-back buffer.
struct Offscreen {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    /// Bytes per row in `readback`, padded to wgpu's copy alignment.
    row_pitch: u32,
    width: u32,
    height: u32,
}

pub struct ShaderRenderer {
    gpu: Rc<Gpu>,
    presenter: Presenter,
    /// Window (monitor) size.
    size: (u32, u32),
    pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    uniform_bind: wgpu::BindGroup,
    offscreen: Option<Offscreen>,
    governor: Governor,
    auto_downgrade: bool,
    start: Instant,
    last: Instant,
    next: Instant,
    paused_at: Option<Instant>,
    paused_total: Duration,
    frame: i32,
    monitor_origin: (i32, i32),
    last_status: String,
}

impl ShaderRenderer {
    pub fn new(
        gpu: Rc<Gpu>,
        hwnd: Hwnd,
        size: (u32, u32),
        lang: Language,
        source: &str,
        opts: RenderOptions,
    ) -> Result<Self, String> {
        let device = &gpu.device;
        let size = (size.0.max(1), size.1.max(1));
        let presenter = Presenter::new(hwnd.0, size).map_err(|e| e.to_string())?;

        let (fragment, entry) = fragment_module(device, &lang, source)?;
        let vertex = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fullscreen"),
            source: wgpu::ShaderSource::Wgsl(VERTEX_WGSL.into()),
        });

        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniforms"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform_buf.as_entire_binding() }],
        });

        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wallpaper"),
            bind_group_layouts: &[Some(&uniform_layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wallpaper"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &vertex,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &fragment,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &[Some(FORMAT.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(e.to_string());
        }

        let now = Instant::now();
        let mut r = Self {
            gpu: gpu.clone(),
            presenter,
            size,
            pipeline,
            uniform_buf,
            uniform_bind,
            offscreen: None,
            governor: Governor::new(opts.fps, opts.render_scale),
            auto_downgrade: opts.auto_downgrade,
            start: now,
            last: now,
            next: now,
            paused_at: None,
            paused_total: Duration::ZERO,
            frame: 0,
            monitor_origin: (opts.monitor_x, opts.monitor_y),
            last_status: String::new(),
        };
        r.rebuild_offscreen();
        Ok(r)
    }

    fn rebuild_offscreen(&mut self) {
        let scale = self.governor.quality().render_scale.clamp(0.25, 1.0);
        let width = ((self.size.0 as f32 * scale) as u32).max(1);
        let height = ((self.size.1 as f32 * scale) as u32).max(1);
        if self.offscreen.as_ref().is_some_and(|o| o.width == width && o.height == height) {
            return;
        }
        let device = &self.gpu.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let row_pitch = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(row_pitch) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        if let Err(e) = self.presenter.resize((width, height)) {
            log::warn!("shader resize: {e}");
        }
        self.offscreen = Some(Offscreen { texture, view, readback, row_pitch, width, height });
    }

    fn uniforms(&self, now: Instant, width: u32, height: u32) -> Uniforms {
        let time = (now - self.start).saturating_sub(self.paused_total).as_secs_f32();
        let delta = (now - self.last).as_secs_f32();
        let scale = width as f32 / self.size.0 as f32;
        let mut p = POINT::default();
        let (mx, my) = if unsafe { GetCursorPos(&mut p) }.is_ok() {
            let x = (p.x - self.monitor_origin.0) as f32 * scale;
            let y = height as f32 - (p.y - self.monitor_origin.1) as f32 * scale;
            (x, y)
        } else {
            (0.0, 0.0)
        };
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        Uniforms {
            resolution: [width as f32, height as f32, 1.0],
            time,
            mouse: [mx, my, 0.0, 0.0],
            time_delta: delta,
            frame: self.frame,
            _pad: [0.0; 2],
            date: [2026.0, 0.0, 0.0, (secs % 86400.0) as f32],
        }
    }

    fn render(&mut self, now: Instant) -> Result<(), String> {
        let o = self.offscreen.as_ref().ok_or("no render target")?;
        let u = self.uniforms(now, o.width, o.height);
        self.gpu.queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&u));

        let mut enc = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wallpaper"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &o.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.uniform_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        enc.copy_texture_to_buffer(
            o.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &o.readback,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(o.row_pitch), rows_per_image: None },
            },
            wgpu::Extent3d { width: o.width, height: o.height, depth_or_array_layers: 1 },
        );
        self.gpu.queue.submit([enc.finish()]);

        let slice = o.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(Duration::from_secs(1)) });
        match rx.try_recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(format!("readback: {e}")),
            Err(_) => return Err("readback timed out".into()),
        }
        let shown = {
            let pixels = slice.get_mapped_range();
            self.presenter.present(&pixels, o.row_pitch)
        };
        o.readback.unmap();
        shown.map_err(|e| e.to_string())?;
        self.frame = self.frame.wrapping_add(1);
        self.last = now;
        Ok(())
    }
}

impl Renderer for ShaderRenderer {
    fn kind(&self) -> Option<Kind> {
        Some(Kind::Shader)
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.size = (width.max(1), height.max(1));
        self.offscreen = None;
        self.rebuild_offscreen();
    }

    fn set_paused(&mut self, paused: bool) {
        let now = Instant::now();
        match (paused, self.paused_at) {
            (true, None) => self.paused_at = Some(now),
            (false, Some(at)) => {
                self.paused_total += now - at;
                self.paused_at = None;
                self.last = now;
                self.next = now;
            }
            _ => {}
        }
    }

    fn frame(&mut self, now: Instant) -> Option<Instant> {
        if self.paused_at.is_some() {
            return None;
        }
        if now < self.next {
            return Some(self.next);
        }
        let started = Instant::now();
        if let Err(e) = self.render(now) {
            log::warn!("shader frame: {e}");
        }
        let took = started.elapsed();
        if self.auto_downgrade {
            if let Some(change) = self.governor.record(took) {
                match change {
                    Change::RenderScale { from, to } => {
                        log::info!("governor: render scale {:.0}% -> {:.0}%", from * 100.0, to * 100.0);
                        self.rebuild_offscreen();
                    }
                    Change::Fps { from, to } => log::info!("governor: fps {from} -> {to}"),
                }
            }
        }
        let q = self.governor.quality();
        self.last_status = tr!("{} fps, scale {:.0}%", "{} fps, skala {:.0}%", q.fps, q.render_scale * 100.0);
        // Schedule from the previous deadline to keep a steady cadence.
        let budget = self.governor.frame_budget();
        self.next = (self.next + budget).max(now + budget / 4);
        Some(self.next)
    }

    fn status(&self) -> String {
        if self.paused_at.is_some() {
            tr!("paused", "dijeda")
        } else {
            self.last_status.clone()
        }
    }
}
