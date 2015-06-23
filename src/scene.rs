// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use euclid::rect::{Rect, TypedRect};
use euclid::scale_factor::ScaleFactor;
use euclid::size::TypedSize2D;
use euclid::point::Point2D;
use geometry::{DevicePixel, LayerPixel};
use layers::{BufferRequest, Layer, LayerBuffer};
use std::rc::Rc;

pub struct Scene<T> {
    pub root: Option<Rc<Layer<T>>>,
    pub viewport: TypedRect<DevicePixel, f32>,

    /// The scene scale, to allow for zooming and high-resolution painting.
    pub scale: ScaleFactor<LayerPixel, DevicePixel, f32>,
}

impl<T> Scene<T> {
    pub fn new(viewport: TypedRect<DevicePixel, f32>) -> Scene<T> {
        Scene {
            root: None,
            viewport: viewport,
            scale: ScaleFactor::new(1.0),
        }
    }

    pub fn get_buffer_requests_for_layer(&mut self,
                                         layer: Rc<Layer<T>>,
                                         dirty_rect: TypedRect<LayerPixel, f32>,
                                         viewport_rect: TypedRect<LayerPixel, f32>,
                                         layers_and_requests: &mut Vec<(Rc<Layer<T>>,
                                                                        Vec<BufferRequest>)>,
                                         unused_buffers: &mut Vec<(Rc<Layer<T>>,
                                                                        Vec<Box<LayerBuffer>>)>) {
        // Get buffers for this layer, in global (screen) coordinates.
        let requests = layer.get_buffer_requests(dirty_rect,
                                                 viewport_rect,
                                                 self.scale);
        if !requests.is_empty() {
            layers_and_requests.push((layer.clone(), requests));
        }
        unused_buffers.push((layer.clone(), layer.collect_unused_buffers()));

        // If this layer masks its children, we don't need to ask for tiles outside the
        // boundaries of this layer.
        let mut child_dirty_rect = dirty_rect;
        if *layer.masks_to_bounds.borrow() {
            // FIXME: Likely because of rust bug rust-lang/rust#16822, caching the intersected
            // rect and reusing it causes a crash in rustc. When that bug is resolved this code
            // should simply reuse a cached version of the intersection.
            match layer.transform_state.borrow().screen_rect {
                Some(ref screen_rect) => {
                    child_dirty_rect = match dirty_rect.to_untyped().intersection(&screen_rect.rect) {
                        Some(child_dirty_rect) => {
                            Rect::from_untyped(&child_dirty_rect)
                        }
                        None => {
                            // The layer is entirely clipped by the dirty
                            // rect, so early exit.
                            return;
                        }
                    }
                }
                None => {
                    // The layer is entirely clipped, and it masks children,
                    // so early exit.
                    return;
                }
            }
        }

        for kid in layer.children().iter() {
            self.get_buffer_requests_for_layer(kid.clone(),
                                               child_dirty_rect,
                                               viewport_rect,
                                               layers_and_requests,
                                               unused_buffers);
        }
    }

    pub fn get_buffer_requests(&mut self,
                               requests: &mut Vec<(Rc<Layer<T>>, Vec<BufferRequest>)>,
                               unused_buffers: &mut Vec<(Rc<Layer<T>>, Vec<Box<LayerBuffer>>)>) {
        let root_layer = match self.root {
            Some(ref root_layer) => root_layer.clone(),
            None => return,
        };

        self.get_buffer_requests_for_layer(root_layer.clone(),
                                           *root_layer.bounds.borrow(),
                                           *root_layer.bounds.borrow(),
                                           requests,
                                           unused_buffers);
    }

    pub fn mark_layer_contents_as_changed_recursively_for_layer(&self, layer: Rc<Layer<T>>) {
        layer.contents_changed();
        for kid in layer.children().iter() {
            self.mark_layer_contents_as_changed_recursively_for_layer(kid.clone());
        }
    }

    pub fn mark_layer_contents_as_changed_recursively(&self) {
        let root_layer = match self.root {
            Some(ref root_layer) => root_layer.clone(),
            None => return,
        };
        self.mark_layer_contents_as_changed_recursively_for_layer(root_layer);
    }

    pub fn set_root_layer_size(&self, new_size: TypedSize2D<DevicePixel, f32>) {
        match self.root {
            Some(ref root_layer) => {
                *root_layer.bounds.borrow_mut() = Rect::new(Point2D::zero(), new_size / self.scale);
            },
            None => {},
        }
    }
}

