//! The window's GPU: wgpu (openOMSI's fork) on Metal, Vulkan, Direct3D 12 or GL.

use std::sync::Arc;

use winit::window::Window;

pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub struct Gpu {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub adapter_name: String,
    /// The format the 3D view is drawn in (the sRGB view of the surface; the interface
    /// draws into the plain one, as egui wants).
    pub scene_format: wgpu::TextureFormat,
    pub depth: wgpu::TextureView,
    pub msaa: wgpu::TextureView,
    /// Samples a pixel of the 3D view (1: no edge smoothing, drawn straight into the frame).
    pub samples: u32,
}

impl Gpu {
    pub fn new(window: Arc<Window>) -> anyhow::Result<Gpu> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::PRIMARY | wgpu::Backends::GL;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))?;
        let info = adapter.get_info();
        log::info!("GPU: {} ({:?})", info.name, info.backend);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("openMapper"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))?;
        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]).remove_srgb_suffix();
        let scene_format = format.add_srgb_suffix();
        // (COPY_SRC: the screenshot mode reads the picture back)
        let copy = if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) { wgpu::TextureUsages::COPY_SRC } else { wgpu::TextureUsages::empty() };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | copy,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            // (profiling: as fast as it can, not held to the screen)
            present_mode: if crate::render::profile::enabled() { wgpu::PresentMode::AutoNoVsync } else { wgpu::PresentMode::AutoVsync },
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: if scene_format != format { vec![scene_format] } else { vec![] },
        };
        surface.configure(&device, &config);
        let (depth, msaa) = targets(&device, &config, 4);
        Ok(Gpu { surface, device, queue, config, scene_format, adapter_name: format!("{} ({:?})", info.name, info.backend), depth, msaa, samples: 4 })
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        let (d, m) = targets(&self.device, &self.config, self.samples);
        self.depth = d;
        self.msaa = m;
    }

    /// Change the samples a pixel (the renderer is made again after it).
    pub fn set_samples(&mut self, n: u32) {
        self.samples = n;
        let (d, m) = targets(&self.device, &self.config, n);
        self.depth = d;
        self.msaa = m;
    }
}

fn targets(device: &wgpu::Device, c: &wgpu::SurfaceConfiguration, samples: u32) -> (wgpu::TextureView, wgpu::TextureView) {
    let size = wgpu::Extent3d { width: c.width, height: c.height, depth_or_array_layers: 1 };
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size,
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let msaa = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("msaa"),
        size,
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format: c.format.add_srgb_suffix(),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    (depth.create_view(&Default::default()), msaa.create_view(&Default::default()))
}
