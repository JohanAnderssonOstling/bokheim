package se.bokheim.reader.gpui;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.provider.DocumentsContract;
import android.view.Gravity;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Select storage before starting the native backend or any automatic transfers. */
public final class LibrarySetupActivity extends Activity {
    private static final int SELECT_FOLDER = 1;
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private TextView message;
    private Button choose;
    private boolean pickerOpen;
    private boolean busy;
    @Override protected void onCreate(Bundle state) {
        super.onCreate(state);
        pickerOpen = state != null && state.getBoolean("pickerOpen");
        LinearLayout content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setGravity(Gravity.CENTER_VERTICAL);
        int padding = Math.round(24 * getResources().getDisplayMetrics().density);
        content.setPadding(padding, padding, padding, padding);
        TextView title = new TextView(this);
        title.setText(UiText.ACCOUNT_LIBRARY_LOCATION);
        title.setTextSize(24);
        content.addView(title);
        message = new TextView(this);
        message.setText(UiText.ANDROID_CHOOSE_DOCUMENTS_BOKHEIM);
        message.setTextSize(16);
        message.setPadding(0, padding, 0, padding);
        content.addView(message);
        choose = new Button(this);
        choose.setText(UiText.ANDROID_CHOOSE_FOLDER);
        choose.setOnClickListener(view -> selectFolder());
        content.addView(choose, new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        setContentView(content);
        busy = true;
        choose.setEnabled(false);
        worker.execute(() -> {
            boolean ready = DocumentStorage.hasDefault(getApplicationContext());
            runOnUiThread(() -> {
                if (isFinishing() || isDestroyed()) return;
                busy = false;
                choose.setEnabled(true);
                if (ready) enterApp();
                else if (state == null) selectFolder();
            });
        });
    }
    private void selectFolder() {
        if (pickerOpen || busy) return;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        intent.putExtra(Intent.EXTRA_LOCAL_ONLY, true);
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION | Intent.FLAG_GRANT_PREFIX_URI_PERMISSION);
        Uri documents = DocumentsContract.buildDocumentUri("com.android.externalstorage.documents", "primary:Documents");
        intent.putExtra(DocumentsContract.EXTRA_INITIAL_URI, documents);
        pickerOpen = true;
        try { startActivityForResult(intent, SELECT_FOLDER); }
        catch (RuntimeException failure) { pickerOpen = false; message.setText(failure.getMessage()); }
    }
    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != SELECT_FOLDER) return;
        pickerOpen = false;
        if (result != RESULT_OK || data == null) return;
        busy = true;
        choose.setEnabled(false);
        worker.execute(() -> {
            String error = null;
            try { DocumentStorage.saveDefault(getApplicationContext(), DocumentStorage.retain(getApplicationContext(), data)); }
            catch (Exception failure) { error = failure.getMessage(); }
            String failure = error;
            runOnUiThread(() -> {
                if (isFinishing() || isDestroyed()) return;
                busy = false;
                choose.setEnabled(true);
                if (failure == null) enterApp(); else message.setText(failure);
            });
        });
    }
    private void enterApp() {
        Intent intent = new Intent(getIntent());
        intent.setClass(this, MainActivity.class);
        intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP);
        startActivity(intent);
        finish();
    }
    @Override protected void onSaveInstanceState(Bundle state) { state.putBoolean("pickerOpen", pickerOpen); super.onSaveInstanceState(state); }
    @Override protected void onDestroy() { worker.shutdown(); super.onDestroy(); }
}
