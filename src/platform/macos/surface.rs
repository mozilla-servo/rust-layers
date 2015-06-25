// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Mac OS-specific implementation of cross-process surfaces. This uses `IOSurface`, introduced
//! in Mac OS X 10.6 Snow Leopard.

use texturegl::Texture;

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use euclid::size::Size2D;
use io_surface::{kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow, kIOSurfaceHeight};
use io_surface::{kIOSurfaceIsGlobal, kIOSurfaceWidth, IOSurface, IOSurfaceID};
use io_surface;
use cgl::{CGLChoosePixelFormat, CGLDescribePixelFormat, CGLPixelFormatAttribute};
use cgl::{CGLPixelFormatObj, CORE_BOOLEAN_ATTRIBUTES, CORE_INTEGER_ATTRIBUTES};
use cgl::{kCGLNoError};
use gleam::gl::GLint;
use skia::{SkiaSkNativeSharedGLContextRef, SkiaSkNativeSharedGLContextStealSurface};
use std::cell::RefCell;
use std::collections::HashMap;
use std::mem;
use std::ptr;
use std::rc::Rc;
use std::vec::Vec;

thread_local!(static IO_SURFACE_REPOSITORY: Rc<RefCell<HashMap<IOSurfaceID,IOSurface>>> = Rc::new(RefCell::new(HashMap::new())));

/// The Mac native graphics metadata.
#[derive(Clone, Copy)]
pub struct NativeGraphicsMetadata {
    pub pixel_format: CGLPixelFormatObj,
}
unsafe impl Send for NativeGraphicsMetadata {}

impl NativeGraphicsMetadata {
    /// Creates a native graphics metadatum from a CGL pixel format.
    pub fn from_cgl_pixel_format(pixel_format: CGLPixelFormatObj) -> NativeGraphicsMetadata {
        NativeGraphicsMetadata {
            pixel_format: pixel_format,
        }
    }
}

pub struct NativePaintingGraphicsContext {
    _metadata: NativeGraphicsMetadata,
}

impl NativePaintingGraphicsContext {
    pub fn from_metadata(metadata: &NativeGraphicsMetadata) -> NativePaintingGraphicsContext {
        NativePaintingGraphicsContext {
            _metadata: (*metadata).clone(),
        }
    }
}

impl Drop for NativePaintingGraphicsContext {
    fn drop(&mut self) {}
}

#[derive(Copy, Clone)]
pub struct NativeCompositingGraphicsContext {
    _contents: (),
}

impl NativeCompositingGraphicsContext {
    pub fn new() -> NativeCompositingGraphicsContext {
        NativeCompositingGraphicsContext {
            _contents: (),
        }
    }
}

#[derive(RustcDecodable, RustcEncodable)]
pub struct IOSurfaceNativeSurface {
    io_surface_id: Option<IOSurfaceID>,
    will_leak: bool,
}

impl IOSurfaceNativeSurface {
    pub fn from_io_surface(io_surface: IOSurface) -> IOSurfaceNativeSurface {
        // Take the surface by ID (so that we can send it cross-process) and consume its reference.
        let id = io_surface.get_id();

        let mut io_surface = Some(io_surface);

        IO_SURFACE_REPOSITORY.with(|ref r| {
            r.borrow_mut().insert(id, io_surface.take().unwrap())
        });

        IOSurfaceNativeSurface {
            io_surface_id: Some(id),
            will_leak: true,
        }
    }

    pub fn from_skia_shared_gl_context(context: SkiaSkNativeSharedGLContextRef)
                                       -> IOSurfaceNativeSurface {
        unsafe {
            let surface = SkiaSkNativeSharedGLContextStealSurface(context);
            let io_surface = IOSurface {
                obj: mem::transmute(surface),
            };
            IOSurfaceNativeSurface::from_io_surface(io_surface)
        }
    }

    pub fn new(_: &NativePaintingGraphicsContext, size: Size2D<i32>) -> IOSurfaceNativeSurface {
        unsafe {
            let width_key: CFString = TCFType::wrap_under_get_rule(kIOSurfaceWidth);
            let width_value: CFNumber = CFNumber::from_i32(size.width);

            let height_key: CFString = TCFType::wrap_under_get_rule(kIOSurfaceHeight);
            let height_value: CFNumber = CFNumber::from_i32(size.height);

            let bytes_per_row_key: CFString = TCFType::wrap_under_get_rule(kIOSurfaceBytesPerRow);
            let bytes_per_row_value: CFNumber = CFNumber::from_i32(size.width * 4);

            let bytes_per_elem_key: CFString =
                TCFType::wrap_under_get_rule(kIOSurfaceBytesPerElement);
            let bytes_per_elem_value: CFNumber = CFNumber::from_i32(4);

            let is_global_key: CFString = TCFType::wrap_under_get_rule(kIOSurfaceIsGlobal);
            let is_global_value = CFBoolean::true_value();

            let surface = io_surface::new(&CFDictionary::from_CFType_pairs(&[
                (width_key.as_CFType(), width_value.as_CFType()),
                (height_key.as_CFType(), height_value.as_CFType()),
                (bytes_per_row_key.as_CFType(), bytes_per_row_value.as_CFType()),
                (bytes_per_elem_key.as_CFType(), bytes_per_elem_value.as_CFType()),
                (is_global_key.as_CFType(), is_global_value.as_CFType()),
            ]));

            IOSurfaceNativeSurface::from_io_surface(surface)
        }
    }

    pub fn bind_to_texture(&self,
                           _: &NativeCompositingGraphicsContext,
                           texture: &Texture,
                           size: Size2D<isize>) {
        let _bound_texture = texture.bind();
        let io_surface = io_surface::lookup(self.io_surface_id.unwrap());
        io_surface.bind_to_gl_texture(Size2D::new(size.width as i32, size.height as i32))
    }

    pub fn upload(&mut self, _: &NativePaintingGraphicsContext, data: &[u8]) {
        let io_surface = io_surface::lookup(self.io_surface_id.unwrap());
        io_surface.upload(data)
    }

    pub fn get_id(&self) -> isize {
        match self.io_surface_id {
            None => 0,
            Some(id) => id as isize,
        }
    }

    pub fn destroy(&mut self, _: &NativePaintingGraphicsContext) {
        IO_SURFACE_REPOSITORY.with(|ref r| {
            r.borrow_mut().remove(&self.io_surface_id.unwrap())
        });
        self.io_surface_id = None;
        self.mark_wont_leak()
    }

    pub fn mark_will_leak(&mut self) {
        self.will_leak = true
    }

    pub fn mark_wont_leak(&mut self) {
        self.will_leak = false
    }
}
