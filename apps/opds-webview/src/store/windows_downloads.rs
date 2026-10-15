//! WebView2 downloads are separate from webpage navigation. Cancel and queue
//! their original URLs on a cost change so Wry can clean its partial file.
use super::Action;
use std::sync::mpsc::{self, Receiver, Sender};
use webview2_com::Microsoft::Web::WebView2::Win32::{COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS, ICoreWebView2_4, ICoreWebView2DownloadOperation};
use webview2_com::{DownloadStartingEventHandler, take_pwstr};
use windows::core::{Interface, PWSTR, Result};
use wry::{WebView, WebViewExtWindows};

pub(super) struct Gate {
    operations: Vec<ICoreWebView2DownloadOperation>,
    incoming: Receiver<ICoreWebView2DownloadOperation>,
    native: ICoreWebView2_4,
    token: i64,
    sender: Sender<Action>,
}

impl Gate {
    pub(super) fn attach(page: &WebView, sender: Sender<Action>) -> Result<Self> {
        let (captured, incoming) = mpsc::channel();
        let native: ICoreWebView2_4 = page.webview().cast()?;
        let mut token = 0;
        unsafe {
            native.add_DownloadStarting(
                &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let _ = captured.send(args.DownloadOperation()?);
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }
        Ok(Self { operations: Vec::new(), incoming, native, token, sender })
    }

    pub(super) fn tick(&mut self, _active: bool) {
        self.operations.extend(self.incoming.try_iter());
        self.operations.retain(|operation| unsafe {
            let mut state = COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS;
            if operation.State(&mut state).is_err() || state != COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS {
                return false;
            }
            if client_platform_runtime::transfers::allowed() {
                return true;
            }
            let mut uri = PWSTR::null();
            if operation.Uri(&mut uri).is_ok() {
                let _ = self.sender.send(Action::DownloadDeferred(take_pwstr(uri)));
            }
            let _ = operation.Cancel();
            false
        });
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        unsafe {
            let _ = self.native.remove_DownloadStarting(self.token);
        }
    }
}
