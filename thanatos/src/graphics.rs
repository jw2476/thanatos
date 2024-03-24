use crate::{
    assets::{self, MeshId},
    camera::Camera,
    event::Event,
    window::Window,
    world::{System, World},
};
use glam::{Mat4, Vec3};
use std::{borrow::Cow, rc::Rc};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: Vec3,
    pub colour: Vec3,
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array!(0 => Float32x3, 1 => Float32x3);

    pub const fn get_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

pub struct Context<'a> {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    surface: wgpu::Surface<'a>,
    pub device: wgpu::Device,
    queue: wgpu::Queue,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    render_pipeline: wgpu::RenderPipeline,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
}

impl<'a> Context<'a> {
    fn get_size(window: &winit::window::Window) -> winit::dpi::PhysicalSize<u32> {
        let mut size = window.inner_size();
        size.width = size.width.max(1);
        size.height = size.height.max(1);
        size
    }

    pub async fn new(window: &Window) -> Self {
        let size = Self::get_size(&window.window);

        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.window.clone()).unwrap();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_limits: wgpu::Limits::downlevel_defaults()
                        .using_resolution(adapter.limits()),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                "../assets/shaders/shader.wgsl"
            ))),
        });

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let swapchain_format = swapchain_capabilities.formats[0];

        let config = surface
            .get_default_config(&adapter, size.width, size.height)
            .unwrap();
        surface.configure(&device, &config);

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            contents: bytemuck::cast_slice(&Mat4::IDENTITY.to_cols_array()),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
            label: None,
        });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: None,
            });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
            label: None,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&camera_bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::get_layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(swapchain_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        Self {
            instance,
            adapter,
            surface,
            device,
            pipeline_layout,
            queue,
            render_pipeline,
            shader,
            config,
            size,
            camera_buffer,
            camera_bind_group,
        }
    }
}

pub fn resize_surface(world: &mut World, event: &Event) {
    let mut ctx = world.get_mut::<Context>().unwrap();

    match event {
        Event::Resized(new_size) => {
            ctx.config.width = new_size.width.max(1);
            ctx.config.height = new_size.height.max(1);
            ctx.surface.configure(&ctx.device, &ctx.config);
        }
        _ => (),
    }
}

pub struct RenderObject {
    pub mesh: MeshId,
}

pub fn draw(world: &mut World) {
    let camera = world.get::<Camera>().unwrap();
    let ctx = world.get::<Context>().unwrap();
    let assets = world.get::<assets::Manager>().unwrap();
    let objects = world.get_components::<RenderObject>();

    ctx.queue.write_buffer(
        &ctx.camera_buffer,
        0,
        bytemuck::cast_slice(&camera.get_matrix().to_cols_array()),
    );

    let frame = ctx
        .surface
        .get_current_texture()
        .expect("Failed to acquire next swap chain texture");
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rpass.set_pipeline(&ctx.render_pipeline);
        rpass.set_bind_group(0, &ctx.camera_bind_group, &[]);
        objects.into_iter().for_each(|object| {
            let mesh = assets.get_mesh(object.mesh).unwrap();
            rpass.set_vertex_buffer(0, mesh.vertices.slice(..));
            rpass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            rpass.draw_indexed(0..mesh.num_indices, 0, 0..1);
        })
    }

    ctx.queue.submit(Some(encoder.finish()));
    frame.present();
}
