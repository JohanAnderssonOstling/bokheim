package se.bokheim.reader.gpui;

import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.util.Base64;
import org.json.JSONArray;
import org.json.JSONObject;

/** Artwork comes only from the in-process library bridge, never notification intents. */
final class MediaArtwork {
    static byte[] bytes(JSONObject state) {
        String encoded = state.optString("coverBase64", null);
        if (encoded != null) {
            if (encoded.isEmpty() || encoded.length() > ((256 * 1024 + 2) / 3) * 4) return null;
            try {
                byte[] bytes = Base64.decode(encoded, Base64.NO_WRAP);
                return bytes.length <= 256 * 1024 ? bytes : null;
            } catch (IllegalArgumentException error) { return null; }
        }
        // Speech notifications still use the byte-array representation.
        JSONArray data = state.optJSONArray("cover");
        if (data == null || data.length() == 0 || data.length() > 256 * 1024) return null;
        byte[] bytes = new byte[data.length()];
        for (int i = 0; i < bytes.length; i++) bytes[i] = (byte) data.optInt(i);
        return bytes;
    }

    static Bitmap bitmap(byte[] bytes) {
        if (bytes == null) return null;
        BitmapFactory.Options bounds = new BitmapFactory.Options();
        bounds.inJustDecodeBounds = true;
        BitmapFactory.decodeByteArray(bytes, 0, bytes.length, bounds);
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null;
        BitmapFactory.Options options = new BitmapFactory.Options();
        options.inSampleSize = 1;
        while (bounds.outWidth / options.inSampleSize > 600 || bounds.outHeight / options.inSampleSize > 600) options.inSampleSize *= 2;
        return BitmapFactory.decodeByteArray(bytes, 0, bytes.length, options);
    }

    static String credit(String author, String chapter) {
        return author.isEmpty() ? chapter : chapter.isEmpty() ? author : author + " · " + chapter;
    }

    private MediaArtwork() {}
}
