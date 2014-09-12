// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use color::Color;
use layers::Layer;
use layers;
use scene::Scene;
use texturegl::{Linear, Nearest, VerticalFlip};
use texturegl::{Texture, TextureTarget2D, TextureTargetRectangle};
use tiling::Tile;
use platform::surface::NativeCompositingGraphicsContext;

use geom::matrix::{Matrix4, identity, ortho};
use geom::size::Size2D;
use libc::c_int;
use opengles::gl2::{ARRAY_BUFFER, BLEND, COLOR_BUFFER_BIT, COMPILE_STATUS, FRAGMENT_SHADER};
use opengles::gl2::{LINK_STATUS, ONE_MINUS_SRC_ALPHA};
use opengles::gl2::{SRC_ALPHA, STATIC_DRAW, TEXTURE_2D, TEXTURE0};
use opengles::gl2::{LINE_STRIP, TRIANGLE_STRIP, VERTEX_SHADER, GLenum, GLfloat, GLint, GLsizei};
use opengles::gl2::{GLuint, active_texture, attach_shader, bind_buffer, bind_texture, blend_func};
use opengles::gl2::{buffer_data, create_program, clear, clear_color, compile_shader};
use opengles::gl2::{create_shader, draw_arrays, enable, enable_vertex_attrib_array, disable_vertex_attrib_array};
use opengles::gl2::{gen_buffers, get_attrib_location, get_program_info_log, get_program_iv};
use opengles::gl2::{get_shader_info_log, get_shader_iv, get_uniform_location, line_width};
use opengles::gl2::{link_program, shader_source, uniform_1i, uniform_4f};
use opengles::gl2::{uniform_matrix_4fv, use_program, vertex_attrib_pointer_f32, viewport};
use std::fmt;
use std::num::Zero;
use std::rc::Rc;

static FRAGMENT_SHADER_SOURCE: &'static str = "
    #ifdef GL_ES
        precision mediump float;
    #endif

    varying vec2 vTextureCoord;
    uniform samplerType uSampler;

    void main(void) {
        gl_FragColor = samplerFunction(uSampler, vTextureCoord);
    }
";

static SOLID_COLOR_FRAGMENT_SHADER_SOURCE: &'static str = "
    #ifdef GL_ES
        precision mediump float;
    #endif

    uniform vec4 uColor;
    void main(void) {
        gl_FragColor = uColor;
    }
";

static VERTEX_SHADER_SOURCE: &'static str = "
    attribute vec2 aVertexPosition;

    uniform mat4 uMVMatrix;
    uniform mat4 uPMatrix;
    uniform mat4 uTextureSpaceTransform;

    varying vec2 vTextureCoord;

    void main(void) {
        gl_Position = uPMatrix * uMVMatrix * vec4(aVertexPosition, 0.0, 1.0);
        vTextureCoord = (uTextureSpaceTransform * vec4(aVertexPosition, 0., 1.)).xy;
    }
";

static TEXTURED_QUAD_VERTICES: [f32, ..8] = [
    0.0, 0.0,
    0.0, 1.0,
    1.0, 0.0,
    1.0, 1.0,
];

static LINE_QUAD_VERTICES: [f32, ..10] = [
    0.0, 0.0,
    0.0, 1.0,
    1.0, 1.0,
    1.0, 0.0,
    0.0, 0.0,
];

static TILE_DEBUG_BORDER_COLOR: Color = Color { r: 0., g: 1., b: 1., a: 1.0 };
static TILE_DEBUG_BORDER_THICKNESS: uint = 1;
static LAYER_DEBUG_BORDER_COLOR: Color = Color { r: 1., g: 0.5, b: 0., a: 1.0 };
static LAYER_DEBUG_BORDER_THICKNESS: uint = 2;

struct Buffers {
    textured_quad_vertex_buffer: GLuint,
    line_quad_vertex_buffer: GLuint,
}

struct ShaderProgram {
    id: GLuint,
}

impl ShaderProgram {
    pub fn new(vertex_shader_source: &str, fragment_shader_source: &str) -> ShaderProgram {
        let id = create_program();
        attach_shader(id, ShaderProgram::compile_shader(fragment_shader_source, FRAGMENT_SHADER));
        attach_shader(id, ShaderProgram::compile_shader(vertex_shader_source, VERTEX_SHADER));
        link_program(id);
        if get_program_iv(id, LINK_STATUS) == (0 as GLint) {
            fail!("Failed to compile shader program: {:s}", get_program_info_log(id));
        }

        ShaderProgram {
            id: id,
        }
    }

    pub fn compile_shader(source_string: &str, shader_type: GLenum) -> GLuint {
        let id = create_shader(shader_type);
        shader_source(id, [ source_string.as_bytes() ]);
        compile_shader(id);
        if get_shader_iv(id, COMPILE_STATUS) == (0 as GLint) {
            fail!("Failed to compile shader: {:s}", get_shader_info_log(id));
        }

        return id;
    }

    pub fn get_attribute_location(&self, name: &str) -> GLint {
        get_attrib_location(self.id, name)
    }

    pub fn get_uniform_location(&self, name: &str) -> GLint {
        get_uniform_location(self.id, name)
    }
}

struct TextureProgram {
    program: ShaderProgram,
    vertex_position_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    sampler_uniform: c_int,
    texture_space_transform_uniform: c_int,
}

impl TextureProgram {
    fn new(sampler_function: &str, sampler_type: &str) -> TextureProgram {
        let fragment_shader_source
             = format_args!(fmt::format,
                            "#define samplerFunction {}\n#define samplerType {}\n{}",
                            sampler_function,
                            sampler_type,
                            FRAGMENT_SHADER_SOURCE);
        let program = ShaderProgram::new(VERTEX_SHADER_SOURCE, fragment_shader_source.as_slice());
        TextureProgram {
            program: program,
            vertex_position_attr: program.get_attribute_location("aVertexPosition"),
            modelview_uniform: program.get_uniform_location("uMVMatrix"),
            projection_uniform: program.get_uniform_location("uPMatrix"),
            sampler_uniform: program.get_uniform_location("uSampler"),
            texture_space_transform_uniform: program.get_uniform_location("uTextureSpaceTransform"),
        }
    }

    fn bind_uniforms_and_attributes(&self,
                                    transform: &Matrix4<f32>,
                                    projection_matrix: &Matrix4<f32>,
                                    texture_space_transform: &Matrix4<f32>,
                                    buffers: &Buffers) {
        uniform_1i(self.sampler_uniform, 0);
        uniform_matrix_4fv(self.modelview_uniform, false, transform.to_array());
        uniform_matrix_4fv(self.projection_uniform, false, projection_matrix.to_array());

        bind_buffer(ARRAY_BUFFER, buffers.textured_quad_vertex_buffer);
        vertex_attrib_pointer_f32(self.vertex_position_attr as GLuint, 2, false, 0, 0);

        uniform_matrix_4fv(self.texture_space_transform_uniform,
                           false,
                           texture_space_transform.to_array());
    }

    fn enable_attribute_arrays(&self) {
        enable_vertex_attrib_array(self.vertex_position_attr as GLuint);
    }

    fn disable_attribute_arrays(&self) {
        disable_vertex_attrib_array(self.vertex_position_attr as GLuint);
    }

    fn create_2d_program() -> TextureProgram {
        TextureProgram::new("texture2D", "sampler2D")
    }

    #[cfg(not(target_os="android"))]
    fn create_rectangle_program_if_necessary() -> Option<TextureProgram> {
        use opengles::gl2::TEXTURE_RECTANGLE_ARB;
        enable(TEXTURE_RECTANGLE_ARB);
        Some(TextureProgram::new("texture2DRect", "sampler2DRect"))
    }

    #[cfg(target_os="android")]
    fn create_rectangle_program_if_necessary() -> Option<TextureProgram> {
        None
    }
}

struct SolidLineProgram {
    program: ShaderProgram,
    vertex_position_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    color_uniform: c_int,
    texture_space_transform_uniform: c_int,
}

impl SolidLineProgram {
    fn new() -> SolidLineProgram {
        let program = ShaderProgram::new(VERTEX_SHADER_SOURCE, SOLID_COLOR_FRAGMENT_SHADER_SOURCE);
        SolidLineProgram {
            program: program,
            vertex_position_attr: program.get_attribute_location("aVertexPosition"),
            modelview_uniform: program.get_uniform_location("uMVMatrix"),
            projection_uniform: program.get_uniform_location("uPMatrix"),
            color_uniform: program.get_uniform_location("uColor"),
            texture_space_transform_uniform: program.get_uniform_location("uTextureSpaceTransform"),
        }
    }

    fn bind_uniforms_and_attributes(&self,
                                    transform: &Matrix4<f32>,
                                    projection_matrix: &Matrix4<f32>,
                                    buffers: &Buffers,
                                    color: Color) {
        uniform_matrix_4fv(self.modelview_uniform, false, transform.to_array());
        uniform_matrix_4fv(self.projection_uniform, false, projection_matrix.to_array());
        uniform_4f(self.color_uniform,
                   color.r as GLfloat,
                   color.g as GLfloat,
                   color.b as GLfloat,
                   color.a as GLfloat);

        bind_buffer(ARRAY_BUFFER, buffers.line_quad_vertex_buffer);
        vertex_attrib_pointer_f32(self.vertex_position_attr as GLuint, 2, false, 0, 0);

        let texture_transform: Matrix4<f32> = identity();
        uniform_matrix_4fv(self.texture_space_transform_uniform,
                           false,
                           texture_transform.to_array());
    }

    fn enable_attribute_arrays(&self) {
        enable_vertex_attrib_array(self.vertex_position_attr as GLuint);
    }

    fn disable_attribute_arrays(&self) {
        disable_vertex_attrib_array(self.vertex_position_attr as GLuint);
    }
}

pub struct RenderContext {
    texture_2d_program: TextureProgram,
    texture_rectangle_program: Option<TextureProgram>,
    solid_line_program: SolidLineProgram,
    buffers: Buffers,

    /// The platform-specific graphics context.
    compositing_context: NativeCompositingGraphicsContext,

    /// Whether to show lines at border and tile boundaries for debugging purposes.
    show_debug_borders: bool,
}

impl RenderContext {
    pub fn new(compositing_context: NativeCompositingGraphicsContext,
               show_debug_borders: bool) -> RenderContext {
        enable(TEXTURE_2D);
        enable(BLEND);
        blend_func(SRC_ALPHA, ONE_MINUS_SRC_ALPHA);

        let texture_2d_program = TextureProgram::create_2d_program();
        let solid_line_program = SolidLineProgram::new();
        let texture_rectangle_program = TextureProgram::create_rectangle_program_if_necessary();

        RenderContext {
            texture_2d_program: texture_2d_program,
            texture_rectangle_program: texture_rectangle_program,
            solid_line_program: solid_line_program,
            buffers: RenderContext::init_buffers(),
            compositing_context: compositing_context,
            show_debug_borders: show_debug_borders,
        }
    }

    fn init_buffers() -> Buffers {
        let textured_quad_vertex_buffer = gen_buffers(1)[0];
        bind_buffer(ARRAY_BUFFER, textured_quad_vertex_buffer);
        buffer_data(ARRAY_BUFFER, TEXTURED_QUAD_VERTICES, STATIC_DRAW);

        let line_quad_vertex_buffer = gen_buffers(1)[0];
        bind_buffer(ARRAY_BUFFER, line_quad_vertex_buffer);
        buffer_data(ARRAY_BUFFER, LINE_QUAD_VERTICES, STATIC_DRAW);

        Buffers {
            textured_quad_vertex_buffer: textured_quad_vertex_buffer,
            line_quad_vertex_buffer: line_quad_vertex_buffer,
        }
    }
}

pub fn bind_and_render_quad(render_context: RenderContext,
                            texture: &Texture,
                            transform: &Matrix4<f32>,
                            scene_size: Size2D<f32>) {
    let mut texture_coordinates_need_to_be_scaled_by_size = false;
    let program = match texture.target {
        TextureTarget2D => render_context.texture_2d_program,
        TextureTargetRectangle(..) => match render_context.texture_rectangle_program {
            Some(program) => {
                texture_coordinates_need_to_be_scaled_by_size = true;
                program
            }
            None => fail!("There is no shader program for texture rectangle"),
        },
    };
    program.enable_attribute_arrays();

    use_program(program.program.id);
    active_texture(TEXTURE0);

    // FIXME: This should technically check that the transform
    // matrix only contains scale in these components.
    let has_scale = transform.m11 as uint != texture.size.width ||
                    transform.m22 as uint != texture.size.height;
    let filter_mode = if has_scale {
        Linear
    } else {
        Nearest
    };
    texture.set_filter_mode(filter_mode);

    let _bound_texture = texture.bind();

    // Set the projection matrix.
    let projection_matrix = ortho(0.0, scene_size.width, scene_size.height, 0.0, -10.0, 10.0);

    // We calculate a transformation matrix for the texture coordinates
    // which is useful for flipping the texture vertically or scaling the
    // coordinates when dealing with GL_ARB_texture_rectangle.
    let mut texture_transform: Matrix4<f32> = identity();
    if texture.flip == VerticalFlip {
        texture_transform = texture_transform.scale(1.0, -1.0, 1.0);
    }
    if texture_coordinates_need_to_be_scaled_by_size {
        texture_transform = texture_transform.scale(texture.size.width as f32,
                                                    texture.size.height as f32,
                                                    1.0);
    }
    if texture.flip == VerticalFlip {
        texture_transform = texture_transform.translate(0.0, -1.0, 0.0);
    }

    program.bind_uniforms_and_attributes(transform,
                                         &projection_matrix,
                                         &texture_transform,
                                         &render_context.buffers);


    // Draw!
    draw_arrays(TRIANGLE_STRIP, 0, 4);
    bind_texture(TEXTURE_2D, 0);

    program.disable_attribute_arrays()
}

pub fn bind_and_render_quad_lines(render_context: RenderContext,
                                  transform: &Matrix4<f32>,
                                  scene_size: Size2D<f32>,
                                  color: Color,
                                  line_thickness: uint) {
    let solid_line_program = render_context.solid_line_program;
    solid_line_program.enable_attribute_arrays();
    use_program(solid_line_program.program.id);
    let projection_matrix = ortho(0.0, scene_size.width, scene_size.height, 0.0, -10.0, 10.0);
    solid_line_program.bind_uniforms_and_attributes(transform,
                                                    &projection_matrix,
                                                    &render_context.buffers,
                                                    color);
    line_width(line_thickness as GLfloat);
    draw_arrays(LINE_STRIP, 0, 5);
    solid_line_program.disable_attribute_arrays();
}

// Layer rendering

pub trait Render {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>);
}

impl<T> Render for layers::Layer<T> {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>) {
        let bounds = self.bounds.borrow().to_untyped();
        let cumulative_transform = transform.translate(bounds.origin.x, bounds.origin.y, 0.0);
        let tile_transform = cumulative_transform.mul(&*self.transform.borrow());

        self.create_textures(&render_context.compositing_context);
        self.do_for_all_tiles(|tile: &Tile| {
            tile.render(render_context, tile_transform, scene_size)
        });

        if render_context.show_debug_borders {
            let quad_transform = transform.scale(bounds.size.width, bounds.size.height, 1.);
            bind_and_render_quad_lines(render_context,
                                       &quad_transform,
                                       scene_size,
                                       LAYER_DEBUG_BORDER_COLOR,
                                       LAYER_DEBUG_BORDER_THICKNESS);
        }

        for child in self.children().iter() {
            child.render(render_context, cumulative_transform, scene_size)
        }

    }
}

impl Render for Tile {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>) {
        if self.texture.is_zero() {
            return;
        }

        let transform = transform.mul(&self.transform);
        bind_and_render_quad(render_context, &self.texture, &transform, scene_size);

        if render_context.show_debug_borders {
            bind_and_render_quad_lines(render_context,
                                       &transform,
                                       scene_size,
                                       TILE_DEBUG_BORDER_COLOR,
                                       TILE_DEBUG_BORDER_THICKNESS);
        }
    }
}

pub fn render_scene<T>(root_layer: Rc<Layer<T>>,
                       render_context: RenderContext,
                       scene: &Scene<T>) {
    // Set the viewport.
    viewport(0 as GLint, 0 as GLint, scene.size.width as GLsizei, scene.size.height as GLsizei);

    // Clear the screen.
    clear_color(scene.background_color.r,
                scene.background_color.g,
                scene.background_color.b,
                scene.background_color.a);
    clear(COLOR_BUFFER_BIT);

    // Set up the initial modelview matrix.
    let transform = identity().scale(scene.scale, scene.scale, 1.0);

    // Render the root layer.
    root_layer.render(render_context, transform, scene.size);
}
