pub mod components;

pub use fontdue::{Font, FontSettings};

use std::{collections::HashMap, mem::size_of, rc::Rc};

use anyhow::Result;
use etagere::Size;
use fontdue::layout::TextStyle;
use glam::{Vec2, Vec4};
use hephaestus::{
    buffer::{Dynamic, Static},
    command::{self, BufferToImageRegion, TransitionLayout},
    descriptor,
    image::{Image, ImageInfo, ImageView, Sampler},
    pipeline::{Graphics, ImageLayout, RenderPass, ShaderModule, Viewport},
    task::Task,
    vertex::{self, AttributeType},
    AccessFlags, BufferUsageFlags, Context, DescriptorType, Extent2D, Extent3D, Format,
    ImageAspectFlags, ImageUsageFlags, Offset3D, PipelineStageFlags, SampleCountFlags,
};

#[derive(Debug, Clone)]
pub enum Event {
    Click(Vec2),
    RightClick(Vec2)
}

#[derive(Clone, Copy, Debug)]
pub struct Signal(usize);

#[derive(Default, Clone, Debug)]
pub struct Signals(Vec<bool>);

impl Signals {
    pub fn signal(&mut self) -> Signal {
        self.0.push(false);
        Signal(self.0.len() - 1)
    }

    pub fn get(&self, signal: Signal) -> bool {
        self.0.get(signal.0).copied().unwrap_or_default()
    }

    pub fn set(&mut self, signal: Signal) {
        self.0.get_mut(signal.0).map(|x| *x = true);
    }

    pub fn clear(&mut self) {
        self.0.iter_mut().for_each(|x| *x = false);
    }
}

pub fn clicked(events: &[Event], area: Area) -> bool {
    events
        .iter()
        .filter_map(|event| {
            if let Event::Click(position) = event {
                Some(*position)
            } else {
                None
            }
        })
        .any(|position| area.contains(position))
}

pub fn right_clicked(events: &[Event], area: Area) -> bool {
    events
        .iter()
        .filter_map(|event| {
            if let Event::RightClick(position) = event {
                Some(*position)
            } else {
                None
            }
        })
        .any(|position| area.contains(position))
}


#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constraint<T> {
    pub min: T,
    pub max: T,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub origin: Vec2,
    pub size: Vec2,
}

impl Area {
    pub fn points(&self) -> [Vec2; 4] {
        [
            self.origin,
            self.origin + Vec2::X * self.size.x,
            self.origin + self.size,
            self.origin + Vec2::Y * self.size.y,
        ]
    }

    pub fn as_vec4(&self) -> Vec4 {
        Vec4::new(self.origin.x, self.origin.y, self.size.x, self.size.y)
    }

    pub fn vertices(areas: &[Area]) -> (Vec<Vec2>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for area in areas {
            indices.append(
                &mut [0, 1, 2, 2, 3, 0]
                    .into_iter()
                    .map(|x| x + vertices.len() as u32)
                    .collect(),
            );

            vertices.extend_from_slice(
                &area
                    .points()
                    .into_iter()
                    .map(|point| Vec2::new(point.x, point.y))
                    .collect::<Vec<_>>(),
            );
        }

        (vertices, indices)
    }

    pub fn contains(&self, point: Vec2) -> bool {
        self.origin.x < point.x
            && point.x < self.origin.x + self.size.x
            && self.origin.y < point.y
            && point.y < self.origin.y + self.size.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rectangle {
    area: Area,
    radius: f32,
    colour: Vec4,
}

pub struct Text {
    pub origin: Vec2,
    pub text: String,
    pub font_size: f32,
    pub font: Rc<Font>,
    pub colour: Vec4,
}

impl Text {
    pub fn get_size(&self) -> Vec2 {
        Vec2::ZERO
    }
}

#[derive(Default)]
pub struct Layer {
    rectangles: Vec<Rectangle>,
    text: Vec<Text>,
}

impl Layer {
    pub fn is_empty(&self) -> bool {
        self.rectangles.is_empty() && self.text.is_empty()
    }
}

pub struct Scene {
    layers: Vec<Layer>,
}

pub struct RenderedScene {
    vertices: Vec<Vec2>,
    indices: Vec<u32>,
    rectangles: Vec<RectangleData>,
    image: (Size, Vec<u8>),
}

impl Scene {
    pub fn new() -> Self {
        Self {
            layers: vec![Layer::default()],
        }
    }

    pub fn rectangle(&mut self, rectangle: Rectangle) {
        self.layers.last_mut().unwrap().rectangles.push(rectangle)
    }

    pub fn text(&mut self, text: Text) {
        self.layers.last_mut().unwrap().text.push(text)
    }

    pub fn layer(&mut self) {
        self.layers.push(Layer::default())
    }

    pub fn render(&self) -> Result<RenderedScene> {
        let (mut vertices, mut indices, mut rectangles) = self.render_rectangles();
        let (mut text_vertices, text_indices, mut text_rectangles, image) = self.render_text()?;
        indices.append(
            &mut text_indices
                .into_iter()
                .map(|index| index + vertices.len() as u32)
                .collect(),
        );
        vertices.append(&mut text_vertices);
        rectangles.append(&mut text_rectangles);

        Ok(RenderedScene {
            vertices,
            indices,
            rectangles,
            image,
        })
    }

    fn render_rectangles(&self) -> (Vec<Vec2>, Vec<u32>, Vec<RectangleData>) {
        let (vertices, indices) = Area::vertices(
            &self
                .layers
                .iter()
                .flat_map(|layer| &layer.rectangles)
                .map(|rectangle| rectangle.area)
                .collect::<Vec<Area>>(),
        );

        let rectangles = self
            .layers
            .iter()
            .flat_map(|layer| &layer.rectangles)
            .map(|rectangle| RectangleData {
                colour: rectangle.colour,
                area: Vec4::new(
                    rectangle.area.origin.x,
                    rectangle.area.origin.y,
                    rectangle.area.size.x,
                    rectangle.area.size.y,
                ),
                sample_area: Vec4::ZERO,
                radius: Vec4::new(rectangle.radius, 0.0, 0.0, 0.0),
            })
            .collect::<Vec<_>>();

        (vertices, indices, rectangles)
    }

    fn render_text(&self) -> Result<(Vec<Vec2>, Vec<u32>, Vec<RectangleData>, (Size, Vec<u8>))> {
        let text = self
            .layers
            .iter()
            .flat_map(|layer| &layer.text)
            .collect::<Vec<_>>();

        let layouts = text
            .iter()
            .map(|text| {
                let mut layout = fontdue::layout::Layout::<()>::new(
                    fontdue::layout::CoordinateSystem::PositiveYDown,
                );
                layout.append(
                    &[text.font.clone()],
                    &TextStyle::new(&text.text, text.font_size, 0),
                );
                (text, layout.glyphs().to_owned())
            })
            .collect::<Vec<_>>();

        let areas = layouts
            .iter()
            .zip(&text)
            .flat_map(|((_, layout), text)| {
                let offset = layout
                    .first()
                    .map(|glyph| Vec2::new(glyph.x, glyph.y))
                    .unwrap_or_default();
                layout.iter().map(move |c| Area {
                    origin: Vec2::new(c.x, c.y) - offset + text.origin,
                    size: Vec2::new(c.width as f32, c.height as f32),
                })
            })
            .collect::<Vec<Area>>();

        let (vertices, indices) = Area::vertices(&areas);

        let glyphs: HashMap<fontdue::layout::GlyphRasterConfig, (&Text, Size)> =
            HashMap::from_iter(layouts.iter().flat_map(|(text, layout)| {
                layout
                    .iter()
                    .map(|c| (c.key, (**text, Size::new(c.width as i32, c.height as i32))))
            }));

        let mut atlas = etagere::BucketedAtlasAllocator::new(Size::new(1024, 512));
        let mut allocate = |size: etagere::euclid::Size2D<i32, etagere::euclid::UnknownUnit>| loop {
            if size.width == 0 || size.height == 0 {
                return etagere::euclid::Box2D::new(
                    etagere::euclid::Point2D::new(0, 0),
                    etagere::euclid::Point2D::new(0, 0),
                );
            }
            if let Some(etagere::Allocation { rectangle, .. }) = atlas.allocate(size) {
                return rectangle;
            }
            let size = atlas.size();
            atlas.grow(Size::new(size.width, size.height * 2));
        };

        let glyph_areas: HashMap<
            &fontdue::layout::GlyphRasterConfig,
            (
                &Text,
                etagere::euclid::Box2D<i32, etagere::euclid::UnknownUnit>,
            ),
        > = HashMap::from_iter(
            glyphs
                .iter()
                .map(|(key, (font, size))| (key, (*font, allocate(*size)))),
        );

        let image_size = atlas.size();
        let mut image_data = vec![0; image_size.width as usize * image_size.height as usize];
        for (key, (text, area)) in &glyph_areas {
            let (metrics, data) = text.font.rasterize_indexed(key.glyph_index, key.px);
            for y in 0..metrics.height {
                let image_index =
                    (area.min.y as usize + y) * image_size.width as usize + area.min.x as usize;
                let data_index = y * metrics.width;
                image_data[image_index..image_index + metrics.width]
                    .copy_from_slice(&data[data_index..data_index + metrics.width]);
            }
        }

        let sample_areas = layouts
            .iter()
            .flat_map(|(_, layout)| {
                layout.iter().map(|c| {
                    let (_, area) = glyph_areas.get(&c.key).unwrap();
                    Area {
                        origin: Vec2::new(area.min.x as f32, area.min.y as f32),
                        size: Vec2::new(
                            (area.max.x - area.min.x) as f32,
                            (area.max.y - area.min.y) as f32,
                        ),
                    }
                })
            })
            .collect::<Vec<Area>>();

        let colours = layouts
            .iter()
            .flat_map(|(text, layout)| vec![text.colour; layout.len()]);

        let rectangles: Vec<RectangleData> = areas
            .iter()
            .zip(sample_areas.clone())
            .zip(colours)
            .map(|((area, sample_area), colour)| RectangleData {
                area: area.as_vec4(),
                sample_area: Vec4::new(
                    sample_area.origin.x,
                    sample_area.origin.y,
                    area.size.x,
                    area.size.y,
                ),
                colour,
                radius: Vec4::ZERO,
            })
            .collect();

        Ok((vertices, indices, rectangles, (image_size, image_data)))
    }

    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(|layer| layer.is_empty())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: Vec2,
}

impl Vertex {
    pub fn info() -> vertex::Info {
        vertex::Info::new(size_of::<Self>()).attribute(AttributeType::Vec2, 0)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct RectangleData {
    pub colour: Vec4,
    pub area: Vec4,
    pub sample_area: Vec4,
    pub radius: Vec4,
}

pub struct Renderer {
    pipeline: Graphics,
    layout: Rc<descriptor::Layout>,
}

pub struct Frame {
    vertex_buffer: Rc<Static>,
    index_buffer: Rc<Static>,
    num_indices: u32,
    set: Rc<descriptor::Set>,
    view: Rc<ImageView>,
    sampler: Rc<Sampler>,
}

impl Renderer {
    pub fn new(ctx: &Context, render_pass: &RenderPass, subpass: usize) -> Result<Self> {
        let ui_vertex =
            ShaderModule::new(&ctx.device, &std::fs::read("assets/shaders/ui.vert.spv")?)?;

        let ui_fragment =
            ShaderModule::new(&ctx.device, &std::fs::read("assets/shaders/ui.frag.spv")?)?;

        let layout = descriptor::Layout::new(
            ctx,
            &[
                DescriptorType::STORAGE_BUFFER,
                DescriptorType::UNIFORM_BUFFER,
                DescriptorType::COMBINED_IMAGE_SAMPLER,
            ],
            1000,
        )?;

        let pipeline = Graphics::builder()
            .vertex(&ui_vertex)
            .vertex_info(Vertex::info())
            .fragment(&ui_fragment)
            .render_pass(render_pass)
            .subpass(subpass as u32)
            .viewport(Viewport::Dynamic)
            .layouts(vec![&layout])
            .multisampled(ctx.device.physical.get_samples())
            .build(&ctx.device)?;

        Ok(Self { pipeline, layout })
    }

    pub fn prepare(&self, ctx: &Context, scene: &Scene, viewport: Vec2) -> Result<Frame> {
        let rendered = scene.render()?;
        let num_indices = rendered.indices.len() as u32;
        let vertex_buffer = Static::new(
            ctx,
            bytemuck::cast_slice::<Vec2, u8>(&rendered.vertices),
            BufferUsageFlags::VERTEX_BUFFER,
        )?;
        let index_buffer = Static::new(
            ctx,
            bytemuck::cast_slice::<u32, u8>(&rendered.indices),
            BufferUsageFlags::INDEX_BUFFER,
        )?;
        let rectangle_buffer = Static::new(
            ctx,
            bytemuck::cast_slice::<RectangleData, u8>(&rendered.rectangles),
            BufferUsageFlags::STORAGE_BUFFER,
        )?;

        let viewport_buffer = Static::new(
            ctx,
            bytemuck::cast_slice::<Vec2, u8>(&[viewport]),
            BufferUsageFlags::UNIFORM_BUFFER,
        )?;

        let image = Rc::new(Image::new(
            ctx,
            ImageInfo {
                format: Format::R8_UNORM,
                extent: Extent2D {
                    width: rendered.image.0.width as u32,
                    height: rendered.image.0.height as u32,
                },
                usage: ImageUsageFlags::TRANSFER_DST | ImageUsageFlags::SAMPLED,
                samples: SampleCountFlags::TYPE_1,
            },
        )?);
        let buffer = Dynamic::new(ctx, rendered.image.1.len(), BufferUsageFlags::TRANSFER_SRC)?;
        buffer.write(&rendered.image.1)?;

        let cmd = ctx
            .command_pool
            .alloc()?
            .begin()?
            .transition_layout(
                &image,
                TransitionLayout {
                    from: ImageLayout::UNDEFINED,
                    to: ImageLayout::TRANSFER_DST_OPTIMAL,
                    before: (AccessFlags::NONE, PipelineStageFlags::TOP_OF_PIPE),
                    after: (AccessFlags::TRANSFER_WRITE, PipelineStageFlags::TRANSFER),
                },
            )
            .copy_buffer_to_image(
                &buffer,
                &image,
                ImageLayout::TRANSFER_DST_OPTIMAL,
                BufferToImageRegion {
                    from_offset: 0,
                    to_offset: Offset3D::default(),
                    to_extent: Extent3D {
                        width: rendered.image.0.width as u32,
                        height: rendered.image.0.height as u32,
                        depth: 1,
                    },
                },
            )
            .transition_layout(
                &image,
                TransitionLayout {
                    from: ImageLayout::TRANSFER_DST_OPTIMAL,
                    to: ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    before: (AccessFlags::TRANSFER_WRITE, PipelineStageFlags::TRANSFER),
                    after: (
                        AccessFlags::SHADER_READ,
                        PipelineStageFlags::FRAGMENT_SHADER,
                    ),
                },
            )
            .end()?;

        Task::run(&ctx.device, &ctx.device.queues.graphics, &cmd)?;

        let view = ImageView::new(
            &ctx.device,
            &image,
            Format::R8_UNORM,
            ImageAspectFlags::COLOR,
            Extent2D {
                width: rendered.image.0.width as u32,
                height: rendered.image.0.height as u32,
            },
        )?;

        let sampler = Sampler::new(&ctx.device)?;

        let set = self
            .layout
            .alloc()?
            .write_buffer(0, &rectangle_buffer)
            .write_buffer(1, &viewport_buffer)
            .write_image(2, &view, &sampler, ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .finish();

        Ok(Frame {
            vertex_buffer,
            index_buffer,
            num_indices,
            set,
            view,
            sampler,
        })
    }

    pub fn draw<'a>(&'a self, frame: Frame, cmd: command::Recorder<'a>) -> command::Recorder<'a> {
        let set = Rc::into_inner(frame.set)
            .unwrap()
            .write_image(
                2,
                &frame.view,
                &frame.sampler,
                ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            )
            .finish();

        cmd.next_subpass()
            .bind_graphics_pipeline(&self.pipeline)
            .bind_vertex_buffer(&frame.vertex_buffer, 0)
            .bind_index_buffer(&frame.index_buffer)
            .bind_descriptor_set(&set, 0)
            .draw_indexed(frame.num_indices, 1, 0, 0, 0)
    }
}

pub trait Element {
    fn layout(&mut self, constraint: Constraint<Vec2>) -> Vec2;
    fn paint(&mut self, area: Area, scene: &mut Scene, events: &[Event], signals: &mut Signals);
}
