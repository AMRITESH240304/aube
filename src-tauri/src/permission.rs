// macOS hands back silence, not an error, when an app may not capture system
// audio, so the call would quietly go missing from recordings. There's no
// public API for this permission; these are the private TCC calls Apple's own
// AudioCap sample uses.

use std::{
    ffi::{c_char, c_void, CStr},
    ptr::null,
    sync::mpsc,
    time::Duration,
};

use block2::RcBlock;

use crate::Result;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(
        alloc: *const c_void,
        s: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFRelease(object: *const c_void);
}

extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

type Preflight = unsafe extern "C" fn(*const c_void, *const c_void) -> i32;
type Request = unsafe extern "C" fn(*const c_void, *const c_void, *const c_void);

const ALLOWED: i32 = 0;
const UNDECIDED: i32 = 2;
const UTF8: u32 = 0x0800_0100;

// Under `tauri dev` the terminal asks on the app's behalf, and macOS checks the
// terminal's permission instead.
fn bundled() -> bool {
    std::env::current_exe().is_ok_and(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS/"))
}

fn symbol<T>(handle: *mut c_void, name: &CStr) -> Option<T> {
    let found = unsafe { dlsym(handle, name.as_ptr()) };
    (!found.is_null()).then(|| unsafe { std::mem::transmute_copy(&found) })
}

// Asks the first time, showing macOS's prompt; the answer arrives off the main thread.
fn request(request: Request, service: *const c_void) -> bool {
    let (tx, rx) = mpsc::channel();
    let handler = RcBlock::new(move |granted: u8| {
        let _ = tx.send(granted != 0);
    });
    unsafe { request(service, null(), &*handler as *const _ as *const c_void) };
    rx.recv_timeout(Duration::from_secs(120)).unwrap_or(false)
}

pub fn system_audio() -> Result<()> {
    let tcc = unsafe {
        dlopen(
            c"/System/Library/PrivateFrameworks/TCC.framework/TCC".as_ptr(),
            1,
        )
    };
    let (Some(preflight), Some(ask)) = (
        symbol::<Preflight>(tcc, c"TCCAccessPreflight"),
        symbol::<Request>(tcc, c"TCCAccessRequest"),
    ) else {
        return Ok(()); // Can't tell, so try anyway.
    };
    let service =
        unsafe { CFStringCreateWithCString(null(), c"kTCCServiceAudioCapture".as_ptr(), UTF8) };
    let allowed = match unsafe { preflight(service, null()) } {
        ALLOWED => true,
        UNDECIDED if bundled() => request(ask, service),
        _ => false,
    };
    unsafe { CFRelease(service) };
    if allowed {
        Ok(())
    } else if bundled() {
        Err(
            "Ease Rec isn't allowed to hear system audio. Turn it on in System Settings → \
             Privacy & Security → Screen & System Audio Recording, under System Audio \
             Recording Only, then start a new recording."
                .into(),
        )
    } else {
        Err(
            "Under `tauri dev`, macOS checks your terminal's permission. Add your terminal \
             in System Settings → Privacy & Security → Screen & System Audio Recording, \
             under System Audio Recording Only, or run the built app."
                .into(),
        )
    }
}
