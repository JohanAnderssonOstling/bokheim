package se.bokheim.reader.gpui;

import static org.junit.Assert.*;
import static org.robolectric.Shadows.shadowOf;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.os.Looper;
import android.os.PowerManager;
import android.provider.Settings;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.Robolectric;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.android.controller.ActivityController;
import org.robolectric.annotation.Config;
import org.robolectric.annotation.LooperMode;
import org.robolectric.shadows.ShadowToast;

@RunWith(RobolectricTestRunner.class)
@Config(sdk = 28)
@LooperMode(LooperMode.Mode.PAUSED)
public class BatteryOptimizationTest {
    private ActivityController<Activity> controller;
    private Activity activity;

    @Before public void setup() {
        controller = Robolectric.buildActivity(Activity.class).setup();
        activity = controller.get();
        activity.getSharedPreferences("battery_permissions", Activity.MODE_PRIVATE).edit().clear().commit();
    }

    @After public void teardown() { controller.pause().stop().destroy(); }

    @Test public void acceptingRequestsExemptionForThisPackageOnlyOnce() {
        AlertDialog dialog = BatteryOptimization.promptIfNeeded(activity);
        assertNotNull(dialog);
        dialog.getButton(AlertDialog.BUTTON_POSITIVE).performClick();
        shadowOf(Looper.getMainLooper()).idle();
        Intent request = shadowOf(activity).getNextStartedActivity();
        assertEquals(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, request.getAction());
        assertEquals("package:" + activity.getPackageName(), request.getDataString());
        // Opening the system dialog does not itself grant the exemption.
        assertFalse(BatteryOptimization.isGranted(activity));
        assertNull(BatteryOptimization.promptIfNeeded(activity));
    }

    @Test public void decliningDoesNotOpenSettingsOrPromptAgain() {
        AlertDialog dialog = BatteryOptimization.promptIfNeeded(activity);
        dialog.getButton(AlertDialog.BUTTON_NEGATIVE).performClick();
        shadowOf(Looper.getMainLooper()).idle();
        assertNull(shadowOf(activity).getNextStartedActivity());
        assertNull(BatteryOptimization.promptIfNeeded(activity));
    }

    @Test public void cancellingDoesNotPromptAgain() {
        BatteryOptimization.promptIfNeeded(activity).cancel();
        shadowOf(Looper.getMainLooper()).idle();
        assertNull(shadowOf(activity).getNextStartedActivity());
        assertNull(BatteryOptimization.promptIfNeeded(activity));
    }

    @Test public void existingExemptionSkipsBothDialogs() {
        shadowOf(activity.getSystemService(PowerManager.class))
                .setIgnoringBatteryOptimizations(activity.getPackageName(), true);
        assertNull(BatteryOptimization.promptIfNeeded(activity));
        BatteryOptimization.requestExemption(activity);
        assertNull(shadowOf(activity).getNextStartedActivity());
    }

    @Test public void finishingActivityDoesNotShowPrompt() {
        activity.finish();
        assertNull(BatteryOptimization.promptIfNeeded(activity));
    }

    public static class MissingSettingsActivity extends Activity {
        boolean rejectAll;
        Intent fallback;
        @Override public void startActivity(Intent intent) {
            if (rejectAll || Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS.equals(intent.getAction())) {
                throw new ActivityNotFoundException();
            }
            fallback = intent;
        }
    }

    @Test public void unavailableConfirmationFallsBackToBatterySettings() {
        try (ActivityController<MissingSettingsActivity> missing = Robolectric.buildActivity(MissingSettingsActivity.class).setup()) {
            BatteryOptimization.requestExemption(missing.get());
            assertEquals(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS, missing.get().fallback.getAction());
        }
    }

    @Test public void unavailableSettingsDoNotCrashPlayback() {
        try (ActivityController<MissingSettingsActivity> missing = Robolectric.buildActivity(MissingSettingsActivity.class).setup()) {
            missing.get().rejectAll = true;
            BatteryOptimization.requestExemption(missing.get());
            assertTrue(ShadowToast.getTextOfLatestToast().contains("Playback can still continue"));
        }
    }
}
