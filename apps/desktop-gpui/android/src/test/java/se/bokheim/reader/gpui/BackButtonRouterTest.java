package se.bokheim.reader.gpui;

import static org.junit.Assert.*;
import android.view.KeyEvent;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.annotation.Config;

@RunWith(RobolectricTestRunner.class)
@Config(sdk = 30)
public class BackButtonRouterTest {
    @Test public void keyPairAndRepeatsSendExactlyOneNavigationRequest() {
        AtomicInteger calls = new AtomicInteger();
        BackButtonRouter router = new BackButtonRouter(calls::incrementAndGet);
        assertTrue(router.dispatchKeyEvent(new KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_BACK)));
        assertTrue(router.dispatchKeyEvent(new KeyEvent(0, 0, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_BACK, 1)));
        assertTrue(router.dispatchKeyEvent(new KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_BACK)));
        assertEquals(1, calls.get());
        assertTrue(router.dispatchKeyEvent(new KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_BACK)));
        assertEquals(2, calls.get());
    }

    @Test public void releaseIsConsumedEvenWhenTheFirstRequestClosesTheCurrentSurface() {
        AtomicInteger calls = new AtomicInteger();
        BackButtonRouter router = new BackButtonRouter(calls::incrementAndGet);
        router.dispatchKeyEvent(new KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_BACK));
        assertTrue(router.dispatchKeyEvent(new KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_BACK)));
        assertEquals(1, calls.get());
    }

    @Test public void gesturesUseNavigationAndOtherKeysKeepTheirNormalPath() {
        AtomicInteger calls = new AtomicInteger();
        BackButtonRouter router = new BackButtonRouter(calls::incrementAndGet);
        router.onBackPressed();
        assertEquals(1, calls.get());
        assertFalse(router.dispatchKeyEvent(new KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ESCAPE)));
        assertFalse(router.dispatchKeyEvent(new KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_VOLUME_UP)));
        assertEquals(1, calls.get());
    }
}
