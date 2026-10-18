//! Shader wallpapers on wgpu.
//!
//! GLSL files use the Shadertoy convention (`void mainImage(out vec4 fragColor,
//! in vec2 fragCoord)` with `iResolution`, `iTime`, `iTimeDelta`, `iFrame`,
//! `iMouse`, `iDate`). WGSL files define `fn main_image(frag_coord: vec2<f32>) -> vec4<f32>`
//! and read the same values from the `u` uniform.

use std::rc::Rc;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use nouwall_core::governor::Change;
use nouwall_core::{tr, Governor, Kind};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use super::{RenderOptions, Renderer};
use crate::util::Hwnd;

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

const BLIT_WGSL: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct V { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> V {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: V;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

@fragment
fn fs_main(v: V) -> @location(0) vec4<f32> {
    return textureSample(src, samp, v.uv);
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

struct Offscreen {
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
    width: u32,
    height: u32,
}

pub struct ShaderRenderer {
    gpu: Rc<Gpu>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    uniform_bind: wgpu::BindGroup,
    blit_pipeline: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
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
        let surface = gpu.instance.create_surface(hwnd).map_err(|e| e.to_string())?;
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or("surface has no formats")?;
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.0.max(1),
            height: size.1.max(1),
            present_mode,
            desired_maximum_frame_latency: 1,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: Default::default(),
        };
        surface.configure(device, &config);

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
                targets: &[Some(format.into())],
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

        let blit_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit"),
            source: wgpu::ShaderSource::Wgsl(BLIT_WGSL.into()),
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blit_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit"),
            bind_group_layouts: &[Some(&blit_layout)],
            ..Default::default()
        });
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&blit_pl),
            vertex: wgpu::VertexState {
                module: &blit_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &blit_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let now = Instant::now();
        let mut r = Self {
            gpu: gpu.clone(),
            surface,
            config,
            pipeline,
            uniform_buf,
            uniform_bind,
            blit_pipeline,
            blit_layout,
            sampler,
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
        if scale >= 0.999 {
            self.offscreen = None;
            return;
        }
        let width = ((self.config.width as f32 * scale) as u32).max(1);
        let height = ((self.config.height as f32 * scale) as u32).max(1);
        if self.offscreen.as_ref().is_some_and(|o| o.width == width && o.height == height) {
            return;
        }
        let device = &self.gpu.device;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit"),
            layout: &self.blit_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.offscreen = Some(Offscreen { view, bind, width, height });
    }

    fn uniforms(&self, now: Instant, width: u32, height: u32) -> Uniforms {
        let time = (now - self.start).saturating_sub(self.paused_total).as_secs_f32();
        let delta = (now - self.last).as_secs_f32();
        let scale = width as f32 / self.config.width.max(1) as f32;
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
        let surface_tex = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.gpu.device, &self.config);
                return Ok(());
            }
            other => return Err(format!("surface: {other:?}")),
        };
        let target = surface_tex.texture.create_view(&Default::default());
        let (w, h) = match &self.offscreen {
            Some(o) => (o.width, o.height),
            None => (self.config.width, self.config.height),
        };
        let u = self.uniforms(now, w, h);
        self.gpu.queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&u));

        let mut enc = self.gpu.device.create_command_encoder(&Default::default());
        {
            let view = self.offscreen.as_ref().map(|o| &o.view).unwrap_or(&target);
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wallpaper"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
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
        if let Some(o) = &self.offscreen {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &o.bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.gpu.queue.submit([enc.finish()]);
        self.gpu.queue.present(surface_tex);
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
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.gpu.device, &self.config);
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
