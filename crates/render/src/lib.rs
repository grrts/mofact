//! Render lane: wgpu renderer that draws the latest simulation snapshot with instanced batches, texture atlases per zoom band and smoothing between ticks.
//!
//! T03 brings up the GPU: [`Gpu`] owns the device, and a [`Target`] is the
//! window surface it presents to. The surface is separate because mobile
//! platforms take it away when the app goes to the background and give a new
//! one when it comes back; the device, and everything uploaded to it, stays.
//! Instanced batches, atlases and zoom bands arrive in T16.

pub use mofact_core as core;
pub use wgpu;

use mofact_artkit as _;
use std::fmt;

/// The factory floor colour the frame is cleared to before anything is drawn.
pub const FLOOR: wgpu::Color = wgpu::Color {
    r: 0.067,
    g: 0.078,
    b: 0.094,
    a: 1.0,
};

#[derive(Debug)]
pub enum GpuError {
    NoAdapter(wgpu::RequestAdapterError),
    NoDevice(wgpu::RequestDeviceError),
    Surface(wgpu::CreateSurfaceError),
    /// The adapter can't present to this surface at all.
    Unsupported,
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuError::NoAdapter(e) => write!(f, "no usable GPU: {e}"),
            GpuError::NoDevice(e) => write!(f, "could not open the GPU: {e}"),
            GpuError::Surface(e) => write!(f, "could not create the window surface: {e}"),
            GpuError::Unsupported => write!(f, "the GPU can't present to this window"),
        }
    }
}

impl std::error::Error for GpuError {}

impl From<wgpu::CreateSurfaceError> for GpuError {
    fn from(e: wgpu::CreateSurfaceError) -> Self {
        GpuError::Surface(e)
    }
}

/// The GPU device. Lives for the whole run, across surface loss.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// Open the GPU that can present to `surface`, created from `instance`.
    pub fn new(instance: wgpu::Instance, surface: &wgpu::Surface<'_>) -> Result<Self, GpuError> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(surface),
            ..Default::default()
        }))
        .map_err(GpuError::NoAdapter)?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("mofact"),
            // Phones and WebGL-class GPUs are the floor (R46).
            required_limits:
                wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(GpuError::NoDevice)?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    pub fn info(&self) -> wgpu::AdapterInfo {
        self.adapter.get_info()
    }
}

/// What happened when drawing a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    Presented,
    /// Nothing to draw into right now (minimised, occluded, or a timeout); try next frame.
    Skipped,
    /// The surface was out of date and has been reconfigured; try next frame.
    Reconfigured,
}

/// A window surface and its configuration.
pub struct Target {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

impl Target {
    /// Configure `surface` for `gpu` at `width` × `height` pixels.
    pub fn new(
        gpu: &Gpu,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self, GpuError> {
        let mut config = surface
            .get_default_config(&gpu.adapter, width.max(1), height.max(1))
            .ok_or(GpuError::Unsupported)?;
        // Vsync: the frame rate follows the display and the battery (R47).
        config.present_mode = wgpu::PresentMode::AutoVsync;
        config.desired_maximum_frame_latency = 2;
        surface.configure(&gpu.device, &config);
        Ok(Self { surface, config })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// Follow a window size change, in pixels.
    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) != self.size() {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&gpu.device, &self.config);
        }
    }

    /// Clear to `color` and present.
    pub fn draw(&mut self, gpu: &Gpu, color: wgpu::Color) -> Frame {
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                // Draw this one, then reconfigure for the next.
                present(gpu, t, color);
                self.surface.configure(&gpu.device, &self.config);
                return Frame::Reconfigured;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&gpu.device, &self.config);
                return Frame::Reconfigured;
            }
            _ => return Frame::Skipped,
        };
        present(gpu, texture, color);
        Frame::Presented
    }
}

fn present(gpu: &Gpu, texture: wgpu::SurfaceTexture, color: wgpu::Color) {
    let view = texture
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame"),
        });
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(color),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    gpu.queue.submit([encoder.finish()]);
    gpu.queue.present(texture);
}
