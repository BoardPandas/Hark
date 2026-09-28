//! A retained WinRT share source. Construct, show and drop on the UI thread.

use std::marker::PhantomData;
use std::rc::Rc;
use windows::core::{factory, w, Error, Result, HSTRING, PCWSTR};
use windows::ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager};
use windows::Foundation::TypedEventHandler;
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_SINGLETHREADED};
use windows::Win32::UI::Shell::IDataTransferManagerInterop;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};

pub(super) struct WindowsShare {
    manager: DataTransferManager,
    token: i64,
    // Declared after the interfaces, so they are released before uninitializing.
    _apartment: Apartment,
    // Windows interfaces may be agile, but this owner belongs to the UI thread.
    _main_thread: PhantomData<Rc<()>>,
}

impl WindowsShare {
    pub fn show(title: &str, text: &str) -> Result<Self> {
        // SAFETY: called only by Sharing::run on the egui main thread; balanced
        // by Apartment even if activation or handler registration fails.
        unsafe {
            RoInitialize(RO_INIT_SINGLETHREADED)?;
        }
        let apartment = Apartment;
        // SAFETY: only read a top-level HWND and confirm that it is ours.
        let hwnd = unsafe { FindWindowW(PCWSTR::null(), w!("Hark"))? };
        let mut process = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut process));
        }
        if process != std::process::id() {
            return Err(Error::new(
                windows::core::HRESULT(0x80004005_u32 as i32),
                "Hark's main window is unavailable.",
            ));
        }
        let interop: IDataTransferManagerInterop =
            factory::<DataTransferManager, IDataTransferManagerInterop>()?;
        // SAFETY: hwnd is our live main window; the interface is from WinRT.
        let manager: DataTransferManager = unsafe { interop.GetForWindow(hwnd)? };
        let (title, text) = (HSTRING::from(title), HSTRING::from(text));
        let token = manager.DataRequested(&TypedEventHandler::<
            DataTransferManager,
            DataRequestedEventArgs,
        >::new(move |_, args| {
            let request = args.ok()?.Request()?;
            let data = request.Data()?;
            data.Properties()?.SetTitle(&title)?;
            data.SetText(&text)
        }))?;
        let owner = Self {
            manager,
            token,
            _apartment: apartment,
            _main_thread: PhantomData,
        };
        // SAFETY: show only on the thread that owns hwnd and its message pump.
        unsafe {
            interop.ShowShareUIForWindow(hwnd)?;
        }
        Ok(owner)
    }
}

impl Drop for WindowsShare {
    fn drop(&mut self) {
        if let Err(error) = self.manager.RemoveDataRequested(self.token) {
            log::warn!("Windows Share handler removal failed: {}", error.code());
        }
    }
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: exactly one successful RoInitialize belongs to this owner.
        unsafe {
            RoUninitialize();
        }
    }
}
