//! The macOS menu bar. The application menu's About item, which winit points at the system's
//! standard about panel, opens the application's own about box instead: the system panel knows the
//! application only through its bundle, and shows a generic icon and the process name when it runs
//! without one. A File menu beside it carries the items of the window's File menu (PRD 9.11),
//! which the window then does not draw.

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

/// The window's context, to ask it for the about box when the item is chosen.
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and the class adds no ivars and no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "OscqInspectorAboutTarget"]
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

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and the class adds no ivars and no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "OscqInspectorFileTarget"]
    struct FileTarget;

    impl FileTarget {
        // SAFETY: the signature of an action method: one object argument, no return value.
        #[unsafe(method(fileItem:))]
        fn file_item(&self, sender: Option<&NSMenuItem>) {
            let job = sender
                .and_then(|item| usize::try_from(item.tag()).ok())
                .and_then(|i| crate::app::FileJob::ALL.get(i).copied());
            if let (Some(ctx), Some(job)) = (CONTEXT.get(), job) {
                crate::app::request_file(ctx, job);
            }
        }
    }
);

impl FileTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: `init` of NSObject, on a freshly allocated instance.
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

impl AboutTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: `init` of NSObject, on a freshly allocated instance.
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

/// Points the About item of the application menu at the application's about box and names it
/// after the application, and puts the File menu beside it. Returns false while the menu does
/// not exist yet, true once done.
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
    about.setTitle(&NSString::from_str(&format!(
        "About {}",
        crate::app::APP_NAME
    )));
    std::mem::forget(target);
    install_file_menu(mtm, &menu);
    true
}

/// The File menu, second in the menu bar: Save Project and Open Project with their keys, then
/// Export and Import. Each item carries its place in `FileJob::ALL` as its tag.
fn install_file_menu(mtm: MainThreadMarker, bar: &NSMenu) {
    use crate::app::FileJob;
    let target = FileTarget::new(mtm);
    let item = |job: FileJob, key: &str| {
        let tag = FileJob::ALL.iter().position(|j| *j == job).unwrap_or(0);
        // SAFETY: the selector is the one the target answers, `fileItem:`.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(job.label()),
                Some(sel!(fileItem:)),
                &NSString::from_str(key),
            )
        };
        item.setTag(tag as isize);
        // SAFETY: the target answers the item's action; it lives for the process, below.
        unsafe { item.setTarget(Some(&target)) };
        item
    };
    let submenu = |title: &str, jobs: &[FileJob]| {
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
        for &job in jobs {
            menu.addItem(&item(job, ""));
        }
        // SAFETY: an item with no action, holding the submenu.
        let holder = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                None,
                &NSString::from_str(""),
            )
        };
        holder.setSubmenu(Some(&menu));
        holder
    };
    let file = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("File"));
    file.addItem(&item(FileJob::SaveProject, "s"));
    file.addItem(&item(FileJob::OpenProject, "o"));
    file.addItem(&NSMenuItem::separatorItem(mtm));
    file.addItem(&submenu(
        "Export",
        &[
            FileJob::ExportServers,
            FileJob::ExportDocument,
            FileJob::ExportTraffic,
            FileJob::ExportPresets,
        ],
    ));
    file.addItem(&submenu(
        "Import",
        &[FileJob::ImportServers, FileJob::ImportPresets],
    ));
    // SAFETY: as above, an item with no action, holding the File menu.
    let holder = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("File"),
            None,
            &NSString::from_str(""),
        )
    };
    holder.setSubmenu(Some(&file));
    let at = bar.numberOfItems().min(1);
    bar.insertItem_atIndex(&holder, at);
    // A menu item holds its target weakly.
    std::mem::forget(target);
}
