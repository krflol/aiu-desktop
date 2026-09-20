use anyhow::{Result, anyhow};
use eframe::{Frame, egui};
use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

#[derive(Clone)]
pub struct Wake {
    context: egui::Context,
    #[cfg(windows)]
    native: Arc<Mutex<isize>>,
}
impl Wake {
    pub fn new(context: egui::Context, frame: &Frame) -> Result<Self> {
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let hwnd = match frame
                .window_handle()
                .map_err(|e| anyhow!("window handle: {e}"))?
                .as_raw()
            {
                RawWindowHandle::Win32(h) => h.hwnd.get(),
                _ => return Err(anyhow!("not a Win32 window")),
            };
            Ok(Self {
                context,
                native: Arc::new(Mutex::new(hwnd as isize)),
            })
        }
        #[cfg(not(windows))]
        {
            let _ = frame;
            Ok(Self { context })
        }
    }
    pub fn request_repaint(&self) {
        self.context.request_repaint();
        #[cfg(windows)]
        if let Ok(handle) = self.native.lock()
            && *handle != 0
        {
            unsafe {
                let _ = windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(
                    *handle as _,
                    windows_sys::Win32::UI::WindowsAndMessaging::WM_PAINT,
                    0,
                    0,
                );
            }
        }
    }
}

pub struct Heartbeat {
    wake: Wake,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Heartbeat {
    pub fn new(wake: Wake) -> Self {
        let (tx, rx) = mpsc::channel();
        let worker = wake.clone();
        let thread = thread::spawn(move || {
            while rx.recv_timeout(Duration::from_secs(1)).is_err() {
                worker.request_repaint();
            }
        });
        Self {
            wake,
            stop: Some(tx),
            thread: Some(thread),
        }
    }
}
impl Drop for Heartbeat {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Ok(mut h) = self.wake.native.lock() {
            *h = 0;
        }
        self.stop.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
