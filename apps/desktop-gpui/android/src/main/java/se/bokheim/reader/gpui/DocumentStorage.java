package se.bokheim.reader.gpui;

import android.content.ContentResolver;
import android.content.Context;
import android.content.Intent;
import android.content.UriPermission;
import android.database.Cursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.DocumentsContract;
import org.json.JSONArray;
import org.json.JSONObject;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.nio.charset.StandardCharsets;

/** Persistent user-selected roots. Never treats a URI grant as a raw-path grant. */
final class DocumentStorage {
    private static final String PREFIX = "/bokheim-document-trees/";
    private static final String PREFERENCES = "library_location";
    private static final String ROOT = "root";
    private static final String[] COLUMNS = {DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME, DocumentsContract.Document.COLUMN_MIME_TYPE, DocumentsContract.Document.COLUMN_SIZE, DocumentsContract.Document.COLUMN_LAST_MODIFIED, DocumentsContract.Document.COLUMN_FLAGS};
    private DocumentStorage() {}

    static String defaultRoot(Context context) {
        return context.getSharedPreferences(PREFERENCES, 0).getString(ROOT, null);
    }
    static void saveDefault(Context context, String path) throws Exception {
        if (!stat(context.getContentResolver(), resolve(context, path)).getBoolean("directory"))
            throw new IOException("Library location must be a folder");
        if (!context.getSharedPreferences(PREFERENCES, 0).edit().putString(ROOT, path).commit())
            throw new IOException("Could not save library location");
    }
    static boolean hasDefault(Context context) {
        return defaultRoot(context) != null;
    }
    static String retain(Context context, Intent result) throws Exception {
        Uri tree = result.getData();
        if (tree == null) throw new IOException("No folder was selected");
        int flags = result.getFlags() & (Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
        if ((flags & Intent.FLAG_GRANT_READ_URI_PERMISSION) == 0 || (flags & Intent.FLAG_GRANT_WRITE_URI_PERMISSION) == 0)
            throw new IOException("Choose a folder that allows reading and writing");
        context.getContentResolver().takePersistableUriPermission(tree, flags);
        StringBuilder token = new StringBuilder();
        for (byte value : tree.toString().getBytes(StandardCharsets.UTF_8)) token.append(String.format(java.util.Locale.ROOT, "%02x", value & 255));
        String path = PREFIX + token;
        JSONObject root = stat(context.getContentResolver(), resolve(context, path));
        if (!root.getBoolean("directory") || (root.getLong("flags") & DocumentsContract.Document.FLAG_DIR_SUPPORTS_CREATE) == 0)
            throw new IOException("Choose a writable library folder");
        return path;
    }
    private static Uri decodedTree(String path) throws Exception {
        if (!path.startsWith(PREFIX)) throw new IOException("Not a document-tree path");
        String token = path.substring(PREFIX.length()).split("/", 2)[0];
        if (token.isEmpty() || token.length() % 2 != 0) throw new IOException("Invalid document-tree path");
        byte[] bytes = new byte[token.length() / 2];
        for (int i = 0; i < bytes.length; i++) bytes[i] = (byte) Integer.parseInt(token.substring(i * 2, i * 2 + 2), 16);
        return Uri.parse(new String(bytes, StandardCharsets.UTF_8));
    }
    private static Uri tree(Context context, String path) throws Exception {
        Uri tree = decodedTree(path);
        for (UriPermission permission : context.getContentResolver().getPersistedUriPermissions()) {
            if (permission.getUri().equals(tree) && permission.isReadPermission() && permission.isWritePermission()) return tree;
        }
        throw new SecurityException("Access to the library folder has expired. Select the folder again.");
    }
    private static String[] parts(String path) throws IOException {
        String[] split = path.substring(PREFIX.length()).split("/", -1);
        for (String part : split) if (part.isEmpty() || part.equals(".") || part.equals("..")) throw new IOException("Invalid document path");
        return split;
    }
    private static Uri child(ContentResolver resolver, Uri tree, Uri parent, String name) throws Exception {
        Uri children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, DocumentsContract.getDocumentId(parent));
        try (Cursor cursor = resolver.query(children, new String[]{DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME}, null, null, null)) {
            if (cursor == null) throw new IOException("Could not list library folder");
            Uri found = null;
            while (cursor.moveToNext()) if (name.equals(cursor.getString(1))) {
                if (found != null) throw new IOException("Folder contains duplicate names: " + name);
                found = DocumentsContract.buildDocumentUriUsingTree(tree, cursor.getString(0));
            }
            if (found == null) throw new FileNotFoundException(name);
            return found;
        }
    }
    private static Uri resolve(Context context, String path) throws Exception {
        Uri tree = tree(context, path);
        String[] components = parts(path);
        Uri document = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree));
        // Android's local provider defines document IDs as volume:relative/path.
        // Resolve directly through that provider, without rescanning every sibling
        // for each book. Access is still checked against the retained tree grant.
        if ("com.android.externalstorage.documents".equals(tree.getAuthority())) {
            String id = DocumentsContract.getTreeDocumentId(tree);
            for (int i = 1; i < components.length; i++) id += (id.endsWith(":") ? "" : "/") + components[i];
            document = DocumentsContract.buildDocumentUriUsingTree(tree, id);
            stat(context.getContentResolver(), document);
            return document;
        }
        for (int i = 1; i < components.length; i++) document = child(context.getContentResolver(), tree, document, components[i]);
        return document;
    }
    private static String parent(String path) throws IOException {
        if (parts(path).length < 2) throw new IOException("Cannot replace or delete a selected storage root");
        return path.substring(0, path.lastIndexOf('/'));
    }
    private static String name(String path) throws IOException { parts(path); return path.substring(path.lastIndexOf('/') + 1); }
    private static JSONObject stat(ContentResolver resolver, Uri document) throws Exception {

        try (Cursor cursor = resolver.query(document, COLUMNS, null, null, null)) {
            if (cursor == null || !cursor.moveToFirst()) throw new FileNotFoundException("Library file is unavailable");
            return metadata(cursor);
        }
    }
    private static JSONObject metadata(Cursor cursor) throws Exception {
        return new JSONObject().put("id", cursor.getString(0)).put("name", cursor.getString(1))
                .put("directory", DocumentsContract.Document.MIME_TYPE_DIR.equals(cursor.getString(2)))
                .put("size", cursor.isNull(3) ? 0 : cursor.getLong(3)).put("modified", cursor.isNull(4) ? 0 : cursor.getLong(4)).put("flags", cursor.getLong(5));
    }
    private static Uri create(Context context, String path, String mime) throws Exception {
        ContentResolver resolver = context.getContentResolver();
        try { resolve(context, path); throw new ExistsException(); } catch (FileNotFoundException absent) { }
        Uri created = DocumentsContract.createDocument(resolver, resolve(context, parent(path)), mime, name(path));
        if (created == null) throw new IOException("Could not create library file");
        if (!name(path).equals(stat(resolver, created).getString("name"))) {
            DocumentsContract.deleteDocument(resolver, created);
            throw new ExistsException();
        }
        return created;
    }
    private static final class ExistsException extends IOException { ExistsException() { super("A file or folder with this name already exists"); } }
    private static final class CrossDeviceException extends IOException { CrossDeviceException() { super("Source and destination use different storage roots"); } }
    static synchronized String command(Context context, String encoded) {
        JSONObject response = new JSONObject();
        try {
            JSONObject request = new JSONObject(encoded);
            recoverReplacements(context);
            response.put("value", perform(context, request));
        } catch (Exception error) {
            try { response.put("error", error.getMessage() == null ? error.toString() : error.getMessage());
                response.put("kind", error instanceof FileNotFoundException ? "missing" : error instanceof ExistsException ? "exists" : error instanceof SecurityException ? "permission" : error instanceof CrossDeviceException ? "crosses" : error instanceof UnsupportedOperationException ? "unsupported" : "other");
            } catch (org.json.JSONException impossible) { throw new IllegalStateException(impossible); }
        }
        return response.toString();
    }
    private static android.content.SharedPreferences replacements(Context context) {
        return context.getSharedPreferences("library_replacements", 0);
    }
    private static void forgetReplacement(Context context, String id) throws IOException {
        if (!replacements(context).edit().remove(id).commit()) throw new IOException("Could not finish library file replacement");
    }
    /** A provider cannot atomically replace an existing name. Keep a private
     * write-ahead journal so interruption can restore the old complete file. */
    private static void recoverReplacements(Context context) {
        for (java.util.Map.Entry<String, ?> item : replacements(context).getAll().entrySet()) {
            try {
                JSONObject record = new JSONObject((String) item.getValue());
                String target = record.getString("target");
                String backupPath = parent(target) + "/" + record.getString("backup");
                Uri backup;
                try { backup = resolve(context, backupPath); }
                catch (FileNotFoundException absent) { forgetReplacement(context, item.getKey()); continue; }
                boolean present;
                try { resolve(context, target); present = true; }
                catch (FileNotFoundException absent) { present = false; }
                if (present) {
                    if (!DocumentsContract.deleteDocument(context.getContentResolver(), backup)) continue;
                } else {
                    Uri restored = DocumentsContract.renameDocument(context.getContentResolver(), backup, name(target));
                    if (restored == null || !name(target).equals(stat(context.getContentResolver(), restored).getString("name"))) continue;
                }
                forgetReplacement(context, item.getKey());
            } catch (Exception unavailable) {
                // A detached volume or revoked grant must not block other roots.
                // Retain this exact backup's journal until access returns.
            }
        }
    }
    private static Object perform(Context context, JSONObject request) throws Exception {
        ContentResolver resolver = context.getContentResolver();
        String path = request.getString("path");
        String op = request.getString("op");
        if (op.equals("setDefault")) { saveDefault(context, path); return true; }
        if (op.equals("display")) {
            Uri tree = decodedTree(path);
            String id = DocumentsContract.getTreeDocumentId(tree);
            String label = id.contains(":") ? id.substring(id.indexOf(':') + 1) : stat(resolver, resolve(context, path)).getString("name");
            if (label.isEmpty()) label = stat(resolver, resolve(context, PREFIX + parts(path)[0])).getString("name");
            String[] components = parts(path);
            for (int i = 1; i < components.length; i++) label += "/" + components[i];
            return label;
        }
        if (op.equals("mkdir")) { create(context, path, DocumentsContract.Document.MIME_TYPE_DIR); return true; }
        if (op.equals("open")) {
            Uri document;
            try { document = resolve(context, path); if (request.optBoolean("exclusive")) throw new ExistsException(); }
            catch (FileNotFoundException absent) {
                if (!request.optBoolean("create") && !request.optBoolean("exclusive")) throw absent;
                document = create(context, path, "application/octet-stream");
            }
            String mode = request.optBoolean("append") ? "wa" : request.optBoolean("write") ? (request.optBoolean("truncate") ? "rwt" : "rw") : "r";
            ParcelFileDescriptor descriptor = resolver.openFileDescriptor(document, mode);
            if (descriptor == null) throw new IOException("Could not open library file");
            return descriptor.detachFd();
        }
        Uri document = resolve(context, path);
        if (op.equals("show")) {
            Intent show = new Intent(Intent.ACTION_VIEW).setDataAndType(document, DocumentsContract.Document.MIME_TYPE_DIR);
            show.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_GRANT_READ_URI_PERMISSION);
            context.startActivity(show);
            return true;
        }
        if (op.equals("stat")) return stat(resolver, document);
        if (op.equals("list")) {
            JSONArray items = new JSONArray();
            Uri tree = tree(context, path);
            Uri children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, DocumentsContract.getDocumentId(document));
            try (Cursor cursor = resolver.query(children, COLUMNS, null, null, null)) {
                if (cursor == null) throw new IOException("Could not list library folder");
                while (cursor.moveToNext()) items.put(metadata(cursor));
            }
            return items;
        }
        if (op.equals("remove")) {
            parent(path); // A retained tree root must never be removed by cleanup.
            boolean directory = stat(resolver, document).getBoolean("directory");
            if (directory != request.getBoolean("directory")) throw new IOException("Unexpected library file type");
            if (directory && !request.getBoolean("recursive")) {
                Uri children = DocumentsContract.buildChildDocumentsUriUsingTree(tree(context, path), DocumentsContract.getDocumentId(document));
                try (Cursor cursor = resolver.query(children, new String[]{DocumentsContract.Document.COLUMN_DOCUMENT_ID}, null, null, null)) {
                    if (cursor == null || cursor.moveToFirst()) throw new IOException("Folder is not empty");
                }
            }
            if (!DocumentsContract.deleteDocument(resolver, document)) throw new IOException("Could not remove library file");
            return true;
        }
        if (op.equals("rename")) {
            String to = request.getString("to");
            parent(path); parent(to);
            if (!tree(context, path).equals(tree(context, to))) throw new CrossDeviceException();
            if (path.equals(to)) return true;
            // Preserve the old destination until the complete replacement is named.
            Uri backup = null;
            String replacement = null;
            try {
                Uri destination = resolve(context, to);
                if (request.optBoolean("exclusive")) throw new ExistsException();
                if (stat(resolver, destination).getBoolean("directory")) throw new ExistsException();
                replacement = java.util.UUID.randomUUID().toString();
                String backupName = ".bokheim-replaced-" + replacement;
                JSONObject record = new JSONObject().put("target", to).put("backup", backupName);
                if (!replacements(context).edit().putString(replacement, record.toString()).commit())
                    throw new IOException("Could not prepare library file replacement");
                backup = DocumentsContract.renameDocument(resolver, destination, backupName);
                if (backup == null) throw new IOException("Could not stage replacement");
            } catch (FileNotFoundException absent) { }
            boolean movedAcrossFolders = false;
            try {
                if (!parent(path).equals(parent(to))) {
                    Uri moved = DocumentsContract.moveDocument(resolver, document, resolve(context, parent(path)), resolve(context, parent(to)));
                    if (moved == null) throw new IOException("Could not move library file");
                    document = moved;
                    movedAcrossFolders = true;
                }
                if (!name(path).equals(name(to))) {
                    Uri renamed = DocumentsContract.renameDocument(resolver, document, name(to));
                    if (renamed == null) throw new IOException("Could not rename library file");
                    document = renamed;
                }
                if (!name(to).equals(stat(resolver, document).getString("name"))) throw new ExistsException();
            } catch (Exception failure) {
                if (movedAcrossFolders) {
                    try {
                        String currentName = stat(resolver, document).getString("name");
                        if (!name(path).equals(currentName)) {
                            Uri restoredName = DocumentsContract.renameDocument(resolver, document, name(path));
                            if (restoredName == null) throw new IOException("Could not restore the source filename");
                            document = restoredName;
                        }
                        if (DocumentsContract.moveDocument(resolver, document, resolve(context, parent(to)), resolve(context, parent(path))) == null)
                            throw new IOException("Could not restore the source folder");
                    } catch (Exception restoreFailure) { failure.addSuppressed(restoreFailure); }
                }
                if (backup != null) {
                    try {
                        Uri restored = DocumentsContract.renameDocument(resolver, backup, name(to));
                        if (restored != null && name(to).equals(stat(resolver, restored).getString("name"))) forgetReplacement(context, replacement);
                    } catch (Exception restoreFailure) { failure.addSuppressed(restoreFailure); }
                }
                throw failure;
            }
            if (backup != null && !DocumentsContract.deleteDocument(resolver, backup)) throw new IOException("Could not remove replaced library file");
            if (replacement != null) forgetReplacement(context, replacement);
            return true;
        }
        throw new UnsupportedOperationException("Unknown storage operation: " + op);
    }
}
