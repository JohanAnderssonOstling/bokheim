package se.bokheim.reader.gpui;

import static org.junit.Assert.*;

import android.app.Activity;
import android.os.Environment;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.Robolectric;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.android.controller.ActivityController;
import org.robolectric.annotation.Config;
import org.robolectric.annotation.Implementation;
import org.robolectric.annotation.Implements;
import org.robolectric.shadows.ShadowEnvironment;

@RunWith(RobolectricTestRunner.class)
@Config(sdk = 30, shadows = AndroidLibraryStorageTest.PermissionEnvironment.class)
public class AndroidLibraryStorageTest {
    @Implements(Environment.class)
    public static class PermissionEnvironment extends ShadowEnvironment {
        static boolean granted;

        @Implementation(minSdk = 30)
        protected static boolean isExternalStorageManager() {
            return granted;
        }
    }
    private ActivityController<Activity> controller;
    private Activity activity;

    @Before public void setUp() {
        PermissionEnvironment.granted = false;
        controller = Robolectric.buildActivity(Activity.class).setup();
        activity = controller.get();
        activity.getSharedPreferences("storage_permissions", Activity.MODE_PRIVATE).edit().clear().commit();
    }

    @After public void tearDown() {
        controller.pause().stop().destroy();
    }

    @Test public void localFolderMustStayInsideStorageVolume() throws Exception {
        java.io.File root = new java.io.File(activity.getCacheDir(), "volume");
        java.io.File child = new java.io.File(root, "Documents/Bokheim");
        assertTrue(child.mkdirs() || child.isDirectory());
        assertEquals(child.getCanonicalFile(), AndroidLibraryStorage.accessibleChild(root, "Documents/Bokheim"));
        assertNull(AndroidLibraryStorage.accessibleChild(root, "missing"));
        assertThrows(java.io.IOException.class, () -> AndroidLibraryStorage.accessibleChild(root, "../outside"));
    }

    @Test public void grantedLocalTreeUsesFilesystemOnlyForDirectDistribution() throws Exception {
        PermissionEnvironment.granted = true;
        java.io.File folder = new java.io.File(Environment.getExternalStorageDirectory(), "Documents/Bokheim");
        assertTrue(folder.mkdirs() || folder.isDirectory());
        android.net.Uri tree = android.provider.DocumentsContract.buildTreeDocumentUri(
                "com.android.externalstorage.documents", "primary:Documents/Bokheim");
        assertEquals(BuildConfig.PLAY_STORE ? null : folder.getCanonicalFile(), AndroidLibraryStorage.localDirectory(activity, tree));
    }

    @Test public void providersAndDeniedAccessRetainSaf() throws Exception {
        android.net.Uri local = android.net.Uri.parse("content://com.android.externalstorage.documents/tree/primary%3ADocuments");
        assertNull(AndroidLibraryStorage.localDirectory(activity, local));
        PermissionEnvironment.granted = true;
        assertNull(AndroidLibraryStorage.localDirectory(activity,
                android.net.Uri.parse("content://example.cloud.documents/tree/books")));
    }

}
