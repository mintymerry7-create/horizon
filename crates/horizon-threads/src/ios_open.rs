//! iOS: opening a web page in Safari (`UIApplication.open(_:options:completionHandler:)`), for
//! the Microsoft sign-in's code page and chat links. iOS apps can't start other programs.
use block2::Block;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_foundation::{NSDictionary, NSString, NSURL};

/// Opens `url` in the system browser. Call it on the main thread (UIKit's).
pub fn open_url(url: &str) -> bool {
    let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) else {
        return false;
    };
    let Some(class) = AnyClass::get(c"UIApplication") else {
        return false;
    };
    let options = NSDictionary::<NSString, AnyObject>::new();
    let done: Option<&Block<dyn Fn(Bool)>> = None;
    // SAFETY: UIApplication's shared instance and its documented open method, with a URL, an
    // empty options dictionary and no completion block; nothing is kept past the call.
    unsafe {
        let app: *mut AnyObject = msg_send![class, sharedApplication];
        if app.is_null() {
            return false;
        }
        let () = msg_send![app, openURL: &*url, options: &*options, completionHandler: done];
    }
    true
}
