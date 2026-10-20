package se.bokheim.reader.gpui;

import static org.junit.Assert.*;
import android.app.Activity;
import android.webkit.CookieManager;
import java.io.BufferedReader;
import java.io.File;
import java.io.InputStreamReader;
import java.io.IOException;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.List;
import java.util.HashMap;
import java.util.Map;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import org.json.JSONObject;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.Robolectric;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.Shadows;
import org.robolectric.android.controller.ActivityController;
import org.robolectric.annotation.Config;

@RunWith(RobolectricTestRunner.class)
@Config(sdk = 30)
public class BookBrowserTest {
    private ActivityController<Activity> controller;
    private Activity activity;
    private TestServer server;
    private File directory;
    private BookBrowserSurface browser;
    private android.widget.FrameLayout host;
    private android.view.View anchor;

    @Before public void setUp() throws Exception {
        controller = Robolectric.buildActivity(Activity.class).setup();
        activity = controller.get();
        host = new android.widget.FrameLayout(activity);
        anchor = new android.view.View(activity);
        anchor.setFocusableInTouchMode(true);
        host.addView(anchor, new android.widget.FrameLayout.LayoutParams(-1, -1));
        activity.setContentView(host);
        directory = Files.createTempDirectory(activity.getCacheDir().toPath(), "browser-").toFile();
        server = new TestServer();
        CookieManager.getInstance().removeAllCookies(null);
    }

    @After public void tearDown() throws Exception {
        BookDownload.transfersAllowed = () -> true;
        if (browser != null) browser.close();
        server.close();
        controller.pause().stop().destroy();
        for (File file : directory.listFiles()) file.delete();
        directory.delete();
    }

    private String url(String path) { return "http://127.0.0.1:" + server.socket.getLocalPort() + path; }

    @Test public void startPageLoadsLocalShortcutsAndReportsMetadataAfterNavigation() throws Exception {
        List<JSONObject> events = new ArrayList<>();
        String html = "<html><body><a href=\"https://example.com\">Example books</a></body></html>";
        String script = "({url:location.href,title:document.title,icon:null})";
        JSONObject request = new JSONObject().put("url", "about:blank").put("downloads", directory.getAbsolutePath())
            .put("start_page_html", html).put("page_metadata_script", script);
        browser = new BookBrowserSurface(activity, 26, request, anchor, (id, message) -> {
            try { events.add(new JSONObject(message)); } catch (Exception error) { throw new AssertionError(error); }
        });
        org.robolectric.shadows.ShadowWebView page = Shadows.shadowOf(browser.web);
        assertEquals(html, page.getLastLoadDataWithBaseURL().data);
        assertEquals("about:blank", page.getLastLoadDataWithBaseURL().baseUrl);
        browser.command(new JSONObject().put("command", "navigate").put("url", "https://example.com/books"));
        page.getWebViewClient().onPageFinished(browser.web, "https://example.com/books");
        assertEquals(script, page.getLastEvaluatedJavascript());
        page.getLastEvaluatedJavascriptCallback().onReceiveValue("{\"url\":\"https://example.com/books\",\"title\":\"Example books\",\"icon\":\"https://example.com/icon.png\"}");
        JSONObject visited = events.get(events.size() - 1);
        assertEquals("page_visited", visited.getString("kind"));
        assertEquals("Example books", visited.getString("title"));
        assertEquals("https://example.com/icon.png", visited.getString("icon"));

        // A delayed result from the old page must not count the new document.
        browser.command(new JSONObject().put("command", "navigate").put("url", "https://example.com/other"));
        int before = events.size();
        page.getLastEvaluatedJavascriptCallback().onReceiveValue("{\"url\":\"https://example.com/books\",\"title\":\"Old page\",\"icon\":null}");
        assertEquals(before, events.size());
    }

    private android.webkit.WebView popup() {
        android.webkit.WebView.WebViewTransport transport = browser.web.new WebViewTransport();
        android.os.Message result = new android.os.Handler(android.os.Looper.getMainLooper()).obtainMessage(0, transport);
        assertTrue(Shadows.shadowOf(browser.web).getWebChromeClient().onCreateWindow(browser.web, false, true, result));
        return transport.getWebView();
    }

    private JSONObject awaitEvent(List<JSONObject> events, String kind) throws Exception {
        long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5);
        while (System.nanoTime() < deadline) {
            Shadows.shadowOf(android.os.Looper.getMainLooper()).idle();
            for (JSONObject event : events) if (kind.equals(event.getString("kind"))) return event;
            Thread.sleep(10);
        }
        throw new AssertionError("Missing browser event: " + kind);
    }

    @Test public void popupAttachmentDownloadsReportProgressAndReachTheLibrary() throws Exception {
        byte[] body = "a complete epub".getBytes(StandardCharsets.UTF_8);
        server.serve("/attachment", request -> {
            assertEquals("session=signed-in", request.headers.get("cookie"));
            request.respond(200, "Content-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"popup.epub\"\r\n", body, body.length);
        });
        server.start();
        CookieManager.getInstance().setCookie(url("/"), "session=signed-in");
        List<JSONObject> events = new ArrayList<>();
        browser = new BookBrowserSurface(activity, 23, new JSONObject().put("url", "about:blank").put("downloads", directory.getAbsolutePath()), anchor, (id, message) -> {
            try { events.add(new JSONObject(message)); } catch (Exception error) { throw new AssertionError(error); }
        });
        android.webkit.WebView popup = popup();
        assertTrue(popup.getSettings().getJavaScriptEnabled());
        assertFalse(popup.getSettings().getAllowFileAccess());
        android.webkit.DownloadListener listener = Shadows.shadowOf(popup).getDownloadListener();
        assertNotNull("New-window attachments need a download listener", listener);
        listener.onDownloadStart(url("/attachment"), browser.web.getSettings().getUserAgentString(), "attachment; filename=\"popup.epub\"", "application/octet-stream", body.length);
        assertEquals("download_started", events.get(events.size() - 1).getString("kind"));
        assertEquals("popup.epub", events.get(events.size() - 1).getString("name"));
        // The staging owner must stay alive even when the browser closes.
        browser.close();
        JSONObject downloaded = awaitEvent(events, "downloaded");
        assertArrayEquals(body, Files.readAllBytes(new File(downloaded.getString("path")).toPath()));
        assertFalse(awaitEvent(events, "download_finished").has("error"));
        awaitEvent(events, "closed");
        assertTrue(events.stream().anyMatch(event -> event.optString("kind").equals("download_progress")));
    }

    @Test public void popupBookLinksStartDownloadsBeforeLoadingAPage() throws Exception {
        server.serve("/popup.pdf", request -> request.respond(200, "Content-Type: application/pdf\r\n", new byte[]{1, 2, 3}, 3));
        server.start();
        List<JSONObject> events = new ArrayList<>();
        browser = new BookBrowserSurface(activity, 24, new JSONObject().put("url", "about:blank").put("downloads", directory.getAbsolutePath()), anchor, (id, message) -> {
            try { events.add(new JSONObject(message)); } catch (Exception error) { throw new AssertionError(error); }
        });
        android.webkit.WebView popup = popup();
        assertTrue(Shadows.shadowOf(popup).getWebViewClient().shouldOverrideUrlLoading(popup, url("/popup.pdf")));
        assertEquals("download_started", events.get(events.size() - 1).getString("kind"));
        assertEquals("about:blank", browser.web.getUrl());
        awaitEvent(events, "downloaded");
        awaitEvent(events, "download_finished");
    }

    private void pageTouch(int action, float x, float y) {
        float density = activity.getResources().getDisplayMetrics().density;
        android.view.MotionEvent event = android.view.MotionEvent.obtain(0, 16, action, x * density, y * density, 0);
        try { browser.web.dispatchTouchEvent(event); } finally { event.recycle(); }
    }

    private void pullDown() {
        pageTouch(android.view.MotionEvent.ACTION_DOWN, 10, 10);
        pageTouch(android.view.MotionEvent.ACTION_MOVE, 12, 130);
        pageTouch(android.view.MotionEvent.ACTION_UP, 12, 150);
    }

    @Test public void systemBackReportsCurrentHistoryAndSkipsTheInitialBlankPage() throws Exception {
        List<JSONObject> events = new ArrayList<>();
        browser = new BookBrowserSurface(activity, 22, new JSONObject().put("url", "about:blank").put("downloads", directory.getAbsolutePath()), anchor, (id, message) -> {
            try { events.add(new JSONObject(message)); } catch (Exception error) { throw new AssertionError(error); }
        });
        browser.web.clearHistory();
        Shadows.shadowOf(browser.web).pushEntryToHistory("about:blank");
        Shadows.shadowOf(browser.web).pushEntryToHistory("https://example.com/first");
        browser.backPressed();
        assertEquals("navigation", events.get(events.size() - 2).getString("kind"));
        assertFalse(events.get(events.size() - 2).getBoolean("back"));
        assertEquals("back_pressed", events.get(events.size() - 1).getString("kind"));
        Shadows.shadowOf(browser.web).pushEntryToHistory("https://example.com/second");
        browser.backPressed();
        assertTrue(events.get(events.size() - 2).getBoolean("back"));
        browser.command(new JSONObject().put("command", "back"));
        assertEquals(1, Shadows.shadowOf(browser.web).getGoBackInvocations());
        browser.backPressed();
        assertFalse(events.get(events.size() - 2).getBoolean("back"));
    }

    @Test public void hardwareBackBypassesGameActivityAndConsumesTheWholeKeyPress() throws Exception {
        List<JSONObject> events = new ArrayList<>();
        browser = new BookBrowserSurface(activity, 27, new JSONObject().put("url", "about:blank").put("downloads", directory.getAbsolutePath()), anchor, (id, message) -> {
            try { events.add(new JSONObject(message)); } catch (Exception error) { throw new AssertionError(error); }
        });
        // Construct only the router: GameActivity's native surface is not
        // needed to verify that browser Back never reaches its key handler.
        MainActivity router = Robolectric.buildActivity(MainActivity.class).get();
        Map<Long, BookBrowserSurface> browsers = org.robolectric.util.ReflectionHelpers.getField(router, "bookBrowsers");
        AtomicInteger navigation = new AtomicInteger();
        org.robolectric.util.ReflectionHelpers.setField(router, "backButtons", new BackButtonRouter(navigation::incrementAndGet));
        browsers.put(27L, browser);
        assertTrue(router.dispatchKeyEvent(new android.view.KeyEvent(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_BACK)));
        assertEquals(1, navigation.get());
        int afterDown = navigation.get();
        assertTrue(router.dispatchKeyEvent(new android.view.KeyEvent(0, 0, android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_BACK, 1)));
        assertEquals(afterDown, navigation.get());
        // GPUI may close the browser before Android delivers key-up.
        browsers.clear();
        assertTrue(router.dispatchKeyEvent(new android.view.KeyEvent(android.view.KeyEvent.ACTION_UP, android.view.KeyEvent.KEYCODE_BACK)));
        assertEquals(afterDown, navigation.get());
    }

    @Test public void pullDownRefreshOnlyStartsAtPageTopAndIgnoresOtherGestures() throws Exception {
        List<JSONObject> events = new ArrayList<>();
        browser = new BookBrowserSurface(activity, 21, new JSONObject().put("url", "about:blank").put("downloads", directory.getAbsolutePath()), anchor, (id, message) -> {
            try { events.add(new JSONObject(message)); } catch (Exception error) { throw new AssertionError(error); }
        });
        browser.setBounds(0, 80, 360, 600);
        pullDown();
        assertEquals(0, events.stream().filter(event -> event.optString("kind").equals("refresh_requested")).count());
        browser.command(new JSONObject().put("command", "navigate").put("url", "https://example.com/page"));
        pullDown();
        assertEquals(1, events.stream().filter(event -> event.optString("kind").equals("refresh_requested")).count());
        pageTouch(android.view.MotionEvent.ACTION_DOWN, 10, 10);
        pageTouch(android.view.MotionEvent.ACTION_UP, 10, 60);
        pageTouch(android.view.MotionEvent.ACTION_DOWN, 10, 10);
        pageTouch(android.view.MotionEvent.ACTION_MOVE, 150, 20);
        pageTouch(android.view.MotionEvent.ACTION_UP, 160, 160);
        pageTouch(android.view.MotionEvent.ACTION_DOWN, 10, 150);
        pageTouch(android.view.MotionEvent.ACTION_MOVE, 10, 80);
        pageTouch(android.view.MotionEvent.ACTION_UP, 10, 280);
        browser.web.scrollTo(0, 80);
        pageTouch(android.view.MotionEvent.ACTION_DOWN, 10, 10);
        browser.web.scrollTo(0, 0);
        pageTouch(android.view.MotionEvent.ACTION_MOVE, 10, 130);
        pageTouch(android.view.MotionEvent.ACTION_UP, 10, 150);
        pageTouch(android.view.MotionEvent.ACTION_DOWN, 10, 10);
        pageTouch(android.view.MotionEvent.ACTION_CANCEL, 10, 130);
        pageTouch(android.view.MotionEvent.ACTION_UP, 10, 150);
        pageTouch(android.view.MotionEvent.ACTION_DOWN, 10, 10);
        pageTouch(android.view.MotionEvent.ACTION_POINTER_DOWN, 10, 20);
        pageTouch(android.view.MotionEvent.ACTION_UP, 10, 150);
        assertEquals(1, events.stream().filter(event -> event.optString("kind").equals("refresh_requested")).count());
        pullDown();
        assertEquals(2, events.stream().filter(event -> event.optString("kind").equals("refresh_requested")).count());
    }

    @Test public void downloadsHaveSeparateProgressAndCompletionEventsEvenAfterClosing() throws Exception {
        CountDownLatch requested = new CountDownLatch(1), complete = new CountDownLatch(1);
        server.serve("/one.epub", request -> {
            requested.countDown();
            try { complete.await(5, TimeUnit.SECONDS); } catch (InterruptedException error) { Thread.currentThread().interrupt(); }
            request.respond(200, "", new byte[]{1, 2, 3, 4}, 4);
        });
        server.serve("/two.pdf", request -> request.respond(500, "", new byte[0], 0));
        server.start();
        List<JSONObject> events = new ArrayList<>();
        browser = new BookBrowserSurface(activity, 20, new JSONObject().put("url", "about:blank").put("downloads", directory.getAbsolutePath()), anchor, (id, message) -> {
            try { events.add(new JSONObject(message)); } catch (Exception error) { throw new AssertionError(error); }
        });
        browser.startDownload(url("/one.epub"), null, null, null);
        browser.startDownload(url("/two.pdf"), null, null, null);
        assertTrue(requested.await(5, TimeUnit.SECONDS));
        browser.close();
        complete.countDown();
        long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5);
        while (!events.get(events.size() - 1).getString("kind").equals("closed") && System.nanoTime() < deadline) {
            Shadows.shadowOf(android.os.Looper.getMainLooper()).idle();
            Thread.sleep(10);
        }
        Map<String, JSONObject> started = new HashMap<>(), finished = new HashMap<>();
        boolean progress = false;
        for (JSONObject event : events) {
            String kind = event.getString("kind");
            if (kind.equals("download_started")) started.put(event.getString("download"), event);
            if (kind.equals("download_finished")) finished.put(event.getString("download"), event);
            if (kind.equals("download_progress")) {
                assertEquals(url("/one.epub"), event.getString("download"));
                assertEquals(100, event.getInt("percent"));
                progress = true;
            }
        }
        assertEquals(2, started.size());
        assertEquals("one.epub", started.get(url("/one.epub")).getString("name"));
        assertEquals(2, finished.size());
        assertFalse(finished.get(url("/one.epub")).has("error"));
        assertTrue(finished.get(url("/two.pdf")).getString("error").contains("HTTP 500"));
        assertTrue(progress);
        assertEquals("closed", events.get(events.size() - 1).getString("kind"));
    }

    @Test public void magnetClicksAreHandedToRustWithoutNavigatingThePage() throws Exception {
        List<String> events = new ArrayList<>();
        JSONObject request = new JSONObject().put("url", "about:blank").put("downloads", directory.getAbsolutePath());
        browser = new BookBrowserSurface(activity, 13, request, anchor, (id, message) -> events.add(message));
        String magnet = "magnet:?xt=urn:btih:0123456789012345678901234567890123456789";
        android.webkit.WebResourceRequest link = new android.webkit.WebResourceRequest() {
            public android.net.Uri getUrl() { return android.net.Uri.parse(magnet); }
            public boolean isForMainFrame() { return true; }
            public boolean isRedirect() { return false; }
            public boolean hasGesture() { return true; }
            public String getMethod() { return "GET"; }
            public Map<String, String> getRequestHeaders() { return java.util.Collections.emptyMap(); }
        };
        assertTrue(Shadows.shadowOf(browser.web).getWebViewClient().shouldOverrideUrlLoading(browser.web, link));
        JSONObject event = new JSONObject(events.get(events.size() - 1));
        assertEquals("magnet", event.getString("kind"));
        assertEquals(magnet, event.getString("url"));
        assertEquals("about:blank", browser.web.getUrl());
    }

    @Test public void torrentDownloadsUseBrowserCookiesAndAreNotReportedAsBooks() throws Exception {
        byte[] metadata = "torrent metadata".getBytes(StandardCharsets.UTF_8);
        server.serve("/download", request -> {
            assertEquals("session=signed-in", request.headers.get("cookie"));
            request.respond(200, "Content-Type: application/x-bittorrent\r\n", metadata, metadata.length);
        });
        server.start();
        CookieManager.getInstance().setCookie(url("/"), "session=signed-in");
        List<String> events = new ArrayList<>();
        JSONObject request = new JSONObject().put("url", "about:blank").put("downloads", directory.getAbsolutePath());
        browser = new BookBrowserSurface(activity, 14, request, anchor, (id, message) -> events.add(message));
        browser.startDownload(url("/download"), "Bokheim-test", null, null);
        long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5);
        JSONObject torrent = null;
        while (torrent == null && System.nanoTime() < deadline) {
            Shadows.shadowOf(android.os.Looper.getMainLooper()).idle();
            for (String message : events) {
                JSONObject event = new JSONObject(message);
                assertNotEquals("downloaded", event.getString("kind"));
                if (event.getString("kind").equals("torrent_file")) torrent = event;
            }
            Thread.sleep(10);
        }
        assertNotNull("torrent metadata reaches Rust", torrent);
        assertEquals(url("/download"), torrent.getString("download"));
        File file = new File(torrent.getString("path"));
        assertTrue(file.getName().endsWith(".torrent"));
        assertArrayEquals(metadata, Files.readAllBytes(file.toPath()));
    }

    @Test public void downloadedBookUsesBrowserCookiesAgentAndRedirects() throws Exception {
        List<String> headers = new ArrayList<>();
        server.serve("/start", request -> request.respond(302, "Location: /book\r\n", new byte[0], 0));
        server.serve("/book", request -> {
            headers.add(request.headers.get("cookie"));
            headers.add(request.headers.get("user-agent"));
            headers.add(request.headers.get("referer"));
            byte[] body = "a complete epub".getBytes(StandardCharsets.UTF_8);
            request.respond(200, "Content-Type: application/epub+zip\r\nContent-Disposition: attachment; filename=\"my-book.epub\"\r\n", body, body.length);
        });
        server.start();
        CookieManager.getInstance().setCookie(url("/"), "session=signed-in");
        File file = BookDownload.save(url("/start"), "Bokheim-test", url("/store?private=token"), null, null, directory, (bytes, total) -> {});
        assertEquals("my-book.epub", file.getName());
        assertEquals("a complete epub", new String(Files.readAllBytes(file.toPath()), StandardCharsets.UTF_8));
        assertEquals("session=signed-in", headers.get(0));
        assertEquals("Bokheim-test", headers.get(1));
        assertEquals(url("/"), headers.get(2));
        File second = BookDownload.save(url("/book"), "Bokheim-test", null, null, null, directory, (bytes, total) -> {});
        assertNotEquals(file, second);
        assertEquals("a complete epub", new String(Files.readAllBytes(file.toPath()), StandardCharsets.UTF_8));
    }

    @Test public void meteredDownloadsWaitAndRestartWithoutPublishingPartialFiles() throws Exception {
        java.util.concurrent.atomic.AtomicBoolean allowed = new java.util.concurrent.atomic.AtomicBoolean(false);
        java.util.concurrent.atomic.AtomicBoolean interrupted = new java.util.concurrent.atomic.AtomicBoolean(false);
        java.util.concurrent.atomic.AtomicInteger requests = new java.util.concurrent.atomic.AtomicInteger();
        BookDownload.transfersAllowed = allowed::get;
        byte[] body = new byte[512 * 1024];
        java.util.Arrays.fill(body, (byte) 42);
        server.serve("/metered.epub", request -> {
            requests.incrementAndGet();
            request.respond(200, "Content-Type: application/epub+zip\r\n", body, body.length);
        });
        server.start();
        CountDownLatch admission = new CountDownLatch(1);
        CountDownLatch suspended = new CountDownLatch(1);
        ExecutorService download = Executors.newSingleThreadExecutor();
        try {
            java.util.concurrent.Future<File> completed = download.submit(() -> BookDownload.save(url("/metered.epub"), null, null, null, null, directory,
                (bytes, total) -> { if (interrupted.compareAndSet(false, true)) allowed.set(false); },
                () -> { if (interrupted.get()) suspended.countDown(); else admission.countDown(); }));
            assertTrue(admission.await(5, TimeUnit.SECONDS));
            assertEquals(0, requests.get());
            assertEquals(0, directory.list().length);
            allowed.set(true);
            assertTrue(suspended.await(5, TimeUnit.SECONDS));
            assertFalse(completed.isDone());
            assertEquals(0, directory.list().length);
            allowed.set(true);
            File file = completed.get(10, TimeUnit.SECONDS);
            assertArrayEquals(body, Files.readAllBytes(file.toPath()));
            assertEquals(2, requests.get());
            assertEquals(1, directory.list().length);
        } finally { allowed.set(true); download.shutdownNow(); }
    }

    @Test public void cancellingAWaitingDownloadDoesNotFetchOrImportIt() throws Exception {
        BookDownload.transfersAllowed = () -> false;
        List<JSONObject> events = new ArrayList<>();
        browser = new BookBrowserSurface(activity, 31, new JSONObject().put("url", "about:blank").put("downloads", directory.getAbsolutePath()), anchor, (id, message) -> {
            try { events.add(new JSONObject(message)); } catch (Exception error) { throw new AssertionError(error); }
        });
        String address = url("/waiting.epub");
        browser.startDownload(address, null, null, null);
        awaitEvent(events, "download_waiting");
        browser.command(new JSONObject().put("command", "cancel_download").put("download", address));
        assertEquals(address, awaitEvent(events, "cancel_download").getString("download"));
        browser.close();
        awaitEvent(events, "closed");
        assertFalse(events.stream().anyMatch(event -> event.optString("kind").equals("downloaded") || event.optString("kind").equals("download_finished")));
        assertEquals(0, directory.list().length);
    }

    @Test public void cancellingAnActiveDownloadDeletesItsPartialFile() throws Exception {
        byte[] body = new byte[256 * 1024];
        server.serve("/cancel.epub", request -> request.respond(200, "", body, body.length));
        server.start();
        BookDownload.Cancellation cancellation = new BookDownload.Cancellation();
        assertThrows(IOException.class, () -> BookDownload.save(url("/cancel.epub"), null, null, null, null, directory,
                (bytes, total) -> cancellation.cancel(), () -> {}, cancellation));
        assertTrue(cancellation.isCancelled());
        assertEquals(0, directory.list().length);
    }

    @Test public void incompleteAndLoginPagesDoNotReachTheLibrary() throws Exception {
        server.serve("/broken.epub", request -> request.respond(200, "", new byte[]{1, 2, 3}, 20));
        server.serve("/login.epub", request -> request.respond(200, "Content-Type: text/html\r\n", new byte[]{1, 2, 3}, 3));
        server.start();
        assertThrows(java.io.IOException.class, () -> BookDownload.save(url("/broken.epub"), null, null, null, null, directory, (bytes, total) -> {}));
        assertThrows(java.io.IOException.class, () -> BookDownload.save(url("/login.epub"), null, null, null, null, directory, (bytes, total) -> {}));
        assertEquals(0, directory.list().length);
    }

    @Test public void webNavigationAndNamesStayInsideTheBrowserAndStagingFolder() throws Exception {
        for (String value : new String[]{"javascript:alert(1)", "file:///tmp/book.epub", "https://user:password@example.com"}) {
            assertFalse(BookDownload.webAddress(value));
        }
        String filename = BookDownload.filename(url("/book"), "attachment; filename=\"../../book.epub\"", "application/epub+zip");
        assertFalse(filename.contains("/"));
        assertFalse(filename.startsWith("."));
        assertTrue(filename.endsWith(".epub"));
        assertEquals("book.epub", BookDownload.filename(url("/download"), "attachment; filename=\"book.epub\"", "application/octet-stream"));
        assertTrue(BookDownload.bookAddress(url("/book.PDF?download=1")));
    }

    @Test public void closingBrowserWaitsForTheBookBeforeReleasingItsLibraryFolder() throws Exception {
        CountDownLatch requested = new CountDownLatch(1);
        CountDownLatch complete = new CountDownLatch(1);
        server.serve("/book.epub", request -> {
            requested.countDown();
            try { complete.await(5, TimeUnit.SECONDS); } catch (InterruptedException error) { Thread.currentThread().interrupt(); }
            request.respond(200, "", new byte[]{1, 2, 3, 4}, 4);
        });
        server.start();
        List<String> events = new ArrayList<>();
        JSONObject request = new JSONObject().put("url", "about:blank").put("downloads", directory.getAbsolutePath());
        browser = new BookBrowserSurface(activity, 12, request, anchor, (id, message) -> {
            assertEquals(12, id);
            try {
                String kind = new JSONObject(message).getString("kind");
                if (kind.equals("opened") || kind.equals("downloaded") || kind.equals("closed")) events.add(message);
            } catch (Exception error) { throw new AssertionError(error); }
        });
        assertEquals("opened", new JSONObject(events.get(0)).getString("kind"));
        assertTrue(browser.web.getSettings().getJavaScriptEnabled());
        assertTrue(browser.web.getSettings().getDomStorageEnabled());
        assertFalse(browser.web.getSettings().getAllowFileAccess());
        browser.startDownload(url("/book.epub"), null, null, null);
        assertTrue(requested.await(5, TimeUnit.SECONDS));
        browser.close();
        assertEquals(1, events.size());
        complete.countDown();
        long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5);
        while (events.size() < 3 && System.nanoTime() < deadline) {
            Shadows.shadowOf(android.os.Looper.getMainLooper()).idle();
            Thread.sleep(10);
        }
        assertEquals(3, events.size());
        JSONObject downloaded = new JSONObject(events.get(1));
        assertEquals("downloaded", downloaded.getString("kind"));
        assertEquals(directory, new File(downloaded.getString("path")).getParentFile());
        assertEquals("closed", new JSONObject(events.get(2)).getString("kind"));
    }

    @Test public void nativePageUsesGpuiBoundsAndHasNoAndroidBrowserControls() throws Exception {
        List<JSONObject> events = new ArrayList<>();
        browser = new BookBrowserSurface(activity, 19, new JSONObject().put("url", "about:blank").put("downloads", directory.getAbsolutePath()), anchor, (id, message) -> {
            try { events.add(new JSONObject(message)); } catch (Exception error) { throw new AssertionError(error); }
        });
        assertEquals(2, host.getChildCount());
        assertSame(anchor, host.getChildAt(0));
        assertSame(browser.web, host.getChildAt(1));
        assertEquals(android.view.View.INVISIBLE, browser.web.getVisibility());
        browser.command(new JSONObject().put("command", "bounds").put("x", 10).put("y", 90).put("width", 320).put("height", 500));
        android.widget.FrameLayout.LayoutParams bounds = (android.widget.FrameLayout.LayoutParams) browser.web.getLayoutParams();
        assertEquals(10, bounds.leftMargin);
        assertEquals(90, bounds.topMargin);
        assertEquals(320, bounds.width);
        assertEquals(500, bounds.height);
        assertEquals(android.view.View.VISIBLE, browser.web.getVisibility());
        browser.command(new JSONObject().put("command", "navigate").put("url", "https://example.com/books"));
        assertEquals("https://example.com/books", Shadows.shadowOf(browser.web).getLastLoadedUrl());
        browser.command(new JSONObject().put("command", "focus_controls"));
        assertTrue(anchor.hasFocus());
        browser.backPressed();
        assertEquals("back_pressed", events.get(events.size() - 1).getString("kind"));
        browser.setBounds(10, 120, 240, 200);
        assertEquals(200, browser.web.getLayoutParams().height);
        browser.close();
        assertEquals(1, host.getChildCount());
        assertSame(anchor, host.getChildAt(0));
    }

    @Test public void inlineChallengeFramesAreAllowedWithoutChangingTheMainPage() throws Exception {
        List<JSONObject> events = new ArrayList<>();
        browser = new BookBrowserSurface(activity, 25, new JSONObject().put("url", "about:blank").put("downloads", directory.getAbsolutePath()), anchor, (id, message) -> {
            try { events.add(new JSONObject(message)); } catch (Exception error) { throw new AssertionError(error); }
        });
        android.webkit.WebViewClient client = Shadows.shadowOf(browser.web).getWebViewClient();
        int initialEvents = events.size();
        assertFalse(client.shouldOverrideUrlLoading(browser.web, frameRequest("about:srcdoc", false)));
        assertFalse(client.shouldOverrideUrlLoading(browser.web, frameRequest("about:blank", false)));
        assertFalse(client.shouldOverrideUrlLoading(browser.web, frameRequest("https://challenges.cloudflare.com/widget", false)));
        assertEquals(initialEvents, events.size());
        assertEquals("about:blank", browser.web.getUrl());
        assertTrue(client.shouldOverrideUrlLoading(browser.web, frameRequest("javascript:alert(1)", false)));
        assertTrue(client.shouldOverrideUrlLoading(browser.web, frameRequest("file:///tmp/book.epub", false)));
        assertTrue(client.shouldOverrideUrlLoading(browser.web, frameRequest("about:srcdoc", true)));
    }

    private static android.webkit.WebResourceRequest frameRequest(String url, boolean mainFrame) {
        return new android.webkit.WebResourceRequest() {
            public android.net.Uri getUrl() { return android.net.Uri.parse(url); }
            public boolean isForMainFrame() { return mainFrame; }
            public boolean isRedirect() { return false; }
            public boolean hasGesture() { return false; }
            public String getMethod() { return "GET"; }
            public Map<String, String> getRequestHeaders() { return java.util.Collections.emptyMap(); }
        };
    }

    /** A loopback HTTP server using the Java APIs Android unit tests compile against. */
    private static final class TestServer implements AutoCloseable {
        interface Reply { void send(Request request) throws IOException; }
        final ServerSocket socket = new ServerSocket(0, 8, java.net.InetAddress.getByName("127.0.0.1"));
        final Map<String, Reply> replies = new HashMap<>();
        final ExecutorService worker = Executors.newSingleThreadExecutor();
        TestServer() throws IOException { }
        void serve(String path, Reply reply) { replies.put(path, reply); }
        void start() {
            worker.execute(() -> {
                while (!socket.isClosed()) {
                    try (Socket connection = socket.accept()) {
                        connection.setSoTimeout(5000);
                        BufferedReader input = new BufferedReader(new InputStreamReader(connection.getInputStream(), StandardCharsets.UTF_8));
                        String path = input.readLine().split(" ")[1];
                        Request request = new Request(connection);
                        String line;
                        while ((line = input.readLine()) != null && !line.isEmpty()) {
                            int colon = line.indexOf(':');
                            request.headers.put(line.substring(0, colon).toLowerCase(java.util.Locale.ROOT), line.substring(colon + 1).trim());
                        }
                        Reply reply = replies.get(path);
                        if (reply == null) request.respond(404, "", new byte[0], 0);
                        else reply.send(request);
                    } catch (IOException error) {
                        if (!socket.isClosed()) throw new RuntimeException(error);
                    }
                }
            });
        }
        @Override public void close() throws IOException { socket.close(); worker.shutdownNow(); }
        static final class Request {
            final Socket connection;
            final Map<String, String> headers = new HashMap<>();
            Request(Socket connection) { this.connection = connection; }
            void respond(int status, String headers, byte[] body, int length) throws IOException {
                String head = "HTTP/1.1 " + status + " Test\r\nConnection: close\r\nContent-Length: " + length + "\r\n" + headers + "\r\n";
                connection.getOutputStream().write(head.getBytes(StandardCharsets.UTF_8));
                connection.getOutputStream().write(body);
                connection.getOutputStream().flush();
            }
        }
    }
}
