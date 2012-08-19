import geom::matrix::{Matrix4, identity};
import opengles::gl2::{GLuint, delete_textures};

import std::cmp::fuzzy_eq;
import dvec::{DVec, dvec};

enum Format {
    ARGB32Format,
    RGB24Format
}

enum Layer {
    ContainerLayerKind(@ContainerLayer),
    ImageLayerKind(@ImageLayer),
    TiledImageLayerKind(@TiledImageLayer)
}

struct CommonLayer {
    let mut parent: option<Layer>;
    let mut prev_sibling: option<Layer>;
    let mut next_sibling: option<Layer>;

    let mut transform: Matrix4<f32>;

    new() {
        self.parent = none;
        self.prev_sibling = none;
        self.next_sibling = none;

        self.transform = identity(0.0f32);
    }

    // FIXME: Workaround for cross-crate bug regarding mutability of class fields
    fn set_transform(new_transform: Matrix4<f32>) {
        self.transform = new_transform;
    }
}

struct ContainerLayer {
    let mut common: CommonLayer;
    let mut first_child: option<Layer>;
    let mut last_child: option<Layer>;

    new() {
        self.common = CommonLayer();
        self.first_child = none;
        self.last_child = none;
    }
}

struct Image {
    let width: uint;
    let height: uint;
    let format: Format;
    let data: ~[u8];

    let mut texture: option<GLuint>;

    new(width: uint, height: uint, format: Format, +data: ~[u8]) {
        self.width = width;
        self.height = height;
        self.format = format;
        self.data = data;

        self.texture = none;
    }

    drop {
        match copy self.texture {
            none => {
                // Nothing to do.
            }
            some(texture) => {
                delete_textures(&[texture]);
            }
        }
    }
}

struct ImageLayer {
    let mut common: CommonLayer;
    let mut image: @layers::Image;

    new(image: @layers::Image) {
        self.common = CommonLayer();
        self.image = image;
    }

    // FIXME: Workaround for cross-crate bug
    fn set_image(new_image: @layers::Image) {
        self.image = new_image;
    }
}

struct TiledImageLayer {
    mut common: CommonLayer;
    tiles: DVec<@layers::Image>;
    mut tiles_across: uint;
}

fn TiledImageLayer(in_tiles: &[@layers::Image], tiles_across: uint) -> TiledImageLayer {
    let tiles = dvec();
    for in_tiles.each |tile| {
        tiles.push(tile);
    }

    TiledImageLayer {
        common: CommonLayer(),
        tiles: tiles,
        tiles_across: tiles_across
    }
}

