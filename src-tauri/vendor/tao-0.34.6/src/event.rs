// Copyright 2014-2021 The winit contributors
// Copyright 2021-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

//! The `Event` enum and assorted supporting types.
//!
//! These are sent to the closure given to [`EventLoop::run(...)`][event_loop_run], where they get
//! processed and used to modify the program state. For more details, see the root-level documentation.
//!
//! Some of these events represent different "parts" of a traditional event-handling loop. You could
//! approximate the basic ordering loop of [`EventLoop::run(...)`][event_loop_run] like this:
//!
//! ```rust,ignore
//! let mut control_flow = ControlFlow::Poll;
//! let mut start_cause = StartCause::Init;
//!
//! while control_flow != ControlFlow::Exit {
//!     event_handler(NewEvents(start_cause), ..., &mut control_flow);
//!
//!     for e in (window events, user events, device events) {
//!         event_handler(e, ..., &mut control_flow);
//!     }
//!     event_handler(MainEventsCleared, ..., &mut control_flow);
//!
//!     for w in (redraw windows) {
//!         event_handler(RedrawRequested(w), ..., &mut control_flow);
//!     }
//!     event_handler(RedrawEventsCleared, ..., &mut control_flow);
//!
//!     start_cause = wait_if_necessary(control_flow);
//! }
//!
//! event_handler(LoopDestroyed, ..., &mut control_flow);
//! ```
//!
//! This leaves out timing details like `ControlFlow::WaitUntil` but hopefully
//! describes what happens in what order.
//!
//! [event_loop_run]: crate::event_loop::EventLoop::run
use std::{path::PathBuf, time::Instant};

use crate::{
  dpi::{PhysicalPosition, PhysicalSize},
  keyboard::{self, ModifiersState},
  platform_impl,
  window::{Theme, WindowId},
};

/// Describes a generic event.
///
/// See the module-level docs for more information on the event loop manages each event.
#[non_exhaustive]
#[derive(Debug, PartialEq)]
pub enum Event<'a, T: 'static> {
  /// Emitted when new events arrive from the OS to be processed.
  ///
  /// This event type is useful as a place to put code that should be done before you start
  /// processing events, such as updating frame timing information for benchmarking or checking
  /// the [`StartCause`][crate::event::StartCause] to see if a timer set by
  /// [`ControlFlow::WaitUntil`](crate::event_loop::ControlFlow::WaitUntil) has elapsed.
  NewEvents(StartCause),

  /// Emitted when the OS sends an event to a tao window.
  #[non_exhaustive]
  WindowEvent {
    window_id: WindowId,
    event: WindowEvent<'a>,
  },

  /// Emitted when the OS sends an event to a device.
  #[non_exhaustive]
  DeviceEvent {
    device_id: DeviceId,
    event: DeviceEvent,
  },

  /// Emitted when an event is sent from [`EventLoopProxy::send_event`](crate::event_loop::EventLoopProxy::send_event)
  UserEvent(T),

  /// Emitted when the application has been suspended.
  Suspended,

  /// Emitted when the application has been resumed.
  Resumed,

  /// Emitted when all of the event loop's input events have been processed and redraw processing
  /// is about to begin.
  ///
  /// This event is useful as a place to put your code that should be run after all
  /// state-changing events have been handled and you want to do stuff (updating state, performing
  /// calculations, etc) that happens as the "main body" of your event loop. If your program only draws
  /// graphics when something changes, it's usually better to do it in response to
  /// [`Event::RedrawRequested`](crate::event::Event::RedrawRequested), which gets emitted
  /// immediately after this event. Programs that draw graphics continuously, like most games,
  /// can render here unconditionally for simplicity.
  MainEventsCleared,

  /// Emitted after `MainEventsCleared` when a window should be redrawn.
  ///
  /// This gets triggered in two scenarios:
  /// - The OS has performed an operation that's invalidated the window's contents (such as
  ///   resizing the window).
  /// - The application has explicitly requested a redraw via
  ///   [`Window::request_redraw`](crate::window::Window::request_redraw).
  ///
  /// During each iteration of the event loop, Tao will aggregate duplicate redraw requests
  /// into a single event, to help avoid duplicating rendering work.
  ///
  /// Mainly of interest to applications with mostly-static graphics that avoid redrawing unless
  /// something changes, like most non-game GUIs.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux: This is triggered by `draw` signal of the gtk window. It can be used to detect if
  ///   the window is requested to redraw. But widgets it contains are usually not tied to its signal.
  ///   So if you really want to draw each component, please consider using `connect_draw` method
  ///   from [`WidgetExt`] directly.**
  ///
  /// [`WidgetExt`]: https://gtk-rs.org/gtk3-rs/stable/latest/docs/gtk/prelude/trait.WidgetExt.html
  RedrawRequested(WindowId),

  /// Emitted after all `RedrawRequested` events have been processed and control flow is about to
  /// be taken away from the program. If there are no `RedrawRequested` events, it is emitted
  /// immediately after `MainEventsCleared`.
  ///
  /// This event is useful for doing any cleanup or bookkeeping work after all the rendering
  /// tasks have been completed.
  RedrawEventsCleared,

  /// Emitted when the event loop is being shut down.
  ///
  /// This is irreversable - if this event is emitted, it is guaranteed to be the last event that
  /// gets emitted. You generally want to treat this as an "do on quit" event.
  LoopDestroyed,

  /// Emitted when the app is open by external resources, like opening a file or deeplink.
  Opened { urls: Vec<url::Url> },

  /// ## Platform-specific
  ///
  /// - **macOS**: https://developer.apple.com/documentation/appkit/nsapplicationdelegate/1428638-applicationshouldhandlereopen with return value same as hasVisibleWindows
  /// - **Other**: Unsupported.
  #[non_exhaustive]
  Reopen { has_visible_windows: bool },
}

impl<T: Clone> Clone for Event<'static, T> {
  fn clone(&self) -> Self {
    use self::Event::*;
    match self {
      WindowEvent { window_id, event } => WindowEvent {
        window_id: *window_id,
        event: event.clone(),
      },
      UserEvent(event) => UserEvent(event.clone()),
      DeviceEvent { device_id, event } => DeviceEvent {
        device_id: *device_id,
        event: event.clone(),
      },
      NewEvents(cause) => NewEvents(*cause),
      MainEventsCleared => MainEventsCleared,
      RedrawRequested(wid) => RedrawRequested(*wid),
      RedrawEventsCleared => RedrawEventsCleared,
      LoopDestroyed => LoopDestroyed,
      Suspended => Suspended,
      Resumed => Resumed,
      Opened { urls } => Opened { urls: urls.clone() },
      Reopen {
        has_visible_windows,
      } => Reopen {
        has_visible_windows: *has_visible_windows,
      },
    }
  }
}

impl<'a, T> Event<'a, T> {
  pub fn map_nonuser_event<U>(self) -> Result<Event<'a, U>, Event<'a, T>> {
    use self::Event::*;
    match self {
      UserEvent(_) => Err(self),
      WindowEvent { window_id, event } => Ok(WindowEvent { window_id, event }),
      DeviceEvent { device_id, event } => Ok(DeviceEvent { device_id, event }),
      NewEvents(cause) => Ok(NewEvents(cause)),
      MainEventsCleared => Ok(MainEventsCleared),
      RedrawRequested(wid) => Ok(RedrawRequested(wid)),
      RedrawEventsCleared => Ok(RedrawEventsCleared),
      LoopDestroyed => Ok(LoopDestroyed),
      Suspended => Ok(Suspended),
      Resumed => Ok(Resumed),
      Opened { urls } => Ok(Opened { urls }),
      Reopen {
        has_visible_windows,
      } => Ok(Reopen {
        has_visible_windows,
      }),
    }
  }

  /// If the event doesn't contain a reference, turn it into an event with a `'static` lifetime.
  /// Otherwise, return `None`.
  pub fn to_static(self) -> Option<Event<'static, T>> {
    use self::Event::*;
    match self {
      WindowEvent { window_id, event } => event
        .to_static()
        .map(|event| WindowEvent { window_id, event }),
      UserEvent(event) => Some(UserEvent(event)),
      DeviceEvent { device_id, event } => Some(DeviceEvent { device_id, event }),
      NewEvents(cause) => Some(NewEvents(cause)),
      MainEventsCleared => Some(MainEventsCleared),
      RedrawRequested(wid) => Some(RedrawRequested(wid)),
      RedrawEventsCleared => Some(RedrawEventsCleared),
      LoopDestroyed => Some(LoopDestroyed),
      Suspended => Some(Suspended),
      Resumed => Some(Resumed),
      Opened { urls } => Some(Opened { urls }),
      Reopen {
        has_visible_windows,
      } => Some(Reopen {
        has_visible_windows,
      }),
    }
  }
}

/// Describes the reason the event loop is resuming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StartCause {
  /// Sent if the time specified by `ControlFlow::WaitUntil` has been reached. Contains the
  /// moment the timeout was requested and the requested resume time. The actual resume time is
  /// guaranteed to be equal to or after the requested resume time.
  #[non_exhaustive]
  ResumeTimeReached {
    start: Instant,
    requested_resume: Instant,
  },

  /// Sent if the OS has new events to send to the window, after a wait was requested. Contains
  /// the moment the wait was requested and the resume time, if requested.
  #[non_exhaustive]
  WaitCancelled {
    start: Instant,
    requested_resume: Option<Instant>,
  },

  /// Sent if the event loop is being resumed after the loop's control flow was set to
  /// `ControlFlow::Poll`.
  Poll,

  /// Sent once, immediately after `run` is called. Indicates that the loop was just initialized.
  Init,
}

/// Describes an event from a `Window`.
#[non_exhaustive]
#[derive(Debug, PartialEq)]
pub enum WindowEvent<'a> {
  /// The size of the window has changed. Contains the client area's new dimensions.
  Resized(PhysicalSize<u32>),

  /// The position of the window has changed. Contains the window's new position.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux(Wayland)**: will always be (0, 0) since Wayland doesn't support a global cordinate system.
  Moved(PhysicalPosition<i32>),

  /// The window has been requested to close.
  CloseRequested,

  /// The window has been destroyed.
  ///
  /// ## Platform-specific
  ///
  /// - **Windows / Linux:** Only fired if the [`crate::window::Window`] is dropped.
  /// - **macOS:** Fired if the [`crate::window::Window`] is dropped or the dock `Quit` item is clicked.
  Destroyed,

  /// A file has been dropped into the window.
  ///
  /// When the user drops multiple files at once, this event will be emitted for each file
  /// separately.
  DroppedFile(PathBuf),

  /// A file is being hovered over the window.
  ///
  /// When the user hovers multiple files at once, this event will be emitted for each file
  /// separately.
  HoveredFile(PathBuf),

  /// A file was hovered, but has exited the window.
  ///
  /// There will be a single `HoveredFileCancelled` event triggered even if multiple files were
  /// hovered.
  HoveredFileCancelled,

  /// The window received a unicode character.
  ReceivedImeText(String),

  /// The window gained or lost focus.
  ///
  /// The parameter is true if the window has gained focus, and false if it has lost focus.
  Focused(bool),

  /// An event from the keyboard has been received.
  ///
  /// ## Platform-specific
  /// - **Windows:** The shift key overrides NumLock. In other words, while shift is held down,
  ///   numpad keys act as if NumLock wasn't active. When this is used, the OS sends fake key
  ///   events which are not marked as `is_synthetic`.
  #[non_exhaustive]
  KeyboardInput {
    device_id: DeviceId,
    event: KeyEvent,

    /// If `true`, the event was generated synthetically by tao
    /// in one of the following circumstances:
    ///
    /// * Synthetic key press events are generated for all keys pressed
    ///   when a window gains focus. Likewise, synthetic key release events
    ///   are generated for all keys pressed when a window goes out of focus.
    ///   ***Currently, this is only functional on Linux and Windows***
    ///
    /// Otherwise, this value is always `false`.
    is_synthetic: bool,
  },

  /// The keyboard modifiers have changed.
  ModifiersChanged(ModifiersState),

  /// The cursor has moved on the window.
  CursorMoved {
    device_id: DeviceId,

    /// (x,y) coords in pixels relative to the top-left corner of the window. Because the range of this data is
    /// limited by the display area and it may have been transformed by the OS to implement effects such as cursor
    /// acceleration, it should not be used to implement non-cursor-like interactions such as 3D camera control.
    position: PhysicalPosition<f64>,
    #[deprecated = "Deprecated in favor of WindowEvent::ModifiersChanged"]
    modifiers: ModifiersState,
  },

  /// The cursor has entered the window.
  CursorEntered { device_id: DeviceId },

  /// The cursor has left the window.
  CursorLeft { device_id: DeviceId },

  /// A mouse wheel movement or touchpad scroll occurred.
  MouseWheel {
    device_id: DeviceId,
    delta: MouseScrollDelta,
    phase: TouchPhase,
    #[deprecated = "Deprecated in favor of WindowEvent::ModifiersChanged"]
    modifiers: ModifiersState,
  },

  /// An mouse button press has been received.
  MouseInput {
    device_id: DeviceId,
    state: ElementState,
    button: MouseButton,
    #[deprecated = "Deprecated in favor of WindowEvent::ModifiersChanged"]
    modifiers: ModifiersState,
  },

  /// Touchpad pressure event.
  ///
  /// At the moment, only supported on Apple forcetouch-capable macbooks.
  /// The parameters are: pressure level (value between 0 and 1 representing how hard the touchpad
  /// is being pressed) and stage (integer representing the click level).
  TouchpadPressure {
    device_id: DeviceId,
    pressure: f32,
    stage: i64,
  },

  /// Motion on some analog axis. May report data redundant to other, more specific events.
  AxisMotion {
    device_id: DeviceId,
    axis: AxisId,
    value: f64,
  },

  /// Touch event has been received
  Touch(Touch),

  /// The window's scale factor has changed.
  ///
  /// The following user actions can cause DPI changes:
  ///
  /// * Changing the display's resolution.
  /// * Changing the display's scale factor (e.g. in Control Panel on Windows).
  /// * Moving the window to a display with a different scale factor.
  ///
  /// After this event callback has been processed, the window will be resized to whatever value
  /// is pointed to by the `new_inner_size` reference. By default, this will contain the size suggested
  /// by the OS, but it can be changed to any value.
  ///
  /// For more information about DPI in general, see the [`dpi`](crate::dpi) module.
  ScaleFactorChanged {
    scale_factor: f64,
    new_inner_size: &'a mut PhysicalSize<u32>,
  },

  /// The system window theme has changed.
  ///
  /// Applications might wish to react to this to change the theme of the content of the window
  /// when the system changes the window theme.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux / Android / iOS:** Unsupported
  ThemeChanged(Theme),

  /// The window decorations has been clicked.
  ///
  /// ## Platform-specific
  ///
  /// - **Linux / macOS / Android / iOS:** Unsupported
  DecorationsClick,
}

impl Clone for WindowEvent<'static> {
  fn clone(&self) -> Self {
    use self::WindowEvent::*;
    match self {
      Resized(size) => Resized(*size),
      Moved(pos) => Moved(*pos),
      CloseRequested => CloseRequested,
      Destroyed => Destroyed,
      DroppedFile(file) => DroppedFile(file.clone()),
      HoveredFile(file) => HoveredFile(file.clone()),
      HoveredFileCancelled => HoveredFileCancelled,
      ReceivedImeText(c) => ReceivedImeText(c.clone()),
      Focused(f) => Focused(*f),
      KeyboardInput {
        device_id,
        event,
        is_synthetic,
      } => KeyboardInput {
        device_id: *device_id,
        event: event.clone(),
        is_synthetic: *is_synthetic,
      },

      ModifiersChanged(modifiers) => ModifiersChanged(*modifiers),
      #[allow(deprecated)]
      CursorMoved {
        device_id,
        position,
        modifiers,
      } => CursorMoved {
        device_id: *device_id,
        position: *position,
        modifiers: *modifiers,
      },
      CursorEntered { device_id } => CursorEntered {
        device_id: *device_id,
      },
      CursorLeft { device_id } => CursorLeft {
        device_id: *device_id,
      },
      #[allow(deprecated)]
      MouseWheel {
        device_id,
        delta,
        phase,
        modifiers,
      } => MouseWheel {
        device_id: *device_id,
        delta: *delta,
        phase: *phase,
        modifiers: *modifiers,
      },
      #[allow(deprecated)]
      MouseInput {
        device_id,
        state,
        button,
        modifiers,
      } => MouseInput {
        device_id: *device_id,
        state: *state,
        button: *button,
        modifiers: *modifiers,
      },
      TouchpadPressure {
        device_id,
        pressure,
        stage,
      } => TouchpadPressure {
        device_id: *device_id,
        pressure: *pressure,
        stage: *stage,
      },
      AxisMotion {
        device_id,
        axis,
        value,
      } => AxisMotion {
        device_id: *device_id,
        axis: *axis,
        value: *value,
      },
      Touch(touch) => Touch(*touch),
      ThemeChanged(theme) => ThemeChanged(*theme),
      ScaleFactorChanged { .. } => {
        unreachable!("Static event can't be about scale factor changing")
      }
      DecorationsClick => DecorationsClick,
    }
  }
}

impl<'a> WindowEvent<'a> {
  pub fn to_static(self) -> Option<WindowEvent<'static>> {
    use self::WindowEvent::*;
    match self {
      Resized(size) => Some(Resized(size)),
      Moved(position) => Some(Moved(position)),
      CloseRequested => Some(CloseRequested),
      Destroyed => Some(Destroyed),
      DroppedFile(file) => Some(DroppedFile(file)),
      HoveredFile(file) => Some(HoveredFile(file)),
      HoveredFileCancelled => Some(HoveredFileCancelled),
      ReceivedImeText(c) => Some(ReceivedImeText(c)),
      Focused(focused) => Some(Focused(focused)),
      KeyboardInput {
        device_id,
        event,
        is_synthetic,
      } => Some(KeyboardInput {
        device_id,
        event,
        is_synthetic,
      }),
      ModifiersChanged(modifiers) => Some(ModifiersChanged(modifiers)),
      #[allow(deprecated)]
      CursorMoved {
        device_id,
        position,
        modifiers,
      } => Some(CursorMoved {
        device_id,
        position,
        modifiers,
      }),
      CursorEntered { device_id } => Some(CursorEntered { device_id }),
      CursorLeft { device_id } => Some(CursorLeft { device_id }),
      #[allow(deprecated)]
      MouseWheel {
        device_id,
        delta,
        phase,
        modifiers,
      } => Some(MouseWheel {
        device_id,
        delta,
        phase,
        modifiers,
      }),
      #[allow(deprecated)]
      MouseInput {
        device_id,
        state,
        button,
        modifiers,
      } => Some(MouseInput {
        device_id,
        state,
        button,
        modifiers,
      }),
      TouchpadPressure {
        device_id,
        pressure,
        stage,
      } => Some(TouchpadPressure {
        device_id,
        pressure,
        stage,
      }),
      AxisMotion {
        device_id,
        axis,
        value,
      } => Some(AxisMotion {
        device_id,
        axis,
        value,
      }),
      Touch(touch) => Some(Touch(touch)),
      ThemeChanged(theme) => Some(ThemeChanged(theme)),
      ScaleFactorChanged { .. } => None,
      DecorationsClick => Some(DecorationsClick),
    }
  }
}

/// Identifier of an input device.
///
/// Whenever you receive an event arising from a particular input device, this event contains a `DeviceId` which
/// identifies its origin. Note that devices may be virtual (representing an on-screen cursor and keyboard focus) or
/// physical. Virtual devices typically aggregate inputs from multiple physical devices.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub(crate) platform_impl::DeviceId);

impl DeviceId {
  /// # Safety
  /// Returns a dummy `DeviceId`, useful for unit testing. The only guarantee made about the return
  /// value of this function is that it will always be equal to itself and to future values returned
  /// by this function.  No other guarantees are made. This may be equal to a real `DeviceId`.
  ///
  /// **Passing this into a tao function will result in undefined behavior.**
  pub unsafe fn dummy() -> Self {
    DeviceId(platform_impl::DeviceId::dummy())
  }
}

/// Represents raw hardware events that are not associated with any particular window.
///
/// Useful for interactions that diverge significantly from a conventional 2D GUI, such as 3D camera or first-person
/// game controls. Many physical actions, such as mouse movement, can produce both device and window events. Because
/// window events typically arise from virtual devices (corresponding to GUI cursors and keyboard focus) the device IDs
/// may not match.
///
/// Note that these events are delivered regardless of input focus.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum DeviceEvent {
  Added,
  Removed,

  /// Change in physical position of a pointing device.
  ///
  /// This represents raw, unfiltered physical motion. Not to be confused with `WindowEvent::CursorMoved`.
  #[non_exhaustive]
  MouseMotion {
    /// (x, y) change in position in unspecified units.
    ///
    /// Different devices may use different units.
    delta: (f64, f64),
  },

  /// Physical scroll event
  #[non_exhaustive]
  MouseWheel {
    delta: MouseScrollDelta,
  },

  /// Motion on some analog axis.  This event will be reported for all arbitrary input devices
  /// that tao supports on this platform, including mouse devices.  If the device is a mouse
  /// device then this will be reported alongside the MouseMotion event.
  #[non_exhaustive]
  Motion {
    axis: AxisId,
    value: f64,
  },

  #[non_exhaustive]
  Button {
    button: ButtonId,
    state: ElementState,
  },

  Key(RawKeyEvent),

  #[non_exhaustive]
  Text {
    codepoint: char,
  },
}

/// Describes a keyboard input as a raw device event.
///
/// Note that holding down a key may produce repeated `RawKeyEvent`s. The
/// operating system doesn't provide information whether such an event is a
/// repeat or the initial keypress. An application may emulate this by, for
/// example keeping a Map/Set of pressed keys and determining whether a keypress
/// corresponds to an already pressed key.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RawKeyEvent {
  pub physical_key: keyboard::KeyCode,
  pub state: ElementState,
}

/// Describes a keyboard input targeting a window.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct KeyEvent {
  /// Represents the position of a key independent of the currently active layout.
  ///
  /// It also uniquely identifies the physical key (i.e. it's mostly synonymous with a scancode).
  /// The most prevalent use case for this is games. For example the default keys for the player
  /// to move around might be the W, A, S, and D keys on a US layout. The position of these keys
  /// is more important than their label, so they should map to Z, Q, S, and D on an "AZERTY"
  /// layout. (This value is `KeyCode::KeyW` for the Z key on an AZERTY layout.)
  ///
  /// Note that `Fn` and `FnLock` key events are not guaranteed to be emitted by `tao`. These
  /// keys are usually handled at the hardware or OS level.
  pub physical_key: keyboard::KeyCode,

  /// This value is affected by all modifiers except <kbd>Ctrl</kbd>.
  ///
  /// This has two use cases:
  /// - Allows querying whether the current input is a Dead key.
  /// - Allows handling key-bindings on platforms which don't
  ///   support `key_without_modifiers`.
  ///
  /// ## Platform-specific
  /// - **Web:** Dead keys might be reported as the real key instead
  ///   of `Dead` depending on the browser/OS.
  pub logical_key: keyboard::Key<'static>,

  /// Contains the text produced by this keypress.
  ///
  /// In most cases this is identical to the content
  /// of the `Character` variant of `logical_key`.
  /// However, on Windows when a dead key was pressed earlier
  /// but cannot be combined with the character from this
  /// keypress, the produced text will consist of two characters:
  /// the dead-key-character followed by the character resulting
  /// from this keypress.
  ///
  /// An additional difference from `logical_key` is that
  /// this field stores the text representation of any key
  /// that has such a representation. For example when
  /// `logical_key` is `Key::Enter`, this field is `Some("\r")`.
  ///
  /// This is `None` if the current keypress cannot
  /// be interpreted as text.
  ///
  /// See also: `text_with_all_modifiers()`
  pub text: Option<&'static str>,

  pub location: keyboard::KeyLocation,
  pub state: ElementState,
  pub repeat: bool,

  pub(crate) platform_specific: platform_impl::KeyEventExtra,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl KeyEvent {
  /// Identical to `KeyEvent::text` but this is affected by <kbd>Ctrl</kbd>.
  ///
  /// For example, pressing <kbd>Ctrl</kbd>+<kbd>a</kbd> produces `Some("\x01")`.
  pub fn text_with_all_modifiers(&self) -> Option<&str> {
    self.platform_specific.text_with_all_modifiers
  }

  /// This value ignores all modifiers including,
  /// but not limited to <kbd>Shift</kbd>, <kbd>Caps Lock</kbd>,
  /// and <kbd>Ctrl</kbd>. In most cases this means that the
  /// unicode character in the resulting string is lowercase.
  ///
  /// This is useful for key-bindings / shortcut key combinations.
  ///
  /// In case `logical_key` reports `Dead`, this will still report the
  /// key as `Character` according to the current keyboard layout. This value
  /// cannot be `Dead`.
  pub fn key_without_modifiers(&self) -> keyboard::Key<'static> {
    self.platform_specific.key_without_modifiers.clone()
  }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
impl KeyEvent {
  /// Identical to `KeyEvent::text`.
  pub fn text_with_all_modifiers(&self) -> Option<&str> {
    self.text
  }

  /// Identical to `KeyEvent::logical_key`.
  pub fn key_without_modifiers(&self) -> keyboard::Key<'static> {
    self.logical_key.clone()
  }
}

/// Describes touch-screen input state.
#[non_exhaustive]
#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TouchPhase {
  Started,
  Moved,
  Ended,
  Cancelled,
}

/// Represents a touch event
///
/// Every time the user touches the screen, a new `Start` event with an unique
/// identifier for the finger is generated. When the finger is lifted, an `End`
/// event is generated with the same finger id.
///
/// After a `Start` event has been emitted, there may be zero or more `Move`
/// events when the finger is moved or the touch pressure changes.
///
/// The finger id may be reused by the system after an `End` event. The user
/// should assume that a new `Start` event received with the same id has nothing
/// to do with the old finger and is a new finger.
///
/// A `Cancelled` event is emitted when the system has canceled tracking this
/// touch, such as when the window loses focus, or on iOS if the user moves the
/// device against their face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touch {
  pub device_id: DeviceId,
  pub phase: TouchPhase,
  pub location: PhysicalPosition<f64>,
  /// Describes how hard the screen was pressed. May be `None` if the platform
  /// does not support pressure sensitivity.
  ///
  /// ## Platform-specific
  ///
  /// - Only available on **iOS** 9.0+ and **Windows** 8+.
  pub force: Option<Force>,
  /// Unique identifier of a finger.
  pub id: u64,
}

/// Describes the force of a touch event
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Force {
  /// On iOS, the force is calibrated so that the same number corresponds to
  /// roughly the same amount of pressure on the screen regardless of the
  /// device.
  #[non_exhaustive]
  Calibrated {
    /// The force of the touch, where a value of 1.0 represents the force of
    /// an average touch (predetermined by the system, not user-specific).
    ///
    /// The force reported by Apple Pencil is measured along the axis of the
    /// pencil. If you want a force perpendicular to the device, you need to
    /// calculate this value using the `altitude_angle` value.
    force: f64,
    /// The maximum possible force for a touch.
    ///
    /// The value of this field is sufficiently high to provide a wide
    /// dynamic range for values of the `force` field.
    max_possible_force: f64,
    /// The altitude (in radians) of the stylus.
    ///
    /// A value of 0 radians indicates that the stylus is parallel to the
    /// surface. The value of this property is Pi/2 when the stylus is
    /// perpendicular to the surface.
    altitude_angle: Option<f64>,
  },
  /// If the platform reports the force as normalized, we have no way of
  /// knowing how much pressure 1.0 corresponds to - we know it's the maximum
  /// amount of force, but as to how much force, you might either have to
  /// press really really hard, or not hard at all, depending on the device.
  Normalized(f64),
}

impl Force {
  /// Returns the force normalized to the range between 0.0 and 1.0 inclusive.
  /// Instead of normalizing the force, you should prefer to handle
  /// `Force::Calibrated` so that the amount of force the user has to apply is
  /// consistent across devices.
  pub fn normalized(&self) -> f64 {
    match self {
      Force::Calibrated {
        force,
        max_possible_force,
        altitude_angle,
      } => {
        let force = match altitude_angle {
          Some(altitude_angle) => force / altitude_angle.sin(),
          None => *force,
        };
        force / max_possible_force
      }
      Force::Normalized(force) => *force,
    }
  }
}

/// Identifier for a specific analog axis on some device.
pub type AxisId = u32;

/// Identifier for a specific button on some device.
pub type ButtonId = u32;

/// Describes the input state of a key.
#[non_exhaustive]
#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ElementState {
  Pressed,
  Released,
}

/// Describes a button of a mouse controller.
#[non_exhaustive]
#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MouseButton {
  Left,
  Right,
  Middle,
  Other(u16),
}

/// Describes a difference in the mouse scroll wheel state.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MouseScrollDelta {
  /// Amount in lines or rows to scroll in the horizontal
  /// and vertical directions.
  ///
  /// Positive values indicate movement forward
  /// (away from the user) or rightwards.
  LineDelta(f32, f32),
  /// Amount in pixels to scroll in the horizontal and
  /// vertical direction.
  ///
  /// Scroll events are expressed as a PixelDelta if
  /// supported by the device (eg. a touchpad) and
  /// platform.
  PixelDelta(PhysicalPosition<f64>),
}
