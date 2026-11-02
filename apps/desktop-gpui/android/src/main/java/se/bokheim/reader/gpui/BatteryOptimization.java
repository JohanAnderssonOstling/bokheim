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
                .setTitle(UiText.ANDROID_ALLOW_UNINTERRUPTED_BACKGROUND_PLAYBACK)
                .setMessage(UiText.ANDROID_ALLOW_BOKHEIM_TO_IGNORE_BATTERY_OPTIMIZATION_TO_HELP_AUDIOBOOKS_AND_READ_ALOUD)
                .setPositiveButton(UiText.BROWSE_CONTINUE, (dialog, which) -> {
                    remember.run();
                    requestExemption(activity);
                })
                .setNegativeButton(UiText.ANDROID_NOT_NOW, (dialog, which) -> remember.run())
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
                Toast.makeText(activity, UiText.ANDROID_BATTERY_SETTINGS_ARE_UNAVAILABLE_ON_THIS_DEVICE_PLAYBACK_CAN_STILL_CONTINUE, Toast.LENGTH_LONG).show();
            }
        }
    }
}
