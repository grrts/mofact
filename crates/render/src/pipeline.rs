//! The GPU half of the renderer: one pipeline, one atlas texture per band,
//! and a growing instance buffer drawn with a single instanced draw call.

use crate::atlas::Atlas;
use crate::overlay::Overlay;
use crate::scene::{Camera, Instance, Scene};
use crate::Gpu;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

pub struct Renderer {
    atlases: Vec<Atlas>,
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    ground_layout: wgpu::PipelineLayout,
    /// The sprite and ground pipelines for each target format.
    pipelines: HashMap<wgpu::TextureFormat, (wgpu::RenderPipeline, wgpu::RenderPipeline)>,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    bands: Vec<wgpu::BindGroup>,
    instances: wgpu::Buffer,
    capacity: usize,
    atlas_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Globals for the overlay: a pixel-space transform.
    screen: wgpu::Buffer,
    screen_group: wgpu::BindGroup,
    /// The overlay atlas on the GPU and the atlas version it holds.
    ui: Option<(wgpu::Texture, wgpu::BindGroup, u64)>,
}

const ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Unorm8x4, 4 => Uint32
];

impl Renderer {
    /// Upload `atlases` (one per zoom band) and build the sprite pipeline.
    pub fn new(gpu: &Gpu, atlases: Vec<Atlas>) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas"),
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprites"),
            bind_group_layouts: &[Some(&globals_layout), Some(&atlas_layout)],
            ..Default::default()
        });
        let ground_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ground"),
            bind_group_layouts: &[Some(&globals_layout)],
            ..Default::default()
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let screen = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let screen_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("screen"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen.as_entire_binding(),
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bands = atlases
            .iter()
            .map(|atlas| {
                let texture = device.create_texture_with_data(
                    &gpu.queue,
                    &wgpu::TextureDescriptor {
                        label: Some("atlas"),
                        size: wgpu::Extent3d {
                            width: atlas.size,
                            height: atlas.size,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    &atlas.pixels,
                );
                let view = texture.create_view(&Default::default());
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("atlas"),
                    layout: &atlas_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                    ],
                })
            })
            .collect();
        let capacity = 1024;
        Self {
            atlas_layout,
            sampler,
            screen,
            screen_group,
            ui: None,
            atlases,
            instances: instance_buffer(device, capacity),
            capacity,
            shader,
            layout,
            ground_layout,
            pipelines: HashMap::new(),
            globals,
            globals_group,
            bands,
        }
    }

    /// The atlases the renderer was made with, for [`Scene::build`].
    pub fn atlases(&self) -> &[Atlas] {
        &self.atlases
    }

    fn pipeline(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        self.pipelines.entry(format).or_insert_with(|| {
            let ground = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("ground"),
                layout: Some(&self.ground_layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some("vs_ground"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some("fs_ground"),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
            let sprites = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sprites"),
                layout: Some(&self.layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &ATTRIBUTES,
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
            (sprites, ground)
        });
    }

    /// Clear `view` to `clear` and draw `scene` into it.
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        (width, height): (u32, u32),
        camera: &Camera,
        scene: &Scene,
        overlay: Option<&Overlay>,
        clear: wgpu::Color,
    ) {
        let n = scene.ore.len() + scene.instances.len();
        let ui = overlay.map_or(0, |o| o.instances.len());
        if n + ui > self.capacity {
            self.capacity = (n + ui).next_power_of_two();
            self.instances = instance_buffer(&gpu.device, self.capacity);
        }
        gpu.queue.write_buffer(
            &self.globals,
            0,
            bytemuck::cast_slice(&{
                let [a, b, c, d] = camera.transform(width, height);
                [
                    a,
                    b,
                    c,
                    d,
                    camera.pixels_per_tile,
                    width as f32,
                    height as f32,
                    0.0,
                ]
            }),
        );
        // Ore lies on the ground, under everything else.
        let ore = std::mem::size_of_val(scene.ore.as_slice()) as u64;
        if !scene.ore.is_empty() {
            gpu.queue
                .write_buffer(&self.instances, 0, bytemuck::cast_slice(&scene.ore));
        }
        if !scene.instances.is_empty() {
            gpu.queue
                .write_buffer(&self.instances, ore, bytemuck::cast_slice(&scene.instances));
        }
        if let Some(o) = overlay.filter(|_| ui > 0) {
            self.upload_overlay(gpu, o);
            let (w, h) = (width as f32, height as f32);
            gpu.queue.write_buffer(
                &self.screen,
                0,
                bytemuck::cast_slice(&[2.0 / w, -2.0 / h, -1.0, 1.0, 1.0, w, h, 0.0]),
            );
            let at = std::mem::size_of::<Instance>() as u64 * n as u64;
            gpu.queue
                .write_buffer(&self.instances, at, bytemuck::cast_slice(&o.instances));
        }
        self.pipeline(&gpu.device, format);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        let (sprites, ground) = &self.pipelines[&format];
        pass.set_bind_group(0, &self.globals_group, &[]);
        pass.set_pipeline(ground);
        pass.draw(0..3, 0..1);
        if n > 0 {
            pass.set_pipeline(sprites);
            pass.set_bind_group(1, &self.bands[scene.band], &[]);
            pass.set_vertex_buffer(0, self.instances.slice(..));
            pass.draw(0..6, 0..n as u32);
        }
        if let (Some((_, group, _)), true) = (&self.ui, ui > 0) {
            pass.set_pipeline(sprites);
            pass.set_bind_group(0, &self.screen_group, &[]);
            pass.set_bind_group(1, group, &[]);
            pass.set_vertex_buffer(0, self.instances.slice(..));
            pass.draw(0..6, n as u32..(n + ui) as u32);
        }
    }

    /// Copy the overlay atlas to the GPU when it has new glyphs.
    fn upload_overlay(&mut self, gpu: &Gpu, overlay: &Overlay) {
        let atlas = &overlay.atlas;
        if matches!(&self.ui, Some((_, _, v)) if *v == atlas.version) {
            return;
        }
        if self.ui.is_none() {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("overlay atlas"),
                size: wgpu::Extent3d {
                    width: atlas.size,
                    height: atlas.size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let group = atlas_group(&gpu.device, &self.atlas_layout, &self.sampler, &texture);
            self.ui = Some((texture, group, u64::MAX));
        }
        if let Some((texture, _, version)) = &mut self.ui {
            gpu.queue.write_texture(
                texture.as_image_copy(),
                &atlas.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(atlas.size * 4),
                    rows_per_image: Some(atlas.size),
                },
                texture.size(),
            );
            *version = atlas.version;
        }
    }

    /// Draw `scene` into an offscreen image and read it back as RGBA rows,
    /// for screenshots and golden-image tests.
    pub fn capture(
        &mut self,
        gpu: &Gpu,
        size: (u32, u32),
        camera: &Camera,
        scene: &Scene,
        overlay: Option<&Overlay>,
        clear: wgpu::Color,
    ) -> Vec<u8> {
        let (width, height) = size;
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture"),
            size: u64::from(row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let view = texture.create_view(&Default::default());
        self.encode(
            gpu,
            &mut encoder,
            &view,
            format,
            size,
            camera,
            scene,
            overlay,
            clear,
        );
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        gpu.queue.submit([encoder.finish()]);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.expect("map capture"));
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for capture");
        let mapped = buffer.slice(..).get_mapped_range().expect("read capture");
        mapped
            .chunks(row as usize)
            .flat_map(|r| &r[..(width * 4) as usize])
            .copied()
            .collect()
    }
}

fn atlas_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    texture: &wgpu::Texture,
) -> wgpu::BindGroup {
    let view = texture.create_view(&Default::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("atlas"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("instances"),
        contents: &vec![0u8; capacity * std::mem::size_of::<Instance>()],
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    })
}
