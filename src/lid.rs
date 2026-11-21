//! Lid state and built-in display brightness, and the fade that links them.
//!
//! macOS has no public brightness API, so this loads the private DisplayServices framework at
//! runtime. If a future macOS drops it, the lookups fail and the fade quietly does nothing.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2_foundation::{NSNumber, NSString, ns_string};

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGGetOnlineDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayIsBuiltin(display: u32) -> u32;
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    /// Consumes the matching dictionary.
    fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> u32;
    /// Returns a +1 CoreFoundation object, or null.
    fn IORegistryEntryCreateCFProperty(
        entry: u32,
        key: *const NSString,
        allocator: *const c_void,
        options: u32,
    ) -> *mut c_void;
    fn IOObjectRelease(object: u32) -> i32;
}

/// Whether the lid is shut, or None on a Mac without one.
pub fn closed() -> Option<bool> {
    unsafe {
        let root = IOServiceGetMatchingService(0, IOServiceMatching(c"IOPMrootDomain".as_ptr()));
        if root == 0 {
            return None;
        }
        // NSString and NSNumber are toll-free bridged to the CFString key and CFBoolean value.
        let key: &NSString = ns_string!("AppleClamshellState");
        let value = IORegistryEntryCreateCFProperty(root, key, std::ptr::null(), 0);
        IOObjectRelease(root);
        Retained::from_raw(value.cast::<NSNumber>()).map(|state| state.boolValue())
    }
}

type GetBrightness = unsafe extern "C" fn(display: u32, level: *mut f32) -> i32;
type SetBrightness = unsafe extern "C" fn(display: u32, level: f32) -> i32;

fn display_services() -> Option<(GetBrightness, SetBrightness)> {
    static SYMBOLS: OnceLock<Option<(GetBrightness, SetBrightness)>> = OnceLock::new();
    *SYMBOLS.get_or_init(|| unsafe {
        const PATH: &CStr = c"/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices";
        const RTLD_LAZY: c_int = 1;
        let handle = dlopen(PATH.as_ptr(), RTLD_LAZY);
        if handle.is_null() {
            return None;
        }
        let get = dlsym(handle, c"DisplayServicesGetBrightness".as_ptr());
        let set = dlsym(handle, c"DisplayServicesSetBrightness".as_ptr());
        if get.is_null() || set.is_null() {
            return None;
        }
        Some((
            std::mem::transmute::<*mut c_void, GetBrightness>(get),
            std::mem::transmute::<*mut c_void, SetBrightness>(set),
        ))
    })
}

/// The laptop's own panel. External monitors are never touched.
fn builtin_display() -> Option<u32> {
    let mut displays = [0u32; 16];
    let mut count = 0u32;
    if unsafe { CGGetOnlineDisplayList(displays.len() as u32, displays.as_mut_ptr(), &mut count) } != 0 {
        return None;
    }
    displays[..count as usize].iter().copied().find(|&d| unsafe { CGDisplayIsBuiltin(d) } != 0)
}

/// Built-in display brightness, 0.0 to 1.0.
pub fn brightness() -> Option<f32> {
    let (get, _) = display_services()?;
    let mut level = 0.0;
    (unsafe { get(builtin_display()?, &mut level) } == 0).then_some(level)
}

pub fn set_brightness(level: f32) {
    if let (Some((_, set)), Some(display)) = (display_services(), builtin_display()) {
        unsafe { set(display, level) };
    }
}

/// A fade takes this many steps whatever the starting brightness.
const STEPS: f32 = 24.0;

struct Fade {
    original: f32,
    level: f32,
}

/// Fades the screen to black while the lid is shut and back to where it was when it opens.
#[derive(Default)]
pub struct Dimmer {
    fade: Option<Fade>,
}

impl Dimmer {
    /// Advances one step. Returns the brightness to apply, or None once there is nothing to do.
    /// `current` is read once, when the lid first shuts, to remember where to come back to.
    pub fn step(&mut self, closed: bool, current: impl FnOnce() -> Option<f32>) -> Option<f32> {
        if closed && self.fade.is_none() {
            let original = current()?;
            self.fade = Some(Fade { original, level: original });
        }
        let fade = self.fade.as_mut()?;
        let target = if closed { 0.0 } else { fade.original };
        if fade.level == target {
            if !closed {
                self.fade = None;
            }
            return None;
        }
        let delta = fade.original / STEPS;
        fade.level = if closed {
            (fade.level - delta).max(0.0)
        } else {
            (fade.level + delta).min(fade.original)
        };
        Some(fade.level)
    }

    /// The brightness to put back if Lidless quits mid-fade or with the lid shut.
    pub fn original(&self) -> Option<f32> {
        self.fade.as_ref().map(|fade| fade.original)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steps until settled, returning every brightness applied.
    fn run(dimmer: &mut Dimmer, closed: bool, current: f32) -> Vec<f32> {
        let mut applied = Vec::new();
        while let Some(level) = dimmer.step(closed, || Some(current)) {
            applied.push(level);
            assert!(applied.len() < 100, "fade never settled");
        }
        applied
    }

    #[test]
    fn fades_to_black_then_back_to_where_it_was() {
        let mut dimmer = Dimmer::default();
        let down = run(&mut dimmer, true, 0.3125);
        assert_eq!(down.last(), Some(&0.0));
        assert!(down.windows(2).all(|w| w[1] < w[0]), "smooth, always downwards");
        assert!(down.len() >= 24);
        assert_eq!(dimmer.original(), Some(0.3125));

        // The screen reads 0.0 by now; the fade must come back to the remembered level, not that.
        let up = run(&mut dimmer, false, 0.0);
        assert_eq!(up.last(), Some(&0.3125));
        assert!(up.windows(2).all(|w| w[1] > w[0]), "smooth, always upwards");
        assert_eq!(dimmer.original(), None);
    }

    #[test]
    fn reopening_mid_fade_returns_to_the_original() {
        let mut dimmer = Dimmer::default();
        for _ in 0..5 {
            dimmer.step(true, || Some(0.8));
        }
        assert_eq!(run(&mut dimmer, false, 0.5).last(), Some(&0.8));
    }

    #[test]
    fn does_nothing_while_the_lid_stays_open() {
        let mut dimmer = Dimmer::default();
        assert_eq!(dimmer.step(false, || panic!("brightness is not read with the lid open")), None);
    }

    #[test]
    fn waits_when_brightness_cannot_be_read() {
        let mut dimmer = Dimmer::default();
        assert_eq!(dimmer.step(true, || None), None);
        assert_eq!(dimmer.original(), None);
    }

    /// Touches the real screen, so it only runs on request: `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs a MacBook with its lid open; dips the screen brightness briefly"]
    fn hardware_reads_the_lid_and_moves_the_backlight() {
        assert_eq!(closed(), Some(false));
        let before = brightness().expect("built-in display brightness");
        let dipped = (before - 0.1).max(0.0);
        set_brightness(dipped);
        let seen = brightness().expect("brightness after dip");
        set_brightness(before);
        assert!((seen - dipped).abs() < 0.02, "asked for {dipped}, screen reports {seen}");
        assert!((brightness().unwrap() - before).abs() < 0.02, "brightness was restored");
    }

    #[test]
    fn a_screen_already_at_zero_stays_there() {
        let mut dimmer = Dimmer::default();
        assert!(run(&mut dimmer, true, 0.0).is_empty());
        assert!(run(&mut dimmer, false, 0.0).is_empty());
        assert_eq!(dimmer.original(), None);
    }
}
