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
    static volatile java.util.function.BooleanSupplier transfersAllowed = () -> true;
    private static final class Deferred extends IOException {}
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
        void check() throws IOException { if (cancelled) throw new IOException("Download cancelled"); }
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
        return bookName(android.net.Uri.parse(value).getLastPathSegment());
    }

    private static boolean bookName(String name) {
        if (name == null) return false;
        String lower = name.toLowerCase(Locale.ROOT);
        for (String extension : new String[]{".epub", ".pdf", ".mobi", ".azw", ".azw3", ".m4b", ".torrent"}) {
            if (lower.endsWith(extension)) return true;
        }
        return false;
    }

    static String filename(String url, String disposition, String mime) throws IOException {
        // Generic application/octet-stream responses must retain a server's
        // .epub/.pdf filename rather than URLUtil replacing it with .bin.
        String name = URLUtil.guessFileName(url, disposition, null);
        // Servers choose the name, never the destination directory.
        name = name.replaceAll("[\\\\/:*?\"<>|\\p{Cntrl}]", "_").replaceAll("^\\.+", "");
        if (name.length() > 180) name = name.substring(name.length() - 180);
        if (!bookName(name)) {
            String type = mime == null ? "" : mime.split(";", 2)[0].trim().toLowerCase(Locale.ROOT);
            String extension;
            switch (type) {
                case "application/epub+zip": extension = ".epub"; break;
                case "application/pdf": extension = ".pdf"; break;
                case "application/x-mobipocket-ebook": extension = ".mobi"; break;
                case "application/vnd.amazon.ebook": extension = ".azw"; break;
                case "application/x-mobi8-ebook": extension = ".azw3"; break;
                case "audio/x-m4b": case "audio/mp4": extension = ".m4b"; break;
                case "application/x-bittorrent": extension = ".torrent"; break;
                default: throw new IOException("This download is not a supported book file");
            }
            name = (name.isEmpty() ? "book" : name) + extension;
        }
        return name;
    }

    static File save(String address, String userAgent, String referer, String disposition,
                     String mime, File directory, Progress progress) throws IOException {
        return save(address, userAgent, referer, disposition, mime, directory, progress, () -> {});
    }

    static File save(String address, String userAgent, String referer, String disposition,
                     String mime, File directory, Progress progress, Runnable waiting) throws IOException {
        return save(address, userAgent, referer, disposition, mime, directory, progress, waiting, new Cancellation());
    }

    static File save(String address, String userAgent, String referer, String disposition,
                     String mime, File directory, Progress progress, Runnable waiting, Cancellation cancellation) throws IOException {
        for (;;) {
            cancellation.check();
            if (!transfersAllowed.getAsBoolean()) waiting.run();
            while (!transfersAllowed.getAsBoolean()) {
                cancellation.check();
                try { Thread.sleep(250); }
                catch (InterruptedException interrupted) { Thread.currentThread().interrupt(); throw new IOException("Download interrupted", interrupted); }
            }
            try { return saveAllowed(address, userAgent, referer, disposition, mime, directory, progress, cancellation); }
            catch (Deferred deferred) { waiting.run(); }
            catch (IOException error) { cancellation.check(); if (transfersAllowed.getAsBoolean()) throw error; waiting.run(); }
        }
    }

    private static File saveAllowed(String address, String userAgent, String referer, String disposition,
                     String mime, File directory, Progress progress, Cancellation cancellation) throws IOException {
        if (!webAddress(address)) throw new IOException("This download needs an HTTP or HTTPS link");
        HttpURLConnection connection = null;
        File partial = null;
        try {
            URL url = new URL(address);
            CookieManager cookies = CookieManager.getInstance();
            for (int redirects = 0; ; redirects++) {
                cancellation.check();
                if (!transfersAllowed.getAsBoolean()) throw new Deferred();
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
                    if (redirects >= 10) throw new IOException("The download redirected too many times");
                    String location = connection.getHeaderField("Location");
                    if (location == null) throw new IOException("The download redirect has no address");
                    URL next = new URL(url, location);
                    if (!webAddress(next.toString())) throw new IOException("The download redirected to an unsupported address");
                    connection.disconnect();
                    url = next;
                    continue;
                }
                if (status != HttpURLConnection.HTTP_OK) throw new IOException("The download returned HTTP " + status);
                break;
            }
            String responseDisposition = connection.getHeaderField("Content-Disposition");
            String responseMime = connection.getContentType();
            if (responseMime != null && (responseMime.toLowerCase(Locale.ROOT).startsWith("text/html")
                    || responseMime.toLowerCase(Locale.ROOT).startsWith("application/xhtml+xml"))) {
                throw new IOException("The store returned a page instead of the book. Sign in and try again.");
            }
            String name = filename(url.toString(), responseDisposition == null ? disposition : responseDisposition,
                    responseMime == null ? mime : responseMime);
            File root = directory.getCanonicalFile();
            if (!root.isDirectory()) throw new IOException("The library download folder is unavailable");
            cancellation.check();
            partial = File.createTempFile("download-", ".part", root);
            long total = connection.getContentLengthLong();
            long count = 0;
            try (InputStream input = connection.getInputStream(); FileOutputStream output = new FileOutputStream(partial)) {
                byte[] buffer = new byte[65536];
                int length;
                while ((length = input.read(buffer)) != -1) {
                    cancellation.check();
                    if (!transfersAllowed.getAsBoolean()) throw new Deferred();
                    output.write(buffer, 0, length);
                    count += length;
                    progress.update(count, total);
                }
                output.getFD().sync();
            }
            if (count == 0 || (total >= 0 && count != total)) throw new IOException("The book download was incomplete");
            cancellation.check();
            File completed = new File(root, name);
            if (completed.exists()) completed = new File(root, UUID.randomUUID() + "-" + name);
            if (!partial.renameTo(completed)) throw new IOException("Could not finish saving the downloaded book");
            partial = null;
            return completed;
        } finally {
            if (connection != null) connection.disconnect();
            cancellation.disconnected(connection);
            if (partial != null) partial.delete();
        }
    }
}
