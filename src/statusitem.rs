//! The menu bar item drawn by hand for the "icon + speed" and "speed
//! only" styles (ClashBar's `StatusItemContentView`): NSH's tray fits its
//! image in an 18-point square, too small for two lines of speed. Icon
//! only keeps NSH's tray.

use std::cell::RefCell;
use std::sync::OnceLock;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, ProtocolObject};
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSColor, NSEventMask, NSFont, NSFontWeightSemibold, NSImage, NSLineBreakMode, NSMutableParagraphStyle, NSScreen,
    NSStatusBar, NSStatusItem, NSStringDrawing, NSTextAlignment,
};
use objc2_app_kit::{NSFontAttributeName, NSForegroundColorAttributeName, NSParagraphStyleAttributeName};
use objc2_foundation::{NSAttributedStringKey, NSData, NSDictionary, NSPoint, NSRect, NSSize, NSString};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::geometry::Rect;

const ICON_SIZE: f64 = 24.0;
const TEXT_WIDTH: f64 = 43.0;
const LINE_HEIGHT: f64 = 11.0;
const FONT_SIZE: f64 = 10.0;

/// What the item shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    pub show_icon: bool,
    pub running: bool,
    pub tun: bool,
    pub up: String,
    pub down: String,
}

thread_local! {
    static ITEM: RefCell<Option<(Retained<NSStatusItem>, Retained<StatusTarget>)>> = const { RefCell::new(None) };
}

static CLICKS: OnceLock<UnboundedSender<Rect>> = OnceLock::new();

/// The clicks on the item, with its frame on the screen (top-left origin).
pub fn clicks() -> UnboundedReceiver<Rect> {
    let (tx, rx) = unbounded_channel();
    let _ = CLICKS.set(tx);
    rx
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MihobarStatusItemTarget"]
    struct StatusTarget;

    impl StatusTarget {
        #[unsafe(method(clicked:))]
        fn clicked(&self, _sender: Option<&AnyObject>) {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let frame = ITEM.with_borrow(|item| {
                let (item, _) = item.as_ref()?;
                let button = item.button(mtm)?;
                let window = button.window()?;
                Some(window.frame())
            });
            let Some(frame) = frame else { return };
            // AppKit's origin is the primary screen's bottom left.
            let primary_height = NSScreen::screens(mtm).firstObject().map_or(0.0, |s| s.frame().size.height);
            let rect = Rect {
                x: frame.origin.x as f32,
                y: (primary_height - frame.origin.y - frame.size.height) as f32,
                w: frame.size.width as f32,
                h: frame.size.height as f32,
            };
            if let Some(tx) = CLICKS.get() {
                let _ = tx.send(rect);
            }
        }
    }
);

impl StatusTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn icon_image(svg: &str) -> Option<Retained<NSImage>> {
    let data = NSData::with_bytes(svg.as_bytes());
    NSImage::initWithData(NSImage::alloc(), &data)
}

/// The item's image: the icon and the two speed lines, drawn as a
/// template at every scale.
fn compose(display: &Display, height: f64) -> Retained<NSImage> {
    let width = if display.show_icon { ICON_SIZE + TEXT_WIDTH } else { TEXT_WIDTH };
    let icon = display.show_icon.then(|| icon_image(&crate::icons::status_svg(display.running, display.tun))).flatten();
    let up = NSString::from_str(&display.up);
    let down = NSString::from_str(&display.down);
    let text_x = if display.show_icon { ICON_SIZE } else { 0.0 };
    let handler = RcBlock::new(move |bounds: NSRect| -> Bool {
        if let Some(icon) = &icon {
            let y = ((bounds.size.height - ICON_SIZE) / 2.0).floor();
            icon.drawInRect(NSRect::new(NSPoint::new(0.0, y), NSSize::new(ICON_SIZE, ICON_SIZE)));
        }
        unsafe {
            let font = NSFont::monospacedDigitSystemFontOfSize_weight(FONT_SIZE, NSFontWeightSemibold);
            let paragraph = NSMutableParagraphStyle::new();
            paragraph.setAlignment(NSTextAlignment::Right);
            paragraph.setLineBreakMode(NSLineBreakMode::ByTruncatingHead);
            let color = NSColor::blackColor();
            let keys: [&NSAttributedStringKey; 3] = [NSFontAttributeName, NSForegroundColorAttributeName, NSParagraphStyleAttributeName];
            let values: [&AnyObject; 3] = [&font, &color, &paragraph];
            let attributes = NSDictionary::from_slices(&keys, &values);
            let stack_y = ((bounds.size.height - LINE_HEIGHT * 2.0) / 2.0).floor();
            let up_rect = NSRect::new(NSPoint::new(text_x, stack_y + LINE_HEIGHT), NSSize::new(TEXT_WIDTH, LINE_HEIGHT));
            let down_rect = NSRect::new(NSPoint::new(text_x, stack_y), NSSize::new(TEXT_WIDTH, LINE_HEIGHT));
            up.drawInRect_withAttributes(up_rect, Some(&attributes));
            down.drawInRect_withAttributes(down_rect, Some(&attributes));
        }
        Bool::YES
    });
    let image = NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(width, height), false, &handler);
    image.setTemplate(true);
    image
}

/// Shows `display` in the item, making it first (main thread only).
pub fn show(display: &Display) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let bar = NSStatusBar::systemStatusBar();
    let height = bar.thickness();
    ITEM.with_borrow_mut(|slot| {
        if slot.is_none() {
            let item = bar.statusItemWithLength(-1.0);
            let target = StatusTarget::new(mtm);
            if let Some(button) = item.button(mtm) {
                unsafe {
                    button.setTarget(Some(&target));
                    button.setAction(Some(sel!(clicked:)));
                }
                button.sendActionOn(NSEventMask::LeftMouseUp);
            }
            *slot = Some((item, target));
        }
        if let Some((item, _)) = slot.as_ref() {
            if let Some(button) = item.button(mtm) {
                button.setImage(Some(&compose(display, height)));
            }
        }
    });
}

/// Removes the item (main thread only).
pub fn hide() {
    ITEM.with_borrow_mut(|slot| {
        if let Some((item, _)) = slot.take() {
            NSStatusBar::systemStatusBar().removeStatusItem(&item);
        }
    });
}

#[allow(dead_code)]
fn _assert_protocols(_: &ProtocolObject<dyn objc2_foundation::NSObjectProtocol>) {}
