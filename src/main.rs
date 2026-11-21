//! Lidless: one click in the menu bar to stop your Mac sleeping, even with the lid shut.
//!   Open eye   -> `pmset -a disablesleep 1` (never sleeps)
//!   Closed eye -> `pmset -a disablesleep 0` (normal sleep)
//! Left-click toggles. Right-click opens the menu.

mod lid;
mod power;

use std::cell::{Cell, OnceCell, RefCell};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBeep, NSBezierPath,
    NSColor, NSControlStateValueOff, NSControlStateValueOn, NSEventMask, NSEventType,
    NSFontWeightRegular, NSImage, NSImageSymbolConfiguration, NSLineCapStyle, NSMenu, NSMenuItem,
    NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{
    NSBundle, NSError, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSString, NSTimer, NSUUID, NSUserDefaults, ns_string,
};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNNotificationSound, UNUserNotificationCenter,
};

/// Stored inverted so the safety cutoff is on until someone turns it off.
const AUTO_OFF_DISABLED: &str = "autoOffDisabled";

fn auto_off() -> bool {
    !NSUserDefaults::standardUserDefaults().boolForKey(&NSString::from_str(AUTO_OFF_DISABLED))
}

fn opens_at_login() -> bool {
    unsafe { SMAppService::mainAppService().status() == SMAppServiceStatus::Enabled }
}

/// Notifications need a bundle identifier; a bare `cargo run` binary has none.
fn notification_center() -> Option<Retained<UNUserNotificationCenter>> {
    NSBundle::mainBundle().bundleIdentifier()?;
    Some(UNUserNotificationCenter::currentNotificationCenter())
}

fn notify(title: &str, body: &str) {
    let Some(center) = notification_center() else { return };
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSUUID::new().UUIDString(),
        &content,
        None,
    );
    center.addNotificationRequest_withCompletionHandler(&request, None);
}

fn icon(awake: bool) -> Retained<NSImage> {
    let config = NSImageSymbolConfiguration::configurationWithPointSize_weight(14.0, unsafe {
        NSFontWeightRegular
    });
    let open = NSImage::imageWithSystemSymbolName_accessibilityDescription(ns_string!("eye"), None)
        .and_then(|img| img.imageWithSymbolConfiguration(&config))
        .expect("eye is a system symbol on macOS 13+");
    let image = if awake {
        open
    } else {
        // SF Symbols has no closed eye, so draw one in the open eye's box: a lid and three
        // lashes, laid out on a 21x13 grid.
        let draw = RcBlock::new(|rect: NSRect| {
            let at = |x: f64, y: f64| NSPoint::new(x * rect.size.width / 21.0, y * rect.size.height / 13.0);
            let eye = NSBezierPath::bezierPath();
            eye.moveToPoint(at(2.5, 8.5));
            eye.curveToPoint_controlPoint1_controlPoint2(at(18.5, 8.5), at(6.5, 3.0), at(14.5, 3.0));
            for (root, tip) in [((10.5, 4.4), (10.5, 1.4)), ((5.3, 5.9), (3.7, 3.3)), ((15.7, 5.9), (17.3, 3.3))] {
                eye.moveToPoint(at(root.0, root.1));
                eye.lineToPoint(at(tip.0, tip.1));
            }
            eye.setLineWidth(1.5);
            eye.setLineCapStyle(NSLineCapStyle::Round);
            NSColor::blackColor().setStroke();
            eye.stroke();
            Bool::YES
        });
        NSImage::imageWithSize_flipped_drawingHandler(open.size(), false, &draw)
    };
    image.setTemplate(true);
    image
}

struct Ivars {
    item: OnceCell<Retained<NSStatusItem>>,
    /// None until the first refresh has drawn the icon.
    awake: Cell<Option<bool>>,
    dimmer: RefCell<lid::Dimmer>,
    /// True while the fast fade timer is running.
    fading: Cell<bool>,
}

impl Ivars {
    fn awake(&self) -> bool {
        self.awake.get() == Some(true)
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "LidlessApp"]
    #[ivars = Ivars]
    struct App;

    unsafe impl NSObjectProtocol for App {}

    unsafe impl NSApplicationDelegate for App {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _: &NSNotification) {
            let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
            if let Some(button) = item.button(self.mtm()) {
                unsafe {
                    button.setTarget(Some(self));
                    button.setAction(Some(sel!(clicked:)));
                }
                button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
            }
            self.ivars().item.set(item).expect("launched once");
            if let Some(center) = notification_center() {
                center.requestAuthorizationWithOptions_completionHandler(
                    UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
                    &RcBlock::new(|_: Bool, _: *mut NSError| {}),
                );
            }
            self.refresh();
            // Polling also covers thermal changes: the cutoff reacts within five seconds.
            self.every(5.0, sel!(tick:));
            self.every(0.5, sel!(lidTick:));
        }

        /// The safety cutoff only runs while Lidless does, so quitting must not leave the Mac
        /// unable to sleep, nor the screen black.
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _: &NSNotification) {
            if let Some(original) = self.ivars().dimmer.borrow().original() {
                lid::set_brightness(original);
            }
            if power::sleep_disabled() {
                power::allow_sleep_quietly();
            }
        }
    }

    impl App {
        #[unsafe(method(tick:))]
        fn tick(&self, _: Option<&AnyObject>) {
            self.refresh();
        }

        /// Slow poll for the lid. Hands over to the fast timer for the length of a fade.
        #[unsafe(method(lidTick:))]
        fn lid_tick(&self, _: Option<&AnyObject>) {
            if !self.ivars().fading.get() && self.fade_step() {
                self.ivars().fading.set(true);
                self.every(1.0 / 30.0, sel!(fadeTick:));
            }
        }

        #[unsafe(method(fadeTick:))]
        fn fade_tick(&self, timer: &NSTimer) {
            if !self.fade_step() {
                timer.invalidate();
                self.ivars().fading.set(false);
            }
        }

        #[unsafe(method(clicked:))]
        fn clicked(&self, _: Option<&AnyObject>) {
            let event = NSApplication::sharedApplication(self.mtm()).currentEvent();
            if event.is_some_and(|e| e.r#type() == NSEventType::RightMouseUp) {
                self.show_menu();
            } else {
                self.toggle();
            }
        }

        #[unsafe(method(toggle:))]
        fn toggle_action(&self, _: Option<&AnyObject>) {
            self.toggle();
        }

        #[unsafe(method(toggleAutoOff:))]
        fn toggle_auto_off(&self, _: Option<&AnyObject>) {
            NSUserDefaults::standardUserDefaults()
                .setBool_forKey(auto_off(), &NSString::from_str(AUTO_OFF_DISABLED));
        }

        #[unsafe(method(toggleLogin:))]
        fn toggle_login(&self, _: Option<&AnyObject>) {
            let service = unsafe { SMAppService::mainAppService() };
            let result = if opens_at_login() {
                unsafe { service.unregisterAndReturnError() }
            } else {
                unsafe { service.registerAndReturnError() }
            };
            if result.is_err() {
                NSBeep();
            }
        }

        /// Removes the password-free rule (what you'd run before deleting the app).
        #[unsafe(method(removeRule:))]
        fn remove_rule(&self, _: Option<&AnyObject>) {
            if !power::remove_rule() {
                NSBeep();
            }
        }
    }
);

impl App {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars { item: OnceCell::new(), awake: Cell::new(None),
            dimmer: RefCell::default(),
            fading: Cell::new(false),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn every(&self, seconds: f64, action: Sel) {
        unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                seconds, self, action, None, true,
            );
        }
    }

    /// Moves the screen one step towards black (lid shut while staying awake) or back towards
    /// its remembered brightness. Returns false once it has arrived.
    fn fade_step(&self) -> bool {
        let closed = self.ivars().awake() && lid::closed() == Some(true);
        let level = self.ivars().dimmer.borrow_mut().step(closed, lid::brightness);
        level.inspect(|&level| lid::set_brightness(level)).is_some()
    }

    fn toggle(&self) {
        let awake = self.ivars().awake();
        if !awake
            && auto_off()
            && let Some(why) = power::safety_reason()
        {
            notify("Lidless can't switch on", &format!("{why}. Plug in or let it cool down first."));
            NSBeep();
            return;
        }
        if !power::set_sleep_disabled(!awake) {
            NSBeep();
        }
        self.refresh();
    }

    fn refresh(&self) {
        let mut awake = power::sleep_disabled();
        // Safety cutoff: a hot Mac or a low battery in a closed bag is how batteries get cooked.
        if awake
            && auto_off()
            && let Some(why) = power::safety_reason()
            && power::set_sleep_disabled(false)
        {
            awake = false;
            notify("Lidless switched off", &format!("{why}, so your Mac can sleep normally again."));
        }
        // Redraw only on change, not on every poll.
        if self.ivars().awake.replace(Some(awake)) == Some(awake) {
            return;
        }
        let Some(button) = self.ivars().item.get().and_then(|item| item.button(self.mtm())) else { return };
        button.setImage(Some(&icon(awake)));
        button.setToolTip(Some(if awake {
            ns_string!("Lidless: sleep is OFF. Your Mac stays awake, even with the lid shut. Click to allow sleep.")
        } else {
            ns_string!("Lidless: normal sleep. Click to keep your Mac awake.")
        }));
    }

    fn add_item(&self, menu: &NSMenu, title: &str, action: Option<Sel>, key: &str) -> Retained<NSMenuItem> {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                self.mtm().alloc(),
                &NSString::from_str(title),
                action,
                &NSString::from_str(key),
            )
        };
        unsafe { item.setTarget(Some(self)) };
        menu.addItem(&item);
        item
    }

    fn show_menu(&self) {
        let mtm = self.mtm();
        let awake = self.ivars().awake();
        let on_off = |on: bool| if on { NSControlStateValueOn } else { NSControlStateValueOff };
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);

        let status = if awake { "Sleep is off: stays awake with the lid shut" } else { "Normal sleep" };
        self.add_item(&menu, status, None, "").setEnabled(false);
        self.add_item(&menu, if awake { "Allow Sleep" } else { "Keep Awake" }, Some(sel!(toggle:)), "");
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let cutoff = format!("Auto-off When Hot or Below {}% Battery", power::LOW_BATTERY);
        self.add_item(&menu, &cutoff, Some(sel!(toggleAutoOff:)), "").setState(on_off(auto_off()));
        self.add_item(&menu, "Open at Login", Some(sel!(toggleLogin:)), "")
            .setState(on_off(opens_at_login()));
        if power::password_free() {
            self.add_item(&menu, "Ask for Password Again…", Some(sel!(removeRule:)), "");
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let quit = self.add_item(&menu, "Quit Lidless", Some(sel!(terminate:)), "q");
        unsafe { quit.setTarget(None) };

        let Some(item) = self.ivars().item.get() else { return };
        item.setMenu(Some(&menu));
        if let Some(button) = item.button(mtm) {
            unsafe { button.performClick(None) };
        }
        item.setMenu(None);
    }
}

fn main() {
    let mtm = MainThreadMarker::new().expect("main() runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let delegate = App::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
