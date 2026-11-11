package se.bokheim.reader.gpui;

import android.system.ErrnoException;
import android.system.OsConstants;
import org.json.JSONObject;

/** Stable failure codes shared with Rust; exception messages remain diagnostics. */
final class FailureDetails {
    static String classify(Throwable error) {
        String fallback = null;
        for (Throwable cause = error; cause != null; cause = cause.getCause()) {
            if (cause instanceof ErrnoException) {
                int code = ((ErrnoException) cause).errno;
                if (code == OsConstants.ENOSPC || code == OsConstants.EDQUOT) return "StorageFull";
                if (code == OsConstants.EROFS) return "ReadOnlyStorage";
                if (code == OsConstants.EACCES || code == OsConstants.EPERM) return "PermissionDenied";
                if (code == OsConstants.ETIMEDOUT) return "TimedOut";
            }
            if (cause instanceof SecurityException || cause instanceof java.nio.file.AccessDeniedException) return "PermissionDenied";
            if (cause instanceof java.net.SocketTimeoutException || cause instanceof java.util.concurrent.TimeoutException) return "TimedOut";
            if (cause instanceof java.net.UnknownHostException || cause instanceof java.net.ConnectException) return "Offline";
            // FileNotFoundException also wraps failed opens caused by ENOSPC or EACCES.
            if (cause instanceof java.io.FileNotFoundException) fallback = "NotFound";
        }
        return fallback;
    }

    static JSONObject put(JSONObject value, String field, Throwable error) throws org.json.JSONException {
        return value.put(field, error.toString()).put("failure", classify(error));
    }

    static String encode(Throwable error) {
        try { return put(new JSONObject(), "message", error).toString(); }
        catch (org.json.JSONException ignored) { return error.toString(); }
    }
}
