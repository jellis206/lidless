//! Lidless: one click in the menu bar to stop your Mac sleeping, even with the lid shut.
//!   Moon Zzz, struck through -> `pmset -a disablesleep 1` (never sleeps)
//!   Moon Zzz                 -> `pmset -a disablesleep 0` (normal sleep)
//! Left-click toggles. Right-click opens the menu.

mod power;

use std::cell::{Cell, OnceCell};

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
    NSBundle, NSError, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, NSTimer, NSUUID, NSUserDefaults, ns_string,
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
    let base = NSImage::imageWithSystemSymbolName_accessibilityDescription(ns_string!("moon.zzz"), None)
        .and_then(|img| img.imageWithSymbolConfiguration(&config))
        .expect("moon.zzz is a system symbol on macOS 13+");
    let image = if awake {
        let glyph = base.size();
        let size = NSSize::new(glyph.width.max(18.0), glyph.height.max(16.0));
        let draw = RcBlock::new(move |rect: NSRect| {
            base.drawInRect(NSRect::new(
                NSPoint::new((rect.size.width - glyph.width) / 2.0, (rect.size.height - glyph.height) / 2.0),
                glyph,
            ));
            let strike = NSBezierPath::bezierPath();
            strike.moveToPoint(NSPoint::new(1.5, 1.5));
            strike.lineToPoint(NSPoint::new(rect.size.width - 1.5, rect.size.height - 1.5));
            strike.setLineWidth(1.8);
            strike.setLineCapStyle(NSLineCapStyle::Round);
            NSColor::blackColor().setStroke();
            strike.stroke();
            Bool::YES
        });
        NSImage::imageWithSize_flipped_drawingHandler(size, false, &draw)
    } else {
        base
    };
    image.setTemplate(true);
    image
}

struct Ivars {
    item: OnceCell<Retained<NSStatusItem>>,
    awake: Cell<bool>,
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
            unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    5.0,
                    self,
                    sel!(tick:),
                    None,
                    true,
                );
            }
        }

        /// The safety cutoff only runs while Lidless does, so quitting must not leave the Mac
        /// unable to sleep.
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _: &NSNotification) {
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
        let this = Self::alloc(mtm).set_ivars(Ivars { item: OnceCell::new(), awake: Cell::new(false) });
        unsafe { msg_send![super(this), init] }
    }

    fn toggle(&self) {
        let awake = self.ivars().awake.get();
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
        self.ivars().awake.set(awake);
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
        let awake = self.ivars().awake.get();
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
