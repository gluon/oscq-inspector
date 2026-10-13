//! The macOS application menu. Its About item, which winit points at the system's standard about
//! panel, opens the application's own about box instead: the system panel knows the application
//! only through its bundle, and shows a generic icon and the process name when it runs without one.

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::NSApplication;
use objc2_foundation::NSString;

/// The window's context, to ask it for the about box when the item is chosen.
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and the class adds no ivars and no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "VoidOscqAboutTarget"]
    struct AboutTarget;

    impl AboutTarget {
        // SAFETY: the signature of an action method: one object argument, no return value.
        #[unsafe(method(showAbout:))]
        fn show_about(&self, _sender: Option<&AnyObject>) {
            if let Some(ctx) = CONTEXT.get() {
                crate::app::request_about(ctx);
            }
        }
    }
);

impl AboutTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: `init` of NSObject, on a freshly allocated instance.
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

/// Points the About item of the application menu at the application's about box and names it
/// after the application. Returns false while the menu does not exist yet, true once done.
pub fn install(ctx: &egui::Context) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(menu) = NSApplication::sharedApplication(mtm).mainMenu() else {
        return false;
    };
    let Some(app_menu) = menu.itemAtIndex(0).and_then(|item| item.submenu()) else {
        return false;
    };
    let standard = sel!(orderFrontStandardAboutPanel:);
    let Some(about) = (0..app_menu.numberOfItems())
        .filter_map(|i| app_menu.itemAtIndex(i))
        .find(|item| item.action() == Some(standard))
    else {
        return false;
    };
    let _ = CONTEXT.set(ctx.clone());
    let target = AboutTarget::new(mtm);
    // SAFETY: the target answers the action set with it; it is kept alive below, for the life of
    // the process, since a menu item holds its target weakly.
    unsafe {
        about.setTarget(Some(&target));
        about.setAction(Some(sel!(showAbout:)));
    }
    about.setTitle(&NSString::from_str("About Void OSCQ"));
    std::mem::forget(target);
    true
}
