package se.bokheim.reader.gpui;

import android.content.Context;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.nio.channels.FileChannel;
import java.nio.channels.FileLock;

/** Extract once per resource revision, before native SQLite connections open.
 * Revision directories keep existing immutable readers valid across app updates.
 */
final class TaxonomyAssets {
    static synchronized File prepare(Context context) throws IOException {
        String digest;
        try (InputStream input = context.getAssets().open("taxonomy.digest")) {
            byte[] bytes = new byte[65];
            int length = 0, count;
            while (length < bytes.length && (count = input.read(bytes, length, bytes.length - length)) != -1) length += count;
            digest = new String(bytes, 0, length, StandardCharsets.US_ASCII).trim();
        }
        if (!digest.matches("[0-9a-f]{64}")) throw new IOException("Invalid taxonomy digest");
        File root = new File(context.getNoBackupFilesDir(), "bundled-taxonomy");
        File directory = new File(root, digest);
        File destination = new File(directory, "taxonomy.sqlite3");
        if (destination.isFile() && destination.length() > 0) return destination;
        Files.createDirectories(root.toPath());
        // Startup and the isolated migration service can extract concurrently.
        try (FileOutputStream lockFile = new FileOutputStream(new File(root, "install.lock"), true);
             FileChannel channel = lockFile.getChannel(); FileLock lock = channel.lock()) {
            Files.createDirectories(directory.toPath());
            if (destination.isFile() && destination.length() > 0) return destination;
            File staging = new File(directory, "taxonomy.staging");
            try {
                try (InputStream input = context.getAssets().open("taxonomy.sqlite3");
                     FileOutputStream output = new FileOutputStream(staging)) {
                    byte[] buffer = new byte[65536];
                    int count;
                    while ((count = input.read(buffer)) != -1) output.write(buffer, 0, count);
                    output.getFD().sync();
                }
                if (staging.length() == 0) throw new IOException("Empty bundled taxonomy");
                Files.move(staging.toPath(), destination.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
                return destination;
            } finally {
                Files.deleteIfExists(staging.toPath());
            }
        }
    }
}
