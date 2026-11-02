package se.bokheim.reader.gpui;

import android.webkit.CookieManager;
import android.webkit.URLUtil;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.UUID;

/** Streams authenticated browser downloads into the library's hidden staging
 * folder. Only complete files are handed to Rust for placement and import. */
final class BookDownload {
    static final class Cancellation {
        private volatile boolean cancelled;
        private HttpURLConnection connection;
        boolean isCancelled() { return cancelled; }
        void cancel() {
            HttpURLConnection active;
            synchronized (this) { cancelled = true; active = connection; }
            if (active != null) {
                Thread disconnect = new Thread(active::disconnect, "book-download-cancel");
                disconnect.setDaemon(true);
                disconnect.start();
            }
        }
        void check() throws IOException { if (cancelled) throw new IOException(UiText.ANDROID_DOWNLOAD_CANCELLED); }
        synchronized void connected(HttpURLConnection active) throws IOException {
            check();
            connection = active;
        }
        synchronized void disconnected(HttpURLConnection active) { if (connection == active) connection = null; }
    }
    interface Progress { void update(long bytes, long total); }

    static boolean webAddress(String value) {
        if (value == null || value.matches(".*\\s.*")) return false;
        try {
            URL url = new URL(value);
            return (url.getProtocol().equals("https") || url.getProtocol().equals("http"))
                    && !url.getHost().isEmpty() && url.getUserInfo() == null;
        } catch (Exception invalid) { return false; }
    }

    static boolean bookAddress(String value) {
        if (!webAddress(value)) return false;
        return BookFormats.bookName(android.net.Uri.parse(value).getLastPathSegment());
    }


    static String filename(String url, String disposition, String mime) throws IOException {
        // Generic application/octet-stream responses must retain a server's
        // .epub/.pdf filename rather than URLUtil replacing it with .bin.
        String name = URLUtil.guessFileName(url, disposition, null);
        // Servers choose the name, never the destination directory.
        name = name.replaceAll("[\\\\/:*?\"<>|\\p{Cntrl}]", "_").replaceAll("^\\.+", "");
        if (name.length() > 180) name = name.substring(name.length() - 180);
        if (!BookFormats.bookName(name)) {
            String extension = BookFormats.extensionForMime(mime);
            if (extension == null) throw new IOException(UiText.ANDROID_THIS_DOWNLOAD_IS_NOT_A_SUPPORTED_BOOK_FILE);
            name = (name.isEmpty() ? "book" : name) + extension;
        }
        return name;
    }

    static File save(String address, String userAgent, String referer, String disposition,
                     String mime, File directory, Progress progress) throws IOException {
        return save(address, userAgent, referer, disposition, mime, directory, progress, new Cancellation());
    }

    static File save(String address, String userAgent, String referer, String disposition,
                     String mime, File directory, Progress progress, Cancellation cancellation) throws IOException {
        // Browser downloads are explicit user requests, regardless of network cost.
        cancellation.check();
        if (!webAddress(address)) throw new IOException(UiText.ANDROID_THIS_DOWNLOAD_NEEDS_AN_HTTP_OR_HTTPS_LINK);
        HttpURLConnection connection = null;
        File partial = null;
        try {
            URL url = new URL(address);
            CookieManager cookies = CookieManager.getInstance();
            for (int redirects = 0; ; redirects++) {
                cancellation.check();
                connection = (HttpURLConnection) url.openConnection();
                cancellation.connected(connection);
                connection.setInstanceFollowRedirects(false);
                connection.setConnectTimeout(15000);
                connection.setReadTimeout(30000);
                connection.setRequestProperty("Accept-Encoding", "identity");
                if (userAgent != null) connection.setRequestProperty("User-Agent", userAgent);
                String cookie = cookies.getCookie(url.toString());
                if (cookie != null) connection.setRequestProperty("Cookie", cookie);
                if (webAddress(referer) && !(referer.startsWith("https:") && url.getProtocol().equals("http"))) {
                    // Origin-only referrers avoid carrying store tokens across redirects.
                    URL source = new URL(referer);
                    connection.setRequestProperty("Referer", source.getProtocol() + "://" + source.getAuthority() + "/");
                }
                cancellation.check();
                int status = connection.getResponseCode();
                cancellation.check();
                for (Map.Entry<String, List<String>> header : connection.getHeaderFields().entrySet()) {
                    if (header.getKey() != null && header.getKey().equalsIgnoreCase("Set-Cookie")) {
                        for (String value : header.getValue()) cookies.setCookie(url.toString(), value, null);
                    }
                }
                if (status == 301 || status == 302 || status == 303 || status == 307 || status == 308) {
                    if (redirects >= 10) throw new IOException(UiText.ANDROID_THE_DOWNLOAD_REDIRECTED_TOO_MANY_TIMES);
                    String location = connection.getHeaderField("Location");
                    if (location == null) throw new IOException(UiText.ANDROID_THE_DOWNLOAD_REDIRECT_HAS_NO_ADDRESS);
                    URL next = new URL(url, location);
                    if (!webAddress(next.toString())) throw new IOException(UiText.ANDROID_THE_DOWNLOAD_REDIRECTED_TO_AN_UNSUPPORTED_ADDRESS);
                    connection.disconnect();
                    url = next;
                    continue;
                }
                if (status != HttpURLConnection.HTTP_OK) {
                    android.util.Log.w("Bokheim", "Book download returned HTTP " + status);
                    throw new IOException(UiText.ANDROID_THE_DOWNLOAD_RETURNED_HTTP);
                }
                break;
            }
            String responseDisposition = connection.getHeaderField("Content-Disposition");
            String responseMime = connection.getContentType();
            if (responseMime != null && (responseMime.toLowerCase(Locale.ROOT).startsWith("text/html")
                    || responseMime.toLowerCase(Locale.ROOT).startsWith("application/xhtml+xml"))) {
                throw new IOException(UiText.ANDROID_THE_STORE_RETURNED_A_PAGE_INSTEAD_OF_THE_BOOK_SIGN_IN_AND);
            }
            String name = filename(url.toString(), responseDisposition == null ? disposition : responseDisposition,
                    responseMime == null ? mime : responseMime);
            File root = directory.getCanonicalFile();
            if (!root.isDirectory()) throw new IOException(UiText.ANDROID_THE_LIBRARY_DOWNLOAD_FOLDER_IS_UNAVAILABLE);
            cancellation.check();
            partial = File.createTempFile("download-", ".part", root);
            long total = connection.getContentLengthLong();
            long count = 0;
            try (InputStream input = connection.getInputStream(); FileOutputStream output = new FileOutputStream(partial)) {
                byte[] buffer = new byte[65536];
                int length;
                while ((length = input.read(buffer)) != -1) {
                    cancellation.check();
                    output.write(buffer, 0, length);
                    count += length;
                    progress.update(count, total);
                }
                output.getFD().sync();
            }
            if (count == 0 || (total >= 0 && count != total)) throw new IOException(UiText.ANDROID_THE_BOOK_DOWNLOAD_WAS_INCOMPLETE);
            cancellation.check();
            File completed = new File(root, name);
            if (completed.exists()) completed = new File(root, UUID.randomUUID() + "-" + name);
            if (!partial.renameTo(completed)) throw new IOException(UiText.ANDROID_COULD_NOT_FINISH_SAVING_THE_DOWNLOADED_BOOK);
            partial = null;
            return completed;
        } finally {
            if (connection != null) connection.disconnect();
            cancellation.disconnected(connection);
            if (partial != null) partial.delete();
        }
    }
}
