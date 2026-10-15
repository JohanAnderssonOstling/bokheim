package se.bokheim.reader.gpui;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.SharedPreferences;
import android.net.Uri;
import android.os.PowerManager;
import android.provider.Settings;
import android.widget.Toast;

final class BatteryOptimization {
    private static final String PREFERENCES = "battery_permissions";
    private static final String PROMPT_HANDLED = "battery_prompt_handled";

    private BatteryOptimization() {}

    static boolean isGranted(Activity activity) {
        PowerManager power = activity.getSystemService(PowerManager.class);
        return power != null && power.isIgnoringBatteryOptimizations(activity.getPackageName());
    }

    static AlertDialog promptIfNeeded(Activity activity) {
        if (activity.isFinishing() || activity.isDestroyed() || isGranted(activity)) return null;
        SharedPreferences preferences = activity.getSharedPreferences(PREFERENCES, Activity.MODE_PRIVATE);
        if (preferences.getBoolean(PROMPT_HANDLED, false)) return null;
        Runnable remember = () -> preferences.edit().putBoolean(PROMPT_HANDLED, true).apply();
        return new AlertDialog.Builder(activity)
                .setTitle("Allow uninterrupted background playback")
                .setMessage("Allow Bokheim to ignore battery optimization to help audiobooks and read-aloud keep playing with the screen off. This may use more battery. You can change this later in Android's battery settings.")
                .setPositiveButton("Continue", (dialog, which) -> {
                    remember.run();
                    requestExemption(activity);
                })
                .setNegativeButton("Not now", (dialog, which) -> remember.run())
                .setOnCancelListener(dialog -> remember.run())
                .show();
    }

    static void requestExemption(Activity activity) {
        if (isGranted(activity)) return;
        try {
            activity.startActivity(new Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
                    Uri.parse("package:" + activity.getPackageName())));
        } catch (ActivityNotFoundException | SecurityException unavailable) {
            try {
                activity.startActivity(new Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS));
            } catch (ActivityNotFoundException | SecurityException missingSettings) {
                Toast.makeText(activity, "Battery settings are unavailable on this device. Playback can still continue.", Toast.LENGTH_LONG).show();
            }
        }
    }
}
