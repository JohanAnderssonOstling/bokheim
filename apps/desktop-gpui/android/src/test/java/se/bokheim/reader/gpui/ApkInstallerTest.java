package se.bokheim.reader.gpui;

import static org.junit.Assert.*;
import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.annotation.Config;

@RunWith(RobolectricTestRunner.class)
@Config(sdk = 30)
public class ApkInstallerTest {
    private static final String HASH = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";


    @Test public void rejectsSiblingPathsAndSymlinksOutsideStaging() throws Exception {
        File root = Files.createTempDirectory("apk-stage").toFile();
        File staging = new File(root, "staging");
        assertTrue(staging.mkdir());
        File external = new File(root, "application.bin");
        Files.write(external.toPath(), new byte[] {97, 98, 99});
        assertThrows(IOException.class, () -> ApkInstaller.verifyBytes(staging, external, 3, HASH));
        File link = new File(staging, "application.bin");
        Files.createSymbolicLink(link.toPath(), external.toPath());
        assertThrows(IOException.class, () -> ApkInstaller.verifyBytes(staging, link, 3, HASH));
    }


}
