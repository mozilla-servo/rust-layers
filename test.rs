use glut;
use azure;
use geom;

import geom::point::Point2D;
import geom::rect::Rect;
import geom::size::Size2D;
import layers::*;
import scene::*;
import rendergl::*;
import util::convert_rgb32_to_rgb24;

import glut::{DOUBLE, check_loop, create_window, destroy_window, display_func, init};
import glut::{init_display_mode, post_redisplay, swap_buffers, timer_func};

import azure::cairo::CAIRO_FORMAT_RGB24;
import CairoContext = azure::cairo_hl::Context;
import azure::azure_hl::{Color, ColorPattern, DrawTarget};
import azure::cairo_hl::ImageSurface;

import comm::{chan, peek, port, recv, send};
import libc::c_uint;
import os::{getenv, setenv};
import task::task_builder;

class Renderer {
    let image: @Image;
    let mut image_layer: @ImageLayer;
    let mut t: f32;
    let mut delta: f32;
    let mut render_context: option<RenderContext>;

    new() {
        let cairo_image = ImageSurface(CAIRO_FORMAT_RGB24, 500, 704);

        let draw_target = DrawTarget(cairo_image);
        draw_target.fill_rect(Rect(Point2D(50.0f32, 50.0f32), Size2D(300.0f32, 284.0f32)),
                              ColorPattern(Color(1.0f32, 1.0f32, 0.0f32, 1.0f32)));
        draw_target.flush();

        self.image = @Image(cairo_image.width() as uint, cairo_image.height() as uint, RGB24Format,
                            convert_rgb32_to_rgb24(cairo_image.data()));
        io::println(#fmt("image is %ux%u", self.image.width, self.image.height));

        self.image_layer = @ImageLayer(self.image);

        self.t = 1.0f32;
        self.delta = -0.001f32;

        self.render_context = none;
    }

    fn get_display_callback(this: @Renderer) -> fn@() {
        fn@() {
            (*this).display_callback();
        }
    }

    fn display_callback() {
        alt self.render_context {
            none {
                self.render_context = some(init_render_context());
            }
            some(_) {
                // Nothing to do.
            }
        }
        let context = alt self.render_context {
            none { fail }
            some(ctx) { ctx }
        };

        let t = self.t;
        self.image_layer.common.transform = Matrix4(400.0f32 * t, 0.0f32,       0.0f32, 0.0f32,
                                                    0.0f32,       300.0f32 * t, 0.0f32, 0.0f32,
                                                    0.0f32,       0.0f32,       1.0f32, 0.0f32,
                                                    0.0f32,       0.0f32,       0.0f32, 1.0f32);

        let mut scene = Scene(ImageLayerKind(self.image_layer), Size2D(400.0f32, 300.0f32));
        render_scene(context, scene);

        self.t += self.delta;
        if self.t < 0.0f32 || self.t > 1.0f32 {
            self.delta = -self.delta;
        }

        swap_buffers();

        post_redisplay();
    }
}

#[test]
fn test_triangle_and_square() unsafe {
    let builder = task::task().sched_mode(task::osmain);

    let po: port<()> = port();
    let ch = chan(po);
    let _result_ch: chan<()> = do builder.spawn_listener |_po| {
        let renderer = @Renderer();

        init();
        init_display_mode(DOUBLE as c_uint);
        let window = create_window(~"Rust Layers");
        display_func(renderer.get_display_callback(renderer));

        let wakeup = port();
        let wakeup_chan = chan(wakeup);
        do timer_func(2500) {
            send(wakeup_chan, ());
        }

        loop {
            check_loop();

            if peek(wakeup) {
                recv(wakeup);
                send(ch, ());
                destroy_window(window);
                break;
            }
        }

        send(ch, ());
    };

    recv(po);
}


