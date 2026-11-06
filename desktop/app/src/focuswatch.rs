//! Watches which element has the keyboard focus, through UI Automation (M2 phase 5, D-016), and sends what it
//! is to a channel. UI Automation calls the handler on its own threads; the handler only reads a few
//! properties of the element and sends them, so a slow app cannot block anything else.

use std::sync::mpsc::Sender;

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationFocusChangedEventHandler,
    IUIAutomationFocusChangedEventHandler_Impl, IUIAutomationValuePattern, UIA_ValuePatternId,
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

/// The element's properties; `None` when the element is gone already (focus moves on quickly).
fn read(e: &IUIAutomationElement) -> Option<Focus> {
    // SAFETY: COM calls on a live element; each returns an error instead of crashing when the element is gone.
    unsafe {
        let value_read_only = e
            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            .ok()
            .and_then(|v| v.CurrentIsReadOnly().ok())
            .map(|b| b.as_bool());
        let r = e.CurrentBoundingRectangle().ok()?;
        let (exe, class) = sonicscript::inject::app_in_front().unwrap_or_default();
        Some(Focus {
            control_type: e.CurrentControlType().ok()?.0,
            enabled: e.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(false),
            password: e.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(true),
            value_read_only,
            rect: Rect {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            },
            pid: e.CurrentProcessId().unwrap_or_default() as u32,
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
        // SAFETY: COM is set up for this thread before any COM call; the automation object and the handler
        // live for the rest of the process (this thread parks forever).
        let started = unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .and_then(|()| {
                    CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                })
                .and_then(|uia| {
                    let handler: IUIAutomationFocusChangedEventHandler = Handler { tx }.into();
                    uia.AddFocusChangedEventHandler(None, &handler)
                        .map(|()| (uia, handler))
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
