package se.bokheim.reader.gpui;

import static org.junit.Assert.*;

import android.content.Context;
import android.content.ContextWrapper;
import android.content.res.AssetManager;
import java.io.File;
import java.nio.file.Files;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.RuntimeEnvironment;
import org.robolectric.annotation.Config;

@RunWith(RobolectricTestRunner.class)
@Config(sdk = 30)
public class SpeechAssetsTest {
    @Test public void georgeIsNotAvailableOnAndroid() throws Exception {
        Context context = RuntimeEnvironment.getApplication();
        assertThrows(IllegalArgumentException.class, () -> SpeechAssets.prepare(context, "george"));
        assertFalse(new File(context.getNoBackupFilesDir(), "tts/george").exists());
    }

    @Test public void interruptedVoiceCopyIsReplacedAndCompletedCopyIsReused() throws Exception {
        Context context = RuntimeEnvironment.getApplication();
        File destination = SpeechAssets.directory(context);
        File staging = new File(destination.getParentFile(), "alba-staging");
        assertTrue(staging.mkdirs() || staging.isDirectory());
        Files.write(new File(staging, "interrupted").toPath(), new byte[] {1});

        assertEquals(destination, SpeechAssets.prepare(context));
        assertFalse(staging.exists());
        File model = new File(destination, "en_GB-alba-medium.onnx");
        assertTrue(model.length() > 0);
        File tokens = new File(destination, "tokens.txt");
        assertTrue(tokens.isFile());
        assertTrue(new File(destination, "espeak-ng-data/en_dict").isFile());
        long modified = model.lastModified();
        long size = model.length();
        assertEquals(destination, SpeechAssets.prepare(context));
        assertEquals(modified, model.lastModified());

        // Upgrade the previous support-only layout even when its marker matches.
        Files.delete(model.toPath());
        assertEquals(destination, SpeechAssets.prepare(context));
        assertEquals(size, model.length());

        // A newer APK's marker must replace the old voice rather than trust it.
        Files.write(new File(destination, "voice.sha256").toPath(), "old-voice".getBytes(java.nio.charset.StandardCharsets.UTF_8));
        SpeechAssets.prepare(context);
        assertNotEquals("old-voice", new String(Files.readAllBytes(new File(destination, "voice.sha256").toPath()), java.nio.charset.StandardCharsets.UTF_8).trim());
    }

    @Test public void startupExtractionRunsOffTheCallerAndSpeechWaitsForIt() throws Exception {
        Context context = RuntimeEnvironment.getApplication();
        CountDownLatch copying = new CountDownLatch(1);
        CountDownLatch release = new CountDownLatch(1);
        AtomicReference<Thread> copyingThread = new AtomicReference<>();
        Context delayed = new ContextWrapper(context) {
            @Override public Context getApplicationContext() { return this; }
            @Override public AssetManager getAssets() {
                copyingThread.set(Thread.currentThread());
                copying.countDown();
                try {
                    if (!release.await(10, TimeUnit.SECONDS)) throw new IllegalStateException("Extraction was not released");
                } catch (InterruptedException error) {
                    Thread.currentThread().interrupt();
                    throw new IllegalStateException(error);
                }
                return super.getAssets();
            }
        };
        Thread background = SpeechAssets.prepareInBackground(delayed);
        AtomicReference<File> ready = new AtomicReference<>();
        AtomicReference<Throwable> failure = new AtomicReference<>();
        CountDownLatch requested = new CountDownLatch(1);
        CountDownLatch completed = new CountDownLatch(1);
        Thread speech = new Thread(() -> {
            requested.countDown();
            try {
                ready.set(SpeechAssets.prepare(context));
            } catch (Throwable error) {
                failure.set(error);
            } finally {
                completed.countDown();
            }
        }, "test-speech-request");
        try {
            assertTrue(copying.await(5, TimeUnit.SECONDS));
            assertSame(background, copyingThread.get());
            assertNotSame(Thread.currentThread(), copyingThread.get());
            // Activity recreation reuses the worker without waiting for its copy.
            assertSame(background, SpeechAssets.prepareInBackground(context));
            speech.start();
            assertTrue(requested.await(5, TimeUnit.SECONDS));
            assertFalse(completed.await(100, TimeUnit.MILLISECONDS));
        } finally {
            release.countDown();
        }
        background.join(10000);
        speech.join(10000);
        assertFalse(background.isAlive());
        assertFalse(speech.isAlive());
        assertNull(failure.get());
        assertEquals(SpeechAssets.directory(context), ready.get());
        assertTrue(new File(ready.get(), "en_GB-alba-medium.onnx").length() > 0);
    }
}
