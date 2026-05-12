// Copyright 2022-2022 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

mod icon;
use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::{define_class, msg_send, AllocAnyThread, DeclaredClass, Message};
use objc2_app_kit::{
    NSCellImagePosition, NSEvent, NSImage, NSMenu, NSStatusBar, NSStatusItem, NSTrackingArea,
    NSTrackingAreaOptions, NSVariableStatusItemLength, NSView, NSWindow,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGDisplayPixelsHigh, CGMainDisplayID};
use objc2_foundation::{MainThreadMarker, NSData, NSSize, NSString};

pub(crate) use self::icon::PlatformIcon;
use crate::Error;
use crate::{
    icon::Icon, menu, MouseButton, MouseButtonState, Rect, TrayIconAttributes, TrayIconEvent,
    TrayIconId,
};

pub struct TrayIcon {
    ns_status_item: Option<Retained<NSStatusItem>>,
    tray_target: Option<Retained<TrayTarget>>,
    id: TrayIconId,
    attrs: TrayIconAttributes,
    mtm: MainThreadMarker,
}

impl TrayIcon {
    pub fn new(id: TrayIconId, attrs: TrayIconAttributes) -> crate::Result<Self> {
        let mtm = MainThreadMarker::new().ok_or(Error::NotMainThread)?;
        let (ns_status_item, tray_target) = Self::create(&id, &attrs, mtm)?;

        let tray_icon = Self {
            ns_status_item: Some(ns_status_item),
            tray_target: Some(tray_target),
            id,
            attrs,
            mtm,
        };

        Ok(tray_icon)
    }

    fn create(
        id: &TrayIconId,
        attrs: &TrayIconAttributes,
        mtm: MainThreadMarker,
    ) -> crate::Result<(Retained<NSStatusItem>, Retained<TrayTarget>)> {
        let ns_status_item = unsafe {
            NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength)
        };

        set_icon_for_ns_status_item_button(
            &ns_status_item,
            attrs.icon.clone(),
            attrs.icon_is_template,
            mtm,
        )?;

        if let Some(menu) = &attrs.menu {
            unsafe {
                ns_status_item.setMenu((menu.ns_menu() as *const NSMenu).as_ref());
            }
        }

        Self::set_tooltip_inner(&ns_status_item, attrs.tooltip.clone(), mtm)?;
        Self::set_title_inner(&ns_status_item, attrs.title.clone(), mtm);

        let tray_target = unsafe {
            let button = ns_status_item.button(mtm).unwrap();

            let frame = button.frame();

            let target = mtm.alloc().set_ivars(TrayTargetIvars {
                id: NSString::from_str(&id.0),
                menu: RefCell::new(
                    attrs
                        .menu
                        .as_deref()
                        .and_then(|menu| Retained::retain(menu.ns_menu().cast::<NSMenu>())),
                ),
                status_item: ns_status_item.retain(),
                menu_on_left_click: Cell::new(attrs.menu_on_left_click),
            });
            let tray_target: Retained<TrayTarget> = msg_send![super(target), initWithFrame: frame];
            tray_target.setWantsLayer(true);

            button.addSubview(&tray_target);

            tray_target
        };

        Ok((ns_status_item, tray_target))
    }

    fn remove(&mut self) {
        if let (Some(ns_status_item), Some(tray_target)) = (&self.ns_status_item, &self.tray_target)
        {
            unsafe {
                NSStatusBar::systemStatusBar().removeStatusItem(ns_status_item);
                tray_target.removeFromSuperview();
            }
        }

        self.ns_status_item = None;
        self.tray_target = None;
    }

    pub fn set_icon(&mut self, icon: Option<Icon>) -> crate::Result<()> {
        if let (Some(ns_status_item), Some(tray_target)) = (&self.ns_status_item, &self.tray_target)
        {
            set_icon_for_ns_status_item_button(ns_status_item, icon.clone(), false, self.mtm)?;
            tray_target.update_dimensions();
        }
        self.attrs.icon = icon;
        Ok(())
    }

    pub fn set_menu(&mut self, menu: Option<Box<dyn menu::ContextMenu>>) {
        if let (Some(ns_status_item), Some(tray_target)) = (&self.ns_status_item, &self.tray_target)
        {
            unsafe {
                let menu = menu
                    .as_ref()
                    .and_then(|m| m.ns_menu().cast::<NSMenu>().as_ref())
                    .map(|menu| menu.retain());
                ns_status_item.setMenu(menu.as_deref());
                if let Some(menu) = &menu {
                    let () = msg_send![menu, setDelegate: &**ns_status_item];
                }

                *tray_target.ivars().menu.borrow_mut() = menu;
            }
        }
        self.attrs.menu = menu;
    }

    pub fn set_tooltip<S: AsRef<str>>(&mut self, tooltip: Option<S>) -> crate::Result<()> {
        let tooltip = tooltip.map(|s| s.as_ref().to_string());
        if let (Some(ns_status_item), Some(tray_target)) = (&self.ns_status_item, &self.tray_target)
        {
            Self::set_tooltip_inner(ns_status_item, tooltip.clone(), self.mtm)?;
            tray_target.update_dimensions();
        }
        self.attrs.tooltip = tooltip;
        Ok(())
    }

    fn set_tooltip_inner<S: AsRef<str>>(
        ns_status_item: &NSStatusItem,
        tooltip: Option<S>,
        mtm: MainThreadMarker,
    ) -> crate::Result<()> {
        unsafe {
            let tooltip = tooltip.map(|tooltip| NSString::from_str(tooltip.as_ref()));
            if let Some(button) = ns_status_item.button(mtm) {
                button.setToolTip(tooltip.as_deref());
            }
        }
        Ok(())
    }

    pub fn set_title<S: AsRef<str>>(&mut self, title: Option<S>) {
        let title = title.map(|s| s.as_ref().to_string());
        if let (Some(ns_status_item), Some(tray_target)) = (&self.ns_status_item, &self.tray_target)
        {
            Self::set_title_inner(ns_status_item, title.clone(), self.mtm);
            tray_target.update_dimensions();
        }
        self.attrs.title = title;
    }

    fn set_title_inner<S: AsRef<str>>(
        ns_status_item: &NSStatusItem,
        title: Option<S>,
        mtm: MainThreadMarker,
    ) {
        if let Some(title) = title {
            unsafe {
                if let Some(button) = ns_status_item.button(mtm) {
                    button.setTitle(&NSString::from_str(title.as_ref()));
                }
            }
        }
    }

    pub fn set_visible(&mut self, visible: bool) -> crate::Result<()> {
        if visible {
            if self.ns_status_item.is_none() {
                let (ns_status_item, tray_target) = Self::create(&self.id, &self.attrs, self.mtm)?;
                self.ns_status_item = Some(ns_status_item);
                self.tray_target = Some(tray_target);
            }
        } else {
            self.remove();
        }

        Ok(())
    }

    pub fn set_icon_as_template(&mut self, is_template: bool) {
        if let Some(ns_status_item) = &self.ns_status_item {
            unsafe {
                let button = ns_status_item.button(self.mtm).unwrap();
                if let Some(nsimage) = button.image() {
                    nsimage.setTemplate(is_template);
                    button.setImage(Some(&nsimage));
                }
            }
        }
        self.attrs.icon_is_template = is_template;
    }

    pub fn set_icon_with_as_template(
        &mut self,
        icon: Option<Icon>,
        is_template: bool,
    ) -> crate::Result<()> {
        if let (Some(ns_status_item), Some(tray_target)) = (&self.ns_status_item, &self.tray_target)
        {
            set_icon_for_ns_status_item_button(
                ns_status_item,
                icon.clone(),
                is_template,
                self.mtm,
            )?;
            tray_target.update_dimensions();
        }
        self.attrs.icon = icon;
        self.attrs.icon_is_template = is_template;
        Ok(())
    }

    pub fn set_show_menu_on_left_click(&mut self, enable: bool) {
        if let Some(tray_target) = &self.tray_target {
            tray_target.ivars().menu_on_left_click.set(enable);
        }
        self.attrs.menu_on_left_click = enable;
    }

    pub fn rect(&self) -> Option<Rect> {
        let ns_status_item = self.ns_status_item.as_deref()?;
        unsafe {
            let button = ns_status_item.button(self.mtm).unwrap();
            let window = button.window();
            window.map(|window| get_tray_rect(&window))
        }
    }

    pub fn ns_status_item(&self) -> Option<&Retained<NSStatusItem>> {
        self.ns_status_item.as_ref()
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        self.remove()
    }
}

fn set_icon_for_ns_status_item_button(
    ns_status_item: &NSStatusItem,
    icon: Option<Icon>,
    icon_is_template: bool,
    mtm: MainThreadMarker,
) -> crate::Result<()> {
    let button = unsafe { ns_status_item.button(mtm).unwrap() };

    if let Some(icon) = icon {
        let png_icon = icon.inner.to_png()?;

        let (width, height) = icon.inner.get_size();

        let icon_height: f64 = 18.0;
        let icon_width: f64 = (width as f64) / (height as f64 / icon_height);

        unsafe {
            // build our icon
            let nsdata = NSData::from_vec(png_icon);

            let nsimage = NSImage::initWithData(NSImage::alloc(), &nsdata).unwrap();
            let new_size = NSSize::new(icon_width, icon_height);

            button.setImage(Some(&nsimage));
            nsimage.setSize(new_size);
            // The image is to the right of the title
            button.setImagePosition(NSCellImagePosition::ImageLeft);
            nsimage.setTemplate(icon_is_template);
        }
    } else {
        unsafe { button.setImage(None) };
    }

    Ok(())
}

#[derive(Debug)]
struct TrayTargetIvars {
    id: Retained<NSString>,
    menu: RefCell<Option<Retained<NSMenu>>>,
    status_item: Retained<NSStatusItem>,
    menu_on_left_click: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "TaoTrayTarget"]
    #[ivars = TrayTargetIvars]
    struct TrayTarget;

    /// Mouse events on NSResponder
    impl TrayTarget {
        #[unsafe(method(mouseDown:))]
        fn on_mouse_down(&self, event: &NSEvent) {
            send_mouse_event(
                self,
                event,
                MouseEventType::Click,
                Some(MouseClickEvent {
                    button: MouseButton::Left,
                    state: MouseButtonState::Down,
                }),
            );
            on_tray_click(self, MouseButton::Left);
        }

        #[unsafe(method(mouseUp:))]
        fn on_mouse_up(&self, event: &NSEvent) {
            let mtm = MainThreadMarker::from(self);
            unsafe {
                let button = self.ivars().status_item.button(mtm).unwrap();
                button.highlight(false);
            }
            send_mouse_event(
                self,
                event,
                MouseEventType::Click,
                Some(MouseClickEvent {
                    button: MouseButton::Left,
                    state: MouseButtonState::Up,
                }),
            );
        }

        #[unsafe(method(rightMouseDown:))]
        fn on_right_mouse_down(&self, event: &NSEvent) {
            send_mouse_event(
                self,
                event,
                MouseEventType::Click,
                Some(MouseClickEvent {
                    button: MouseButton::Right,
                    state: MouseButtonState::Down,
                }),
            );
            on_tray_click(self, MouseButton::Right);
        }

        #[unsafe(method(rightMouseUp:))]
        fn on_right_mouse_up(&self, event: &NSEvent) {
            send_mouse_event(
                self,
                event,
                MouseEventType::Click,
                Some(MouseClickEvent {
                    button: MouseButton::Right,
                    state: MouseButtonState::Up,
                }),
            );
        }

        #[unsafe(method(otherMouseDown:))]
        fn on_other_mouse_down(&self, event: &NSEvent) {
            let button_number = unsafe { event.buttonNumber() };
            if button_number == 2 {
                send_mouse_event(
                    self,
                    event,
                    MouseEventType::Click,
                    Some(MouseClickEvent {
                        button: MouseButton::Middle,
                        state: MouseButtonState::Down,
                    }),
                );
            }
        }

        #[unsafe(method(otherMouseUp:))]
        fn on_other_mouse_up(&self, event: &NSEvent) {
            let button_number = unsafe { event.buttonNumber() };
            if button_number == 2 {
                send_mouse_event(
                    self,
                    event,
                    MouseEventType::Click,
                    Some(MouseClickEvent {
                        button: MouseButton::Middle,
                        state: MouseButtonState::Up,
                    }),
                );
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn on_mouse_entered(&self, event: &NSEvent) {
            send_mouse_event(self, event, MouseEventType::Enter, None);
        }

        #[unsafe(method(mouseExited:))]
        fn on_mouse_exited(&self, event: &NSEvent) {
            send_mouse_event(self, event, MouseEventType::Leave, None);
        }

        #[unsafe(method(mouseMoved:))]
        fn on_mouse_moved(&self, event: &NSEvent) {
            send_mouse_event(self, event, MouseEventType::Move, None);
        }
    }

    /// Tracking mouse enter/exit/move events
    impl TrayTarget {
        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            unsafe {
                let areas = self.trackingAreas();
                for area in areas {
                    self.removeTrackingArea(&area);
                }

                let _: () = msg_send![super(self), updateTrackingAreas];

                let options = NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::MouseMoved
                    | NSTrackingAreaOptions::ActiveAlways
                    | NSTrackingAreaOptions::InVisibleRect;
                let rect = CGRect {
                    origin: CGPoint { x: 0.0, y: 0.0 },
                    size: CGSize {
                        width: 0.0,
                        height: 0.0,
                    },
                };
                let area = NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    rect,
                    options,
                    Some(self),
                    None,
                );
                self.addTrackingArea(&area);
            }
        }
    }
);

impl TrayTarget {
    fn update_dimensions(&self) {
        let mtm = MainThreadMarker::from(self);
        unsafe {
            let button = self.ivars().status_item.button(mtm).unwrap();
            self.setFrame(button.frame());
        }
    }
}

// Local patch for tauri-apps/tray-icon#251: popping the menu via
// `performClick(None)` synthesizes a click without an NSEvent context, so
// AppKit's status-item popup logic loses screen/Space information and silently
// no-ops on a secondary display while a full-screen Space is active there.
// Drive the popup ourselves through NSStatusItem.popUpStatusItemMenu, which is
// deprecated but functionally intact across every supported macOS version and
// works in every monitor/Space combination.
//
// `popUpStatusItemMenu` is modal — it does not return until the menu is
// dismissed. A menu item action selected inside that modal can re-enter
// `TrayIcon::set_menu`, which calls `menu.borrow_mut()` on the same RefCell,
// so we must not hold a `RefCell::borrow()` across the popup. Clone the
// `Retained<NSMenu>` (an ObjC retain) and drop the borrow before popping up.
#[allow(deprecated)]
fn on_tray_click(this: &TrayTarget, button: MouseButton) {
    let mtm = MainThreadMarker::from(this);
    unsafe {
        let ns_button = this.ivars().status_item.button(mtm).unwrap();

        let menu_on_left_click = this.ivars().menu_on_left_click.get();
        if button == MouseButton::Right || (menu_on_left_click && button == MouseButton::Left) {
            let menu = this.ivars().menu.borrow().as_ref().map(Retained::clone);
            if let Some(menu) = menu {
                if menu.numberOfItems() > 0 {
                    ns_button.highlight(true);
                    this.ivars().status_item.popUpStatusItemMenu(&menu);
                    ns_button.highlight(false);
                    return;
                }
            }
            ns_button.highlight(true);
        } else {
            ns_button.highlight(true);
        }
    }
}

fn get_tray_rect(window: &NSWindow) -> Rect {
    let frame = window.frame();
    let scale_factor = window.backingScaleFactor();

    Rect {
        size: crate::dpi::LogicalSize::new(frame.size.width, frame.size.height)
            .to_physical(scale_factor),
        position: crate::dpi::LogicalPosition::new(
            frame.origin.x,
            flip_window_screen_coordinates(frame.origin.y) - frame.size.height,
        )
        .to_physical(scale_factor),
    }
}

fn send_mouse_event(
    this: &TrayTarget,
    event: &NSEvent,
    mouse_event_type: MouseEventType,
    click_event: Option<MouseClickEvent>,
) {
    let mtm = MainThreadMarker::from(this);
    unsafe {
        let tray_id = TrayIconId(this.ivars().id.to_string());

        // icon position & size
        let window = event.window(mtm).unwrap();
        let icon_rect = get_tray_rect(&window);

        // cursor position
        let mouse_location = NSEvent::mouseLocation();
        let scale_factor = window.backingScaleFactor();
        let cursor_position = crate::dpi::LogicalPosition::new(
            mouse_location.x,
            flip_window_screen_coordinates(mouse_location.y),
        )
        .to_physical(scale_factor);

        let event = match mouse_event_type {
            MouseEventType::Click => {
                let click_event = click_event.unwrap();
                TrayIconEvent::Click {
                    id: tray_id,
                    position: cursor_position,
                    rect: icon_rect,
                    button: click_event.button,
                    button_state: click_event.state,
                }
            }
            MouseEventType::Enter => TrayIconEvent::Enter {
                id: tray_id,
                position: cursor_position,
                rect: icon_rect,
            },
            MouseEventType::Leave => TrayIconEvent::Leave {
                id: tray_id,
                position: cursor_position,
                rect: icon_rect,
            },
            MouseEventType::Move => TrayIconEvent::Move {
                id: tray_id,
                position: cursor_position,
                rect: icon_rect,
            },
        };

        TrayIconEvent::send(event);
    }
}

#[derive(Debug)]
enum MouseEventType {
    Click,
    Enter,
    Leave,
    Move,
}

#[derive(Debug)]
struct MouseClickEvent {
    button: MouseButton,
    state: MouseButtonState,
}

/// Core graphics screen coordinates are relative to the top-left corner of
/// the so-called "main" display, with y increasing downwards - which is
/// exactly what we want in Winit.
///
/// However, `NSWindow` and `NSScreen` changes these coordinates to:
/// 1. Be relative to the bottom-left corner of the "main" screen.
/// 2. Be relative to the bottom-left corner of the window/screen itself.
/// 3. Have y increasing upwards.
///
/// This conversion happens to be symmetric, so we only need this one function
/// to convert between the two coordinate systems.
fn flip_window_screen_coordinates(y: f64) -> f64 {
    unsafe { CGDisplayPixelsHigh(CGMainDisplayID()) as f64 - y }
}
