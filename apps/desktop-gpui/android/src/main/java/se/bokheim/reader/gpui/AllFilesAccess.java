package se.bokheim.reader.gpui;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.SharedPreferences;
import android.net.Uri;
import android.os.Build;
import android.os.Environment;
import android.provider.Settings;
import android.widget.Toast;

final class AllFilesAccess {
    private static final String PREFERENCES = "storage_permissions";
    private static final String PROMPT_HANDLED = "all_files_prompt_handled";

    private AllFilesAccess() {}

    static boolean isGranted() {
        return !BuildConfig.PLAY_STORE && Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && Environment.isExternalStorageManager();
    }

    static AlertDialog promptForLaunch(Activity activity, Intent intent) {
        return intent != null && Intent.ACTION_VIEW.equals(intent.getAction()) ? null : promptIfNeeded(activity);
    }

    static AlertDialog promptIfNeeded(Activity activity) {
        if (BuildConfig.PLAY_STORE || Build.VERSION.SDK_INT < Build.VERSION_CODES.R || isGranted()) {
            return null;
        }
        SharedPreferences preferences = activity.getSharedPreferences(PREFERENCES, Activity.MODE_PRIVATE);
        if (preferences.getBoolean(PROMPT_HANDLED, false)) {
            return null;
        }
        Runnable remember = () -> preferences.edit().putBoolean(PROMPT_HANDLED, true).apply();
        return new AlertDialog.Builder(activity)
                .setTitle(UiText.ANDROID_ALLOW_FILE_ACCESS)
                .setMessage(UiText.ANDROID_ALLOW_BOKHEIM_TO_ACCESS_AND_MANAGE_FILES_IN_SHARED_STORAGE_INCLUDING_DOCUMENTS)
                .setPositiveButton(UiText.ANDROID_OPEN_SETTINGS, (dialog, which) -> {
                    remember.run();
                    openSettings(activity);
                })
                .setNegativeButton(UiText.ANDROID_NOT_NOW, (dialog, which) -> remember.run())
                .setOnCancelListener(dialog -> remember.run())
                .show();
    }

    static void openSettings(Activity activity) {
        if (BuildConfig.PLAY_STORE) return;
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            Toast.makeText(activity, UiText.ANDROID_USE_THE_FILE_PICKER_TO_SELECT_FOLDERS_ON_THIS_ANDROID_VERSION, Toast.LENGTH_LONG).show();
            return;
        }
        try {
            activity.startActivity(new Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                    Uri.parse("package:" + activity.getPackageName())));
        } catch (ActivityNotFoundException unavailable) {
            try {
                activity.startActivity(new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION));
            } catch (ActivityNotFoundException missingSettings) {
                Toast.makeText(activity, UiText.ANDROID_FILE_ACCESS_SETTINGS_ARE_UNAVAILABLE_ON_THIS_DEVICE_YOU_CAN_STILL_USE, Toast.LENGTH_LONG).show();
            }
        }
    }
}
