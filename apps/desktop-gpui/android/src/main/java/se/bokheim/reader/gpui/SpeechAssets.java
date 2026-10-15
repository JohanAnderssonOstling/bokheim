package se.bokheim.reader.gpui;

import android.content.Context;
import android.content.res.AssetManager;
import android.os.Process;
import android.util.Log;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;

/** Extract bundled Alba in the background; inference remains lazy. */
final class SpeechAssets {
    private static final String MARKER = "voice.sha256";
    private static final Object INSTALL_LOCK = new Object();
    private static Thread preparation;

    static synchronized Thread prepareInBackground(Context context) {
        if (preparation == null) {
            Context application = context.getApplicationContext();
            preparation = new Thread(() -> {
                Process.setThreadPriority(Process.THREAD_PRIORITY_BACKGROUND);
                try {
                    prepare(application, "alba");
                } catch (IOException error) {
                    // A later speech request can retry an interrupted or failed copy.
                    Log.e("SpeechAssets", "Could not extract the bundled speech voice", error);
                }
            }, "bokheim-speech-assets");
            preparation.start();
        }
        return preparation;
    }

    static File directory(Context context) {
        return directory(context, "alba");
    }

    static File directory(Context context, String voice) {
        if (!voice.equals("alba")) throw new IllegalArgumentException("Unknown speech voice");
        return new File(context.getNoBackupFilesDir(), "tts/" + voice);
    }

    static File prepare(Context context) throws IOException {
        return prepare(context, "alba");
    }

    static File prepare(Context context, String voice) throws IOException {
        // Speech waits here if the startup worker is still copying. Keep this
        // lock separate from prepareInBackground so Activity recreation cannot
        // make the UI thread wait for installation.
        synchronized (INSTALL_LOCK) {
            return install(context, voice);
        }
    }

    private static File install(Context context, String voice) throws IOException {
        File destination = directory(context, voice);
        String asset = "tts/" + voice;
        String model = "en_GB-alba-medium.onnx";
        AssetManager assets = context.getAssets();
        String expected;
        try (InputStream input = assets.open(asset + "/" + MARKER)) {
            expected = readText(input);
        }
        File marker = new File(destination, MARKER);
        if (marker.isFile() && new File(destination, model).isFile()
                && new File(destination, "tokens.txt").isFile()
                && new File(destination, "espeak-ng-data/en_dict").isFile()) {
            try (InputStream input = new FileInputStream(marker)) {
                if (expected.equals(readText(input))) return destination;
            }
        }
        File staging = new File(destination.getParentFile(), voice + "-staging");
        delete(staging);
        copy(assets, asset, staging, asset);
        // Only publish a complete copy. Interrupted copies remain in staging and
        // are replaced on the next launch; they never acquire the ready marker.
        delete(destination);
        if (!staging.renameTo(destination)) throw new IOException("Cannot install speech voice");
        return destination;
    }

    private static String readText(InputStream input) throws IOException {
        java.io.ByteArrayOutputStream output = new java.io.ByteArrayOutputStream();
        byte[] buffer = new byte[1024];
        int count;
        while ((count = input.read(buffer)) != -1) output.write(buffer, 0, count);
        return output.toString(StandardCharsets.UTF_8.name()).trim();
    }

    private static void copy(AssetManager assets, String path, File destination, String root) throws IOException {
        String[] children = assets.list(path);
        if (children != null && children.length > 0) {
            if (!destination.isDirectory() && !destination.mkdirs()) throw new IOException("Cannot create speech directory");
            for (String child : children) {
                if (!child.equals(MARKER)) {
                    copy(assets, path + "/" + child, new File(destination, child), root);
                }
            }
            // The root marker is written after every model/data file.
            if (path.equals(root)) copy(assets, path + "/" + MARKER, new File(destination, MARKER), root);
        } else {
            File parent = destination.getParentFile();
            if (!parent.isDirectory() && !parent.mkdirs()) throw new IOException("Cannot create speech directory");
            try (InputStream input = assets.open(path); FileOutputStream output = new FileOutputStream(destination)) {
                byte[] buffer = new byte[64 * 1024];
                int count;
                while ((count = input.read(buffer)) != -1) output.write(buffer, 0, count);
                output.getFD().sync();
            }
        }
    }

    private static void delete(File file) throws IOException {
        File[] children = file.listFiles();
        if (children != null) for (File child : children) delete(child);
        if (file.exists() && !file.delete()) throw new IOException("Cannot replace speech assets");
    }
}
