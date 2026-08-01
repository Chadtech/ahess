use std::ffi::c_void;

use cocoa::{
    appkit::{NSApp, NSApplication, NSImage},
    base::nil,
    foundation::{NSAutoreleasePool, NSData, NSUInteger},
};

const APPLICATION_ICON: &[u8] = include_bytes!("../assets/ahess-icon.png");

pub fn set_application_icon() {
    // GPUI creates and owns NSApplication before invoking its run callback.
    // Loading the embedded PNG here also gives unbundled `cargo run` launches
    // the Ahess icon instead of macOS's generic executable tile.
    unsafe {
        let data = NSData::dataWithBytes_length_(
            nil,
            APPLICATION_ICON.as_ptr().cast::<c_void>(),
            APPLICATION_ICON.len() as NSUInteger,
        );
        let image = NSImage::initWithData_(NSImage::alloc(nil), data);

        if image != nil {
            NSApplication::setApplicationIconImage_(NSApp(), image);
            NSAutoreleasePool::autorelease(image);
        }
    }
}
