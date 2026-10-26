package se.bokheim.reader.gpui;

import android.app.Activity;
import android.os.Handler;
import android.os.Looper;
import android.os.Message;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewConfiguration;
import android.webkit.CookieManager;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;
import org.json.JSONObject;
import java.io.File;
import java.util.HashSet;
import java.util.HashMap;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Only the webpage is native. GPUI measures its rectangle and owns all chrome. */
final class BookBrowserSurface {
    interface Events { void send(long id, String message); }
    final long sessionId;
    final WebView web;
    private final Events events;
    private final File downloads;
    private final View anchor;
    private final FrameLayout host;
    private final String startPageHtml, pageMetadataScript;
    private final Handler ui = new Handler(Looper.getMainLooper());
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private final Set<String> transferring = new HashSet<>();
    private final Map<String, BookDownload.Cancellation> cancellations = new HashMap<>();
    private final Set<WebView> popups = new HashSet<>();
    private boolean closed, finished;
    private final int touchSlop;
    private final float refreshDistance;
    private boolean pullCandidate;
    private float pullX, pullY;

    BookBrowserSurface(Activity activity, long id, JSONObject request, View anchor, Events events) throws Exception {
        sessionId = id;
        this.events = events;
        this.anchor = anchor;
        startPageHtml = request.optString("start_page_html");
        pageMetadataScript = request.optString("page_metadata_script");
        touchSlop = ViewConfiguration.get(activity).getScaledTouchSlop();
        refreshDistance = 96 * activity.getResources().getDisplayMetrics().density;
        if (!(anchor.getParent() instanceof FrameLayout)) throw new IllegalStateException("The GPUI surface has no browser container");
        host = (FrameLayout) anchor.getParent();
        downloads = new File(request.getString("downloads")).getCanonicalFile();
        if (!downloads.isDirectory()) throw new java.io.IOException("The library download folder is unavailable");
        String initial = request.getString("url");
        if (!initial.equals("about:blank") && !BookDownload.webAddress(initial)) throw new IllegalArgumentException("Unsupported browser address");
        web = new WebView(activity);
        web.setVisibility(View.INVISIBLE);
        configurePage(web);
        web.setOnTouchListener((view, event) -> pageTouch(event));
        web.setWebViewClient(new WebViewClient() {
            @Override public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                String url = request.getUrl().toString();
                // Challenge widgets use inline documents in child frames.
                if (!request.isForMainFrame() && url.equals("about:srcdoc")) return false;
                if (url.startsWith("magnet:")) { emit("magnet", "url", url); return true; }
                if (!BookDownload.webAddress(url) && !url.equals("about:blank")) {
                    emit("status", "message", "This link cannot open in the book browser");
                    return true;
                }
                if (request.isForMainFrame() && BookDownload.bookAddress(url) && request.getMethod().equals("GET")) {
                    startDownload(url, view.getSettings().getUserAgentString(), null, null);
                    return true;
                }
                return false;
            }
            @Override public void onPageStarted(WebView view, String url, android.graphics.Bitmap icon) { navigation(); }
            @Override public void onPageFinished(WebView view, String url) {
                if (!closed) worker.execute(() -> CookieManager.getInstance().flush());
                navigation();
                if (!closed && BookDownload.webAddress(url) && url.equals(view.getUrl()) && !pageMetadataScript.isEmpty()) {
                    view.evaluateJavascript(pageMetadataScript, result -> {
                        if (closed || !url.equals(view.getUrl())) return;
                        try {
                            JSONObject page = new JSONObject(result);
                            if (url.equals(page.optString("url"))) emit(page.put("kind", "page_visited"));
                        } catch (org.json.JSONException ignored) { /* The page navigated or could not be inspected. */ }
                    });
                }
            }
        });
        web.setWebChromeClient(new WebChromeClient() {
            @Override public void onProgressChanged(WebView view, int progress) {
                try { emit(new JSONObject().put("kind", "page_progress").put("percent", progress)); }
                catch (org.json.JSONException impossible) { throw new IllegalStateException(impossible); }
            }
            @Override public boolean onCreateWindow(WebView view, boolean dialog, boolean gesture, Message result) {
                if (!gesture || closed) return false;
                WebView popup = new WebView(activity);
                configurePage(popup);
                popup.getSettings().setUserAgentString(view.getSettings().getUserAgentString());
                popups.add(popup);
                // Attachment responses do not start a page. They must use the
                // same download handoff as the visible WebView.
                popup.setDownloadListener((url, agent, disposition, mime, length) -> {
                    startDownload(url, agent, disposition, mime);
                    releasePopup(popup);
                });
                popup.setWebViewClient(new WebViewClient() {
                    @Override public boolean shouldOverrideUrlLoading(WebView page, WebResourceRequest request) {
                        if (!request.isForMainFrame() || !request.getMethod().equals("GET")) return false;
                        return redirectPopup(page, request.getUrl().toString());
                    }
                    @Override public boolean shouldOverrideUrlLoading(WebView page, String url) {
                        return redirectPopup(page, url);
                    }
                    private boolean redirectPopup(WebView page, String url) {
                        if (url.equals("about:blank")) return false;
                        navigate(url);
                        releasePopup(page);
                        return true;
                    }
                    @Override public void onPageFinished(WebView page, String url) {
                        redirectPopup(page, url);
                    }
                });
                ((WebView.WebViewTransport) result.obj).setWebView(popup);
                result.sendToTarget();
                return true;
            }
        });
        web.setDownloadListener((url, agent, disposition, mime, length) -> startDownload(url, agent, disposition, mime));
        host.addView(web, new FrameLayout.LayoutParams(0, 0, Gravity.TOP | Gravity.LEFT));
        // Report Opened before loading: page callbacks also use this session.
        emit("opened", null, null);
        navigate(initial);
    }

    private static void configurePage(WebView page) {
        WebSettings settings = page.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
        settings.setSupportMultipleWindows(true);
        CookieManager cookies = CookieManager.getInstance();
        cookies.setAcceptCookie(true);
        // Embedded sign-in and challenge widgets need their own cookie state.
        cookies.setAcceptThirdPartyCookies(page, true);
    }

    private void releasePopup(WebView popup) {
        // Destroy after returning from WebView callbacks. close() may have
        // already released it while this main-thread task was queued.
        ui.post(() -> { if (popups.remove(popup)) popup.destroy(); });
    }

    void command(JSONObject command) throws Exception {
        if (command.getString("command").equals("cancel_download")) {
            String download = command.getString("download");
            BookDownload.Cancellation cancellation = cancellations.get(download);
            if (cancellation != null) cancellation.cancel();
            emit("cancel_download", "download", download);
            return;
        }
        if (closed) return;
        switch (command.getString("command")) {
            case "bounds": setBounds(command.getInt("x"), command.getInt("y"), command.getInt("width"), command.getInt("height")); break;
            case "navigate": navigate(command.getString("url")); break;
            case "back": if (canGoBack()) web.goBack(); break;
            case "forward": if (web.canGoForward()) web.goForward(); break;
            case "reload": web.reload(); break;
            case "focus_controls": web.clearFocus(); anchor.requestFocus(); break;
            case "focus_page": focusPage(); break;
        }
    }

    void setBounds(int x, int y, int width, int height) {
        if (closed) return;
        int[] surfaceLocation = new int[2], hostLocation = new int[2];
        anchor.getLocationInWindow(surfaceLocation);
        host.getLocationInWindow(hostLocation);
        FrameLayout.LayoutParams bounds = new FrameLayout.LayoutParams(Math.max(0, width), Math.max(0, height), Gravity.TOP | Gravity.LEFT);
        bounds.leftMargin = surfaceLocation[0] - hostLocation[0] + x;
        bounds.topMargin = surfaceLocation[1] - hostLocation[1] + y;
        web.setLayoutParams(bounds);
        web.setVisibility(width > 0 && height > 0 ? View.VISIBLE : View.INVISIBLE);
    }

    boolean isClosed() { return closed; }
    void backPressed() {
        if (!closed) {
            navigation();
            emit("back_pressed", null, null);
        }
    }

    private boolean atPageTop() { return web.getScrollY() == 0 && !web.canScrollVertically(-1); }

    private boolean pageTouch(MotionEvent event) {
        if (closed) return false;
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                emit("focused", null, null);
                pullCandidate = event.getPointerCount() == 1 && atPageTop();
                pullX = event.getX();
                pullY = event.getY();
                break;
            case MotionEvent.ACTION_MOVE:
                float dx = Math.abs(event.getX() - pullX), dy = event.getY() - pullY;
                if (event.getPointerCount() != 1 || !atPageTop() || dy < -touchSlop || (dx > touchSlop && dx > Math.abs(dy))) pullCandidate = false;
                break;
            case MotionEvent.ACTION_UP:
                float distance = event.getY() - pullY;
                boolean refresh = pullCandidate && event.getPointerCount() == 1 && atPageTop()
                        && distance >= refreshDistance && distance > Math.abs(event.getX() - pullX) * 1.5f
                        && BookDownload.webAddress(web.getUrl());
                pullCandidate = false;
                if (refresh) emit("refresh_requested", null, null);
                break;
            case MotionEvent.ACTION_CANCEL:
            case MotionEvent.ACTION_POINTER_DOWN:
            case MotionEvent.ACTION_POINTER_UP:
                pullCandidate = false;
                break;
        }
        // Observe the gesture without consuming webpage scrolling, taps or zoom.
        return false;
    }

    private boolean canGoBack() {
        android.webkit.WebBackForwardList history = web.copyBackForwardList();
        int previous = history.getCurrentIndex() - 1;
        return web.canGoBack() && previous >= 0 && !"about:blank".equals(history.getItemAtIndex(previous).getUrl());
    }

    void close() {
        if (closed) return;
        closed = true;
        web.stopLoading();
        ((ViewGroup) web.getParent()).removeView(web);
        web.destroy();
        for (WebView popup : popups) popup.destroy();
        popups.clear();
        anchor.requestFocus();
        // The backend releases staging only after all queued downloads have
        // reported completion. Closing the page never discards a finished book.
        worker.execute(() -> CookieManager.getInstance().flush());
        worker.shutdown();
        finishIfIdle();
    }

    private void finishIfIdle() {
        if (closed && transferring.isEmpty() && !finished) {
            finished = true;
            emit("closed", null, null);
        }
    }

    private void focusPage() {
        ((android.view.inputmethod.InputMethodManager) anchor.getContext().getSystemService(android.content.Context.INPUT_METHOD_SERVICE)).hideSoftInputFromWindow(anchor.getWindowToken(), 0);
        web.requestFocus();
    }

    private void navigate(String url) {
        if (closed) return;
        if (url.startsWith("magnet:")) {
            emit("magnet", "url", url);
        } else if (BookDownload.bookAddress(url)) {
            startDownload(url, web.getSettings().getUserAgentString(), null, null);
        } else if (BookDownload.webAddress(url) || url.equals("about:blank")) {
            if (!url.equals("about:blank")) focusPage();
            if (url.equals("about:blank") && !startPageHtml.isEmpty()) {
                web.loadDataWithBaseURL("about:blank", startPageHtml, "text/html", "UTF-8", null);
            } else web.loadUrl(url);
        } else emit("status", "message", "This link cannot open in the book browser");
    }

    private void navigation() {
        if (closed) return;
        try {
            emit(new JSONObject().put("kind", "navigation").put("url", web.getUrl() == null ? "about:blank" : web.getUrl())
                    .put("back", canGoBack()).put("forward", web.canGoForward()));
        } catch (org.json.JSONException impossible) { throw new IllegalStateException(impossible); }
    }

    void startDownload(String url, String agent, String disposition, String mime) {
        if (closed || transferring.contains(url)) return;
        if (!BookDownload.webAddress(url)) { emit("status", "message", "This download needs an HTTP or HTTPS link"); return; }
        transferring.add(url);
        BookDownload.Cancellation cancellation = new BookDownload.Cancellation();
        cancellations.put(url, cancellation);
        String referer = web.getUrl();
        String name;
        try { name = BookDownload.filename(url, disposition, mime); }
        catch (java.io.IOException unknown) { name = "Book download"; }
        final String downloadName = name;
        downloadEvent("download_started", url, downloadName, null, null, false);
        worker.execute(() -> {
            try {
                final long[] lastPercent = {-2};
                File file = BookDownload.save(url, agent, referer, disposition, mime, downloads, (bytes, total) -> {
                    long percent = total > 0 ? Math.min(100, bytes * 100 / total) : -1;
                    if (percent != lastPercent[0]) {
                        lastPercent[0] = percent;
                        ui.post(() -> { if (!cancellation.isCancelled()) downloadEvent("download_progress", url, null, percent < 0 ? null : (int) percent, null, false); });
                    }
                }, cancellation);
                ui.post(() -> {
                    if (cancellation.isCancelled()) {
                        file.delete();
                        transferring.remove(url);
                        cancellations.remove(url);
                        finishIfIdle();
                        return;
                    }
                    boolean torrent = file.getName().toLowerCase(java.util.Locale.ROOT).endsWith(".torrent");
                    try { emit(new JSONObject().put("kind", torrent ? "torrent_file" : "downloaded").put("path", file.getAbsolutePath()).put("download", url)); }
                    catch (org.json.JSONException impossible) { throw new IllegalStateException(impossible); }
                    downloadEvent("download_finished", url, file.getName(), null, null, torrent);
                    transferring.remove(url);
                    cancellations.remove(url);
                    finishIfIdle();
                });
            } catch (Exception error) {
                ui.post(() -> {
                    if (!cancellation.isCancelled()) downloadEvent("download_finished", url, downloadName, null, "Could not download the book: " + error.getMessage(), false);
                    transferring.remove(url);
                    cancellations.remove(url);
                    finishIfIdle();
                });
            }
        });
    }

    private void downloadEvent(String kind, String download, String name, Integer percent, String error, boolean torrent) {
        try {
            JSONObject event = new JSONObject().put("kind", kind).put("download", download);
            if (name != null) event.put("name", name);
            if (kind.equals("download_progress")) event.put("percent", percent == null ? JSONObject.NULL : percent);
            if (error != null) event.put("error", error);
            if (torrent) event.put("torrent", true);
            emit(event);
        }
        catch (org.json.JSONException impossible) { throw new IllegalStateException(impossible); }
    }
    private void emit(String kind, String key, String value) {
        try {
            JSONObject event = new JSONObject().put("kind", kind);
            if (key != null) event.put(key, value);
            emit(event);
        } catch (org.json.JSONException impossible) { throw new IllegalStateException(impossible); }
    }
    private void emit(JSONObject event) { events.send(sessionId, event.toString()); }
}
