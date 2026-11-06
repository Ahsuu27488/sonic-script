//! Watches which element has the keyboard focus, through UI Automation (M2 phase 5, D-016), and sends what it
//! is to a channel. The properties the pill needs come with each event in one go (a cache request), so an
//! event costs one call into the focused app, not one per property; the handler then only sends them on.

use std::sync::mpsc::Sender;

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationFocusChangedEventHandler,
    IUIAutomationFocusChangedEventHandler_Impl, UIA_BoundingRectanglePropertyId,
    UIA_ControlTypePropertyId, UIA_IsEnabledPropertyId, UIA_IsPasswordPropertyId,
    UIA_IsValuePatternAvailablePropertyId, UIA_PROPERTY_ID, UIA_ProcessIdPropertyId,
    UIA_ValueIsReadOnlyPropertyId,
};
use windows::core::{Ref, Result, implement};

use crate::popup::{Focus, Rect};

#[implement(IUIAutomationFocusChangedEventHandler)]
struct Handler {
    tx: Sender<Option<Focus>>,
}

impl IUIAutomationFocusChangedEventHandler_Impl for Handler_Impl {
    fn HandleFocusChangedEvent(&self, sender: Ref<IUIAutomationElement>) -> Result<()> {
        let focus = sender.as_ref().and_then(read);
        let _ = self.tx.send(focus);
        Ok(())
    }
}

/// The properties read with each event (cached by UI Automation).
const CACHED: [UIA_PROPERTY_ID; 7] = [
    UIA_ControlTypePropertyId,
    UIA_IsEnabledPropertyId,
    UIA_IsPasswordPropertyId,
    UIA_BoundingRectanglePropertyId,
    UIA_ProcessIdPropertyId,
    UIA_IsValuePatternAvailablePropertyId,
    UIA_ValueIsReadOnlyPropertyId,
];

/// A cached boolean property; `None` when it is not a boolean (not supported by the element).
fn cached_bool(e: &IUIAutomationElement, id: UIA_PROPERTY_ID) -> Option<bool> {
    // SAFETY: reads a VARIANT the element returned; only its `boolVal` when its type says it is a VT_BOOL.
    unsafe {
        let v: VARIANT = e.GetCachedPropertyValue(id).ok()?;
        let inner = &v.Anonymous.Anonymous;
        (inner.vt == VT_BOOL).then(|| inner.Anonymous.boolVal.0 != 0)
    }
}

/// The element's properties from the event's cache; `None` when they could not be read.
fn read(e: &IUIAutomationElement) -> Option<Focus> {
    // SAFETY: cached reads on a live element: no call into the focused app.
    unsafe {
        let r = e.CachedBoundingRectangle().ok()?;
        let pid = e.CachedProcessId().unwrap_or_default() as u32;
        let value_read_only = match cached_bool(e, UIA_IsValuePatternAvailablePropertyId) {
            Some(true) => Some(cached_bool(e, UIA_ValueIsReadOnlyPropertyId).unwrap_or(true)),
            _ => None,
        };
        // The program is the focused element's own (a UWP app is not "ApplicationFrameHost"); the window
        // class is the one in front (terminals).
        let (front_exe, class) = sonicscript::inject::app_in_front().unwrap_or_default();
        let exe = sonicscript::inject::exe_of_pid(pid).unwrap_or(front_exe);
        Some(Focus {
            control_type: e.CachedControlType().ok()?.0,
            enabled: e.CachedIsEnabled().map(|b| b.as_bool()).unwrap_or(false),
            password: e.CachedIsPassword().map(|b| b.as_bool()).unwrap_or(true),
            value_read_only,
            rect: Rect {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            },
            pid,
            exe,
            class,
        })
    }
}

/// Starts watching on a thread of its own; every focus change sends `Some(focus)`, or `None` when the element
/// could not be read. Never waits: registering the handler needs the app's main thread to be running (it
/// hangs when the caller blocks the main thread waiting for it), so a failure is reported through `failed`.
pub fn start(tx: Sender<Option<Focus>>, failed: impl FnOnce(String) + Send + 'static) {
    std::thread::spawn(move || {
        // SAFETY: COM is set up for this thread before any COM call; the automation object, the cache request
        // and the handler live for the rest of the process (this thread parks forever).
        let started = unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .and_then(|()| {
                    CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                })
                .and_then(|uia| {
                    let cache = uia.CreateCacheRequest()?;
                    for id in CACHED {
                        cache.AddProperty(id)?;
                    }
                    let handler: IUIAutomationFocusChangedEventHandler = Handler { tx }.into();
                    uia.AddFocusChangedEventHandler(&cache, &handler)
                        .map(|()| (uia, cache, handler))
                })
        };
        match started {
            Ok(keep) => {
                crate::popup::debug_log("focuswatch: started");
                let _keep = keep;
                loop {
                    std::thread::park();
                }
            }
            Err(e) => failed(format!("UI Automation is not available: {e}")),
        }
    });
}
