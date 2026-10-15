package se.bokheim.reader.gpui;

import static org.junit.Assert.*;
import android.content.Context;
import java.io.File;
import java.nio.file.Files;
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
}
