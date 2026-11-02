package se.bokheim.reader.gpui;

import static org.junit.Assert.*;
import android.content.Context;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InterruptedIOException;
import java.nio.channels.FileLock;
import java.nio.file.Files;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.RuntimeEnvironment;
import org.robolectric.annotation.Config;

@RunWith(RobolectricTestRunner.class)
@Config(sdk = 30)
public class TaxonomyAssetsTest {
    @Test public void completedExtractionIsReusedAndInterruptedExtractionIsReplaced() throws Exception {
        Context context = RuntimeEnvironment.getApplication();
        File destination = TaxonomyAssets.prepare(context);
        byte[] original = Files.readAllBytes(destination.toPath());
        assertTrue(original.length > 16);
        assertEquals("SQLite format 3", new String(original, 0, 15, java.nio.charset.StandardCharsets.US_ASCII));
        assertTrue(destination.toPath().startsWith(context.getNoBackupFilesDir().toPath()));
        long modified = destination.lastModified();
        assertEquals(destination, TaxonomyAssets.prepare(context));
        assertEquals(modified, destination.lastModified());
        Files.delete(destination.toPath());
        File staging = new File(destination.getParentFile(), "taxonomy.staging");
        Files.write(staging.toPath(), new byte[] {1});
        assertEquals(destination, TaxonomyAssets.prepare(context));
        assertArrayEquals(original, Files.readAllBytes(destination.toPath()));
        assertFalse(staging.exists());
    }

    @Test public void contendedInstallationLockTimesOut() throws Exception {
        File file = File.createTempFile("taxonomy-install", ".lock");
        ExecutorService startup = Executors.newSingleThreadExecutor();
        try (FileOutputStream holder = new FileOutputStream(file);
             FileLock held = holder.getChannel().lock();
             FileOutputStream contender = new FileOutputStream(file, true)) {
            startup.submit(() -> {
                IOException error = assertThrows(IOException.class,
                    () -> TaxonomyAssets.acquireInstallationLock(contender.getChannel(), 30));
                assertEquals("Timed out waiting for taxonomy extraction lock", error.getMessage());
            }).get(2, TimeUnit.SECONDS);
        } finally {
            startup.shutdownNow();
            Files.deleteIfExists(file.toPath());
        }
    }

    @Test public void interruptedWaitLeavesExtractionAvailableForRetry() throws Exception {
        Context context = RuntimeEnvironment.getApplication();
        File destination = TaxonomyAssets.prepare(context);
        Files.delete(destination.toPath());
        File lockFile = new File(destination.getParentFile().getParentFile(), "install.lock");
        ExecutorService startup = Executors.newSingleThreadExecutor();
        try (FileOutputStream holder = new FileOutputStream(lockFile, true);
             FileLock held = holder.getChannel().lock()) {
            startup.submit(() -> {
                Thread.currentThread().interrupt();
                assertThrows(InterruptedIOException.class, () -> TaxonomyAssets.prepare(context));
                assertTrue(Thread.currentThread().isInterrupted());
            }).get(2, TimeUnit.SECONDS);
            assertFalse(destination.exists());
        } finally { startup.shutdownNow(); }
        assertEquals(destination, TaxonomyAssets.prepare(context));
        assertTrue(destination.length() > 16);
    }
}
