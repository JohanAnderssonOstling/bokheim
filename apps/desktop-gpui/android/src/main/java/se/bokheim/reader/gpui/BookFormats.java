package se.bokheim.reader.gpui;

import java.util.LinkedHashSet;
import java.util.Locale;
import java.util.Set;
import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/** Platform adapter for the registry bundled from shared/book-model/formats.json. */
final class BookFormats {
    private static final JSONArray FORMATS;
    static {
        try { FORMATS = new JSONArray(BuildConfig.BOOK_FORMATS_JSON); }
        catch (JSONException invalid) { throw new ExceptionInInitializerError(invalid); }
    }

    static boolean bookName(String name) {
        if (name == null) return false;
        String lower = name.toLowerCase(Locale.ROOT);
        try {
            for (int index = 0; index < FORMATS.length(); index++) {
                JSONObject format = FORMATS.getJSONObject(index);
                if (!format.getBoolean("browser_download")) continue;
                JSONArray files = format.getJSONArray("files");
                for (int file = 0; file < files.length(); file++) {
                    JSONArray extensions = files.getJSONObject(file).getJSONArray("extensions");
                    for (int extension = 0; extension < extensions.length(); extension++) {
                        if (lower.endsWith("." + extensions.getString(extension))) return true;
                    }
                }
            }
            return false;
        } catch (JSONException invalid) { throw new IllegalStateException("Invalid bundled book formats", invalid); }
    }

    static String extensionForMime(String mime) {
        String type = mime == null ? "" : mime.split(";", 2)[0].trim().toLowerCase(Locale.ROOT);
        try {
            for (int index = 0; index < FORMATS.length(); index++) {
                JSONObject format = FORMATS.getJSONObject(index);
                if (!format.getBoolean("browser_download")) continue;
                JSONArray files = format.getJSONArray("files");
                for (int file = 0; file < files.length(); file++) {
                    JSONObject entry = files.getJSONObject(file);
                    JSONArray media = entry.getJSONArray("mime_types");
                    for (int item = 0; item < media.length(); item++) {
                        if (type.equals(media.getJSONObject(item).getString("value"))) {
                            return "." + entry.getJSONArray("extensions").getString(0);
                        }
                    }
                }
            }
            return null;
        } catch (JSONException invalid) { throw new IllegalStateException("Invalid bundled book formats", invalid); }
    }

    static String[] mimeTypesFor(String[] extensions) {
        Set<String> result = new LinkedHashSet<>();
        try {
            for (String extension : extensions) {
                for (int index = 0; index < FORMATS.length(); index++) {
                    JSONArray files = FORMATS.getJSONObject(index).getJSONArray("files");
                    for (int file = 0; file < files.length(); file++) {
                        JSONObject entry = files.getJSONObject(file);
                        JSONArray picker = entry.optJSONArray("picker_extensions");
                        if (picker == null) picker = entry.getJSONArray("extensions");
                        for (int candidate = 0; candidate < picker.length(); candidate++) {
                            if (!extension.equalsIgnoreCase(picker.getString(candidate))) continue;
                            JSONArray types = entry.optJSONArray("picker_mime_types");
                            if (types != null) {
                                for (int type = 0; type < types.length(); type++) result.add(types.getString(type));
                            } else {
                                types = entry.getJSONArray("mime_types");
                                for (int type = 0; type < types.length(); type++) result.add(types.getJSONObject(type).getString("value"));
                            }
                        }
                    }
                }
            }
            return result.toArray(new String[0]);
        } catch (JSONException invalid) { throw new IllegalStateException("Invalid bundled book formats", invalid); }
    }

    private BookFormats() {}
}
