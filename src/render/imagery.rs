//! Pictures laid over the ground of single tiles (satellite imagery, the user's own
//! pictures; `src/geo`): one texture each, with its mipmaps, and how strongly it covers the
//! ground. The terrain shader takes it as the tile's binding 5 and the opacity in
//! `TileUniform::flags[1]`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::doc::TileKey;
use crate::gpu::Gpu;

/// A picture and its smaller copies, largest first (RGBA, row 0 the tile's north edge).
pub struct Mips(pub Vec<image::RgbaImage>);

impl Mips {
    /// The picture at most `max` pixels a side, with every smaller copy down to 1 pixel.
    pub fn of(img: image::RgbaImage, max: u32) -> Mips {
        let mut top = img;
        if top.width() > max || top.height() > max {
            top = image::imageops::resize(&top, max.min(top.width()), max.min(top.height()), image::imageops::FilterType::Triangle);
        }
        let mut levels = vec![top];
        loop {
            let l = levels.last().unwrap();
            if l.width() <= 1 && l.height() <= 1 {
                break;
            }
            let (w, h) = ((l.width() / 2).max(1), (l.height() / 2).max(1));
            let next = image::imageops::resize(l, w, h, image::imageops::FilterType::Triangle);
            levels.push(next);
        }
        Mips(levels)
    }
}

struct Entry {
    view: wgpu::TextureView,
    rev: u64,
    opacity: f32,
}

static REV: AtomicU64 = AtomicU64::new(1);

pub struct Imagery {
    tiles: HashMap<TileKey, Entry>,
    /// For tiles without a picture (clear).
    none: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    /// Different for every renderer: a new one (another graphics quality) has none of the
    /// pictures, and whoever gave them gives them again.
    pub epoch: u64,
}

impl Imagery {
    pub fn new(gpu: &Gpu) -> Imagery {
        let none = super::solid_texture(&gpu.device, &gpu.queue, [0, 0, 0, 0], true);
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("imagery"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        Imagery { tiles: HashMap::new(), none, sampler, epoch: REV.fetch_add(1, Ordering::Relaxed) }
    }

    /// Put up (or replace) a tile's picture.
    pub fn upload(&mut self, gpu: &Gpu, k: TileKey, mips: &Mips, opacity: f32) {
        let top = &mips.0[0];
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("imagery"),
            size: wgpu::Extent3d { width: top.width(), height: top.height(), depth_or_array_layers: 1 },
            mip_level_count: mips.0.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, img) in mips.0.iter().enumerate() {
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                img.as_raw(),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(img.width() * 4), rows_per_image: Some(img.height()) },
                wgpu::Extent3d { width: img.width(), height: img.height(), depth_or_array_layers: 1 },
            );
        }
        let view = tex.create_view(&Default::default());
        self.tiles.insert(k, Entry { view, rev: REV.fetch_add(1, Ordering::Relaxed), opacity });
    }

    pub fn remove(&mut self, k: TileKey) {
        self.tiles.remove(&k);
    }

    pub fn clear(&mut self) {
        self.tiles.clear();
    }

    pub fn set_opacity(&mut self, k: TileKey, opacity: f32) {
        if let Some(e) = self.tiles.get_mut(&k) {
            e.opacity = opacity;
        }
    }

    /// The tile's picture and its revision (0: none, the clear one).
    pub(super) fn view(&self, k: TileKey) -> (&wgpu::TextureView, u64) {
        match self.tiles.get(&k) {
            Some(e) => (&e.view, e.rev),
            None => (&self.none, 0),
        }
    }

    pub(super) fn opacity(&self, k: TileKey) -> f32 {
        self.tiles.get(&k).map(|e| e.opacity).unwrap_or(0.0)
    }
}
