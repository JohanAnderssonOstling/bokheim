//! Observe WKDownload without replacing Wry's download storage or completion
//! delegates. Navigation messages continue through the original delegate.
use super::Action;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject, Sel};
use objc2::{DeclaredClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol};
use objc2_web_kit::{WKDownload, WKNavigationAction, WKNavigationDelegate, WKNavigationResponse, WKWebView};
use std::sync::mpsc::{self, Receiver, Sender};
use wry::{WebView, WebViewExtMacOS};

struct DelegateState {
    original: Retained<ProtocolObject<dyn WKNavigationDelegate>>,
    downloads: Sender<Retained<WKDownload>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "BokheimTransferNavigationDelegate"]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateState]
    struct TransferDelegate;

    unsafe impl NSObjectProtocol for TransferDelegate {
        #[unsafe(method(respondsToSelector:))]
        fn responds(&self, selector: Sel) -> bool {
            selector == sel!(webView:navigationAction:didBecomeDownload:)
                || selector == sel!(webView:navigationResponse:didBecomeDownload:)
                || self.ivars().original.respondsToSelector(selector)
        }
    }

    impl TransferDelegate {
        #[unsafe(method(forwardingTargetForSelector:))]
        fn forward(&self, _selector: Sel) -> *mut AnyObject {
            Retained::as_ptr(&self.ivars().original).cast::<AnyObject>().cast_mut()
        }
    }

    unsafe impl WKNavigationDelegate for TransferDelegate {
        #[unsafe(method(webView:navigationAction:didBecomeDownload:))]
        fn action(&self, page: &WKWebView, action: &WKNavigationAction, download: &WKDownload) {
            unsafe { self.ivars().original.webView_navigationAction_didBecomeDownload(page, action, download); }
            self.capture(download);
        }
        #[unsafe(method(webView:navigationResponse:didBecomeDownload:))]
        fn response(&self, page: &WKWebView, response: &WKNavigationResponse, download: &WKDownload) {
            unsafe { self.ivars().original.webView_navigationResponse_didBecomeDownload(page, response, download); }
            self.capture(download);
        }
    }
);

impl TransferDelegate {
    fn capture(&self, download: &WKDownload) {
        // The delegate callback lends a live Objective-C object; retain it
        // until completion or until the policy cancels this transfer.
        if let Some(download) = unsafe { Retained::retain(download as *const WKDownload as *mut WKDownload) } {
            let _ = self.ivars().downloads.send(download);
        }
    }
}

pub(super) struct Gate {
    _delegate: Retained<TransferDelegate>,
    downloads: Vec<Retained<WKDownload>>,
    incoming: Receiver<Retained<WKDownload>>,
    sender: Sender<Action>,
}

impl Gate {
    pub(super) fn attach(page: &WebView, sender: Sender<Action>) -> Option<Self> {
        let native = page.webview();
        let original = unsafe { native.navigationDelegate()? };
        let (downloads, incoming) = mpsc::channel();
        let object = MainThreadMarker::new()?.alloc::<TransferDelegate>().set_ivars(DelegateState { original, downloads });
        let delegate: Retained<TransferDelegate> = unsafe { msg_send![super(object), init] };
        unsafe {
            native.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        }
        Some(Self { _delegate: delegate, downloads: Vec::new(), incoming, sender })
    }

    pub(super) fn complete(&mut self, url: &str) {
        self.downloads.extend(self.incoming.try_iter());
        let downloads = &mut self.downloads;
        if let Some(index) = downloads.iter().position(|download| unsafe { download.originalRequest().and_then(|request| request.URL()).and_then(|url| url.absoluteString()).is_some_and(|source| source.to_string() == url) }) {
            downloads.remove(index);
        }
    }

    pub(super) fn tick(&mut self, _active: bool) {
        self.downloads.extend(self.incoming.try_iter());
        if client_platform_runtime::transfers::allowed() {
            return;
        }
        for download in self.downloads.drain(..) {
            unsafe {
                if let Some(url) = download.originalRequest().and_then(|request| request.URL()).and_then(|url| url.absoluteString()) {
                    let _ = self.sender.send(Action::DownloadDeferred(url.to_string()));
                }
                download.cancel(None);
            }
        }
    }
}
