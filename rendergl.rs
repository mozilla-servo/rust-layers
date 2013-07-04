// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use layers::{ContainerLayerKind, Flip, NoFlip, TextureLayerKind, VerticalFlip};
use layers;
use scene::Scene;
use texturegl::{Texture};

use geom::matrix::{Matrix4, ortho};
use opengles::gl2::{ARRAY_BUFFER, COLOR_BUFFER_BIT, COMPILE_STATUS, FRAGMENT_SHADER, LINK_STATUS};
use opengles::gl2::{NO_ERROR, STATIC_DRAW, TEXTURE_2D, TEXTURE0, TRIANGLE_STRIP, VERTEX_SHADER};
use opengles::gl2::{GLenum, GLint, GLsizei, GLuint, active_texture, attach_shader, bind_buffer};
use opengles::gl2::{buffer_data, create_program, clear, clear_color, compile_shader};
use opengles::gl2::{create_shader, draw_arrays, enable, enable_vertex_attrib_array, gen_buffers};
use opengles::gl2::{get_attrib_location, get_error, get_program_iv, get_shader_info_log};
use opengles::gl2::{get_shader_iv, get_uniform_location, link_program, shader_source, uniform_1i};
use opengles::gl2::{uniform_matrix_4fv, use_program, vertex_attrib_pointer_f32, viewport};

use std::libc::c_int;

static FRAGMENT_SHADER_SOURCE: &'static str = "
    #ifdef GLES2
        precision mediump float;
    #endif

    varying vec2 vTextureCoord;

    uniform sampler2D uSampler;

    void main(void) {
        gl_FragColor = texture2D(uSampler, vTextureCoord);
    }
";

static VERTEX_SHADER_SOURCE: &'static str = "
    attribute vec3 aVertexPosition;
    attribute vec2 aTextureCoord;

    uniform mat4 uMVMatrix;
    uniform mat4 uPMatrix;

    varying vec2 vTextureCoord;

    void main(void) {
        gl_Position = uPMatrix * uMVMatrix * vec4(aVertexPosition, 1.0);
        vTextureCoord = aTextureCoord;
    }
";

pub fn load_shader(source_string: &str, shader_type: GLenum) -> GLuint {
    let shader_id = create_shader(shader_type);
    shader_source(shader_id, [ source_string.as_bytes().to_owned() ]);
    compile_shader(shader_id);

    if get_error() != NO_ERROR {
        println(fmt!("error: %d", get_error() as int));
        fail!(~"failed to compile shader");
    }

    if get_shader_iv(shader_id, COMPILE_STATUS) == (0 as GLint) {
        println(fmt!("shader info log: %s", get_shader_info_log(shader_id)));
        fail!(~"failed to compile shader");
    }

    return shader_id;
}

pub struct RenderContext {
    program: GLuint,
    vertex_position_attr: c_int,
    texture_coord_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    sampler_uniform: c_int,
    vertex_buffer: GLuint,
    texture_coord_buffer: GLuint,
}

pub fn RenderContext(program: GLuint) -> RenderContext {
    let (vertex_buffer, texture_coord_buffer) = init_buffers();
    let rc = RenderContext {
        program: program,
        vertex_position_attr: get_attrib_location(program, ~"aVertexPosition"),
        texture_coord_attr: get_attrib_location(program, ~"aTextureCoord"),
        modelview_uniform: get_uniform_location(program, ~"uMVMatrix"),
        projection_uniform: get_uniform_location(program, ~"uPMatrix"),
        sampler_uniform: get_uniform_location(program, ~"uSampler"),
        vertex_buffer: vertex_buffer,
        texture_coord_buffer: texture_coord_buffer,
    };

    enable_vertex_attrib_array(rc.vertex_position_attr as GLuint);
    enable_vertex_attrib_array(rc.texture_coord_attr as GLuint);

    rc
}

pub fn init_render_context() -> RenderContext {
    let vertex_shader = load_shader(VERTEX_SHADER_SOURCE, VERTEX_SHADER);
    let fragment_shader = load_shader(FRAGMENT_SHADER_SOURCE, FRAGMENT_SHADER);

    let program = create_program();
    attach_shader(program, vertex_shader);
    attach_shader(program, fragment_shader);
    link_program(program);

    if get_program_iv(program, LINK_STATUS) == (0 as GLint) {
        fail!(~"failed to initialize program");
    }

    use_program(program);
    enable(TEXTURE_2D);

    return RenderContext(program);
}

pub fn init_buffers() -> (GLuint, GLuint) {
    let triangle_vertex_buffer = gen_buffers(1 as GLsizei)[0];
    bind_buffer(ARRAY_BUFFER, triangle_vertex_buffer);

    let (_0, _1) = (0.0f32, 1.0f32);
    let vertices = ~[
        _0, _0, _0,
        _0, _1, _0,
        _1, _0, _0,
        _1, _1, _0
    ];

    buffer_data(ARRAY_BUFFER, vertices, STATIC_DRAW);

    let texture_coord_buffer = gen_buffers(1 as GLsizei)[0];
    bind_buffer(ARRAY_BUFFER, texture_coord_buffer);

    return (triangle_vertex_buffer, texture_coord_buffer);
}

pub fn bind_and_render_quad(render_context: RenderContext, texture: &Texture, flip: Flip) {
    active_texture(TEXTURE0);
    let _bound_texture = texture.bind();

    uniform_1i(render_context.sampler_uniform, 0);

    bind_buffer(ARRAY_BUFFER, render_context.vertex_buffer);
    vertex_attrib_pointer_f32(render_context.vertex_position_attr as GLuint, 3, false, 0, 0);

    // Create the texture coordinate array.
    bind_buffer(ARRAY_BUFFER, render_context.texture_coord_buffer);

    let vertices: [f32, ..8] = match flip {
        NoFlip => {
            [
                0.0, 0.0,
                0.0, 1.0,
                1.0, 0.0,
                1.0, 1.0,
            ]
        }
        VerticalFlip => {
            [
                0.0, 1.0,
                0.0, 0.0,
                1.0, 1.0,
                1.0, 0.0,
            ]
        }
    };

    buffer_data(ARRAY_BUFFER, vertices, STATIC_DRAW);
    vertex_attrib_pointer_f32(render_context.texture_coord_attr as GLuint, 2, false, 0, 0);
    draw_arrays(TRIANGLE_STRIP, 0, 4);
}

// Layer rendering

pub trait Render {
    fn render(@mut self, render_context: RenderContext, transform: Matrix4<f32>);
}

impl Render for layers::ContainerLayer {
    fn render(@mut self, render_context: RenderContext, transform: Matrix4<f32>) {
        let transform = transform.mul(&self.common.transform);
        for self.each_child |child| {
            render_layer(render_context, transform, child);
        }
    }
}

impl Render for layers::TextureLayer {
    fn render(@mut self, render_context: RenderContext, transform: Matrix4<f32>) {
        let transform = transform.mul(&self.common.transform);
        uniform_matrix_4fv(render_context.modelview_uniform, false, transform.to_array());

        bind_and_render_quad(render_context, self.texture.get(), self.flip);
    }
}

fn render_layer(render_context: RenderContext, transform: Matrix4<f32>, layer: layers::Layer) {
    match layer {
        ContainerLayerKind(container_layer) => container_layer.render(render_context, transform),
        TextureLayerKind(texture_layer) => texture_layer.render(render_context, transform),
    }
}

pub fn render_scene(render_context: RenderContext, scene: &Scene) {
    // Set the viewport.
    viewport(0 as GLint, 0 as GLint, scene.size.width as GLsizei, scene.size.height as GLsizei);

    // Clear the screen.
    clear_color(0.38f32, 0.36f32, 0.36f32, 1.0f32);
    clear(COLOR_BUFFER_BIT);

    // Set the projection matrix.
    let projection_matrix = ortho(0.0, scene.size.width, scene.size.height, 0.0, -10.0, 10.0);
    uniform_matrix_4fv(render_context.projection_uniform, false, projection_matrix.to_array());

    // Set up the initial modelview matrix.
    let transform = scene.transform;

    // Render the root layer.
    render_layer(render_context, transform, scene.root);
}

