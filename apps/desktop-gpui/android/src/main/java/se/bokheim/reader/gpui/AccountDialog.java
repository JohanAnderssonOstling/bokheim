package se.bokheim.reader.gpui;

import android.app.Activity;
import android.app.Dialog;
import android.graphics.Typeface;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.graphics.drawable.StateListDrawable;
import android.os.Build;
import android.text.InputType;
import android.text.method.PasswordTransformationMethod;
import android.view.Gravity;
import android.view.View;
import android.view.Window;
import android.view.WindowManager;
import android.view.autofill.AutofillManager;
import android.view.inputmethod.EditorInfo;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import androidx.core.graphics.ColorUtils;
import androidx.core.graphics.Insets;
import androidx.core.view.ViewCompat;
import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import org.json.JSONObject;
import org.json.JSONException;
import java.util.ArrayList;
import java.util.List;

/** Real Android fields, with one stable form throughout an account request. */
final class AccountDialog extends Dialog {
    // Order matches the browser theme sent by open_android_authentication.
    private static final int BACKGROUND = 0, TEXT = 1, MUTED = 2, ACCENT = 3,
        ACCENT_TEXT = 4, BORDER = 5, LINK = 6, DANGER = 7, SELECTION = 8;
    interface Submit { void send(long id, String message); }
    final long sessionId;
    private final Submit submit;
    private final int minimum;
    private final int[] colors;
    private final LinearLayout form;
    private final AutofillManager autofill;
    private final List<Button> buttons = new ArrayList<>();
    private String flow = "login";
    private String pending;
    private boolean busy, finished, committed;
    private EditText email, password, pin, token;
    private TextView feedback;

    AccountDialog(Activity activity, long id, int minimum, int[] colors, Submit submit) {
        super(activity, ColorUtils.calculateLuminance(colors[BACKGROUND]) > 0.5
            ? android.R.style.Theme_Material_Light_NoActionBar : android.R.style.Theme_Material_NoActionBar);
        this.sessionId = id;
        this.minimum = minimum;
        this.colors = colors;
        this.submit = submit;
        autofill = activity.getSystemService(AutofillManager.class);
        ScrollView scroll = new ScrollView(getContext());
        scroll.setFillViewport(true);
        scroll.setBackgroundColor(colors[BACKGROUND]);
        scroll.setClipToPadding(false);
        LinearLayout column = new LinearLayout(getContext());
        column.setOrientation(LinearLayout.VERTICAL);
        column.setGravity(Gravity.CENTER_HORIZONTAL);
        form = new LinearLayout(getContext()) {
            @Override protected void onMeasure(int width, int height) {
                super.onMeasure(MeasureSpec.makeMeasureSpec(Math.min(MeasureSpec.getSize(width), dp(480)), MeasureSpec.EXACTLY), height);
            }
        };
        form.setOrientation(LinearLayout.VERTICAL);
        form.setPadding(dp(24), dp(24), dp(24), dp(24));
        column.addView(form, new LinearLayout.LayoutParams(-1, -2));
        scroll.addView(column);
        setContentView(scroll);
        Window window = getWindow();
        window.setBackgroundDrawableResource(android.R.color.transparent);
        WindowCompat.setDecorFitsSystemWindows(window, false);
        window.setStatusBarColor(Color.TRANSPARENT);
        window.setNavigationBarColor(Color.TRANSPARENT);
        boolean light = ColorUtils.calculateLuminance(colors[BACKGROUND]) > 0.5;
        WindowCompat.getInsetsController(window, scroll).setAppearanceLightStatusBars(light);
        WindowCompat.getInsetsController(window, scroll).setAppearanceLightNavigationBars(light);
        if (Build.VERSION.SDK_INT >= 29) window.setNavigationBarContrastEnforced(false);
        ViewCompat.setOnApplyWindowInsetsListener(scroll, (view, insets) -> {
            Insets bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() | WindowInsetsCompat.Type.displayCutout());
            int keyboard = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom;
            view.setPadding(bars.left, bars.top, bars.right, Math.max(bars.bottom, keyboard));
            return insets;
        });
        window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        setCanceledOnTouchOutside(false);
        render("login", "", "");
    }
    @Override public void dismiss() {
        // Dialog's dismiss listener is asynchronous. Mark this session closed
        // now, before an already queued network response can commit credentials.
        if (!finished) {
            finished = true;
            if (!committed && autofill != null) autofill.cancel();
            emit("closed", "", "", "");
        }
        super.dismiss();
    }
    @Override public void show() {
        super.show();
        getWindow().setLayout(WindowManager.LayoutParams.MATCH_PARENT, WindowManager.LayoutParams.MATCH_PARENT);
        ViewCompat.requestApplyInsets(getWindow().getDecorView());
        email.requestFocus();
    }
    private int dp(int value) { return Math.round(value * getContext().getResources().getDisplayMetrics().density); }
    private TextView text(String value, int size) {
        TextView view = new TextView(getContext());
        view.setText(value);
        view.setTextSize(size);
        view.setTextColor(colors[TEXT]);
        view.setTypeface(Typeface.SERIF);
        view.setPadding(0, dp(8), 0, dp(8));
        form.addView(view);
        return view;
    }
    private EditText field(int id, String label, int inputType, String hint, String value) {
        TextView caption = text(label, 16);
        LinearLayout.LayoutParams captionLayout = new LinearLayout.LayoutParams(-1, -2);
        captionLayout.topMargin = dp(12);
        caption.setLayoutParams(captionLayout);
        caption.setLabelFor(id);
        EditText input = new EditText(getContext());
        input.setId(id);
        input.setInputType(inputType);
        input.setSingleLine(true);
        input.setTextColor(colors[TEXT]);
        input.setHintTextColor(colors[MUTED]);
        input.setTextSize(16);
        input.setHighlightColor(colors[SELECTION]);
        input.setBackgroundTintList(null);
        StateListDrawable background = new StateListDrawable();
        background.addState(new int[]{android.R.attr.state_focused}, surface(colors[BACKGROUND], colors[ACCENT], 2));
        background.addState(new int[]{}, surface(colors[BACKGROUND], colors[BORDER], 1));
        input.setBackground(background);
        input.setPadding(dp(12), dp(12), dp(12), dp(12));
        input.setMinHeight(dp(56));
        if (Build.VERSION.SDK_INT >= 29) {
            GradientDrawable cursor = surface(colors[ACCENT], colors[ACCENT], 0);
            cursor.setSize(dp(2), Math.round(input.getTextSize() * 1.2f));
            input.setTextCursorDrawable(cursor);
            Drawable[] handles = {input.getTextSelectHandle(), input.getTextSelectHandleLeft(), input.getTextSelectHandleRight()};
            for (Drawable handle : handles) if (handle != null) handle.mutate().setTint(colors[ACCENT]);
        }
        input.setTypeface(Typeface.SERIF);
        input.setSaveEnabled(false);
        input.setImportantForAutofill(hint == null ? View.IMPORTANT_FOR_AUTOFILL_NO : View.IMPORTANT_FOR_AUTOFILL_YES);
        if (hint != null) input.setAutofillHints(hint);
        input.setText(value);
        input.setImeOptions(EditorInfo.IME_ACTION_NEXT | EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING);
        form.addView(input, new LinearLayout.LayoutParams(-1, -2));
        return input;
    }
    private GradientDrawable surface(int fill, int stroke, int width) {
        GradientDrawable shape = new GradientDrawable();
        shape.setColor(fill);
        if (width > 0) shape.setStroke(dp(width), stroke);
        return shape;
    }
    private void styleButton(Button button, boolean primary) {
        button.setTextColor(colors[primary ? ACCENT_TEXT : LINK]);
        button.setBackgroundTintList(null);
        StateListDrawable background = new StateListDrawable();
        background.addState(new int[]{android.R.attr.state_focused}, surface(colors[primary ? ACCENT : SELECTION], colors[primary ? ACCENT_TEXT : ACCENT], 2));
        background.addState(new int[]{}, surface(colors[primary ? ACCENT : BACKGROUND], colors[ACCENT], 0));
        button.setBackground(new RippleDrawable(ColorStateList.valueOf(ColorUtils.setAlphaComponent(colors[primary ? ACCENT_TEXT : ACCENT], 40)), background,
            surface(Color.WHITE, Color.WHITE, 0)));
        button.setStateListAnimator(null);
        button.setElevation(0);
    }
    private Button button(String label, Runnable action) {
        Button button = new Button(getContext());
        button.setText(label);
        button.setAllCaps(false);
        button.setTextSize(16);
        button.setTypeface(Typeface.SERIF);
        button.setMinHeight(dp(48));
        button.setMinimumHeight(dp(48));
        button.setPadding(dp(12), dp(10), dp(12), dp(10));
        styleButton(button, false);
        button.setOnClickListener(ignored -> { if (!busy) action.run(); });
        buttons.add(button);
        form.addView(button, new LinearLayout.LayoutParams(-1, -2));
        return button;
    }
    private void feedback(String message, boolean error) {
        feedback.setText(message);
        feedback.setTextColor(colors[error ? DANGER : MUTED]);
        feedback.setVisibility(message.isEmpty() ? View.GONE : View.VISIBLE);
    }
    private void change(String next) {
        if (autofill != null) autofill.cancel();
        render(next, email.getText().toString().trim(), "");
    }
    private void render(String next, String address, String message) {
        flow = next;
        committed = false;
        form.removeAllViews();
        buttons.clear();
        password = pin = token = null;
        String title;
        switch (flow) {
            case "register": title = "Create account"; break;
            case "verify": title = "Check your email"; break;
            case "request-reset": title = "Reset your password"; break;
            case "reset": title = "Set a new password"; break;
            default: title = "Sign in";
        }
        setTitle(title);
        LinearLayout header = new LinearLayout(getContext());
        header.setGravity(Gravity.CENTER_VERTICAL);
        TextView heading = text(title, 28);
        form.removeView(heading);
        header.addView(heading, new LinearLayout.LayoutParams(0, -2, 1));
        Button close = button("Close", this::dismiss);
        form.removeView(close);
        header.addView(close, new LinearLayout.LayoutParams(-2, -2));
        form.addView(header);
        if (android.os.Build.VERSION.SDK_INT >= 28) heading.setAccessibilityHeading(true);
        TextView identity = text("Bokheim · app.bokheim.se", 16);
        identity.setTextColor(colors[MUTED]);
        feedback = text(message, 16);
        feedback.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);
        feedback(message, false);
        email = field(R.id.account_email, "Email", InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            flow.equals("register") ? "newUsername" : View.AUTOFILL_HINT_USERNAME, address);
        if (flow.equals("login") || flow.equals("register") || flow.equals("reset")) {
            password = field(R.id.account_password, flow.equals("reset") ? "New password" : "Password",
                InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD,
                flow.equals("login") ? View.AUTOFILL_HINT_PASSWORD : "newPassword", "");
            password.setTransformationMethod(PasswordTransformationMethod.getInstance());
            Button reveal = button("Show password", () -> {});
            LinearLayout.LayoutParams revealLayout = new LinearLayout.LayoutParams(-2, -2);
            revealLayout.gravity = Gravity.END;
            reveal.setLayoutParams(revealLayout);
            reveal.setOnClickListener(ignored -> {
                if (busy) return;
                boolean hidden = password.getTransformationMethod() instanceof PasswordTransformationMethod;
                password.setTransformationMethod(hidden ? null : PasswordTransformationMethod.getInstance());
                password.setSelection(password.length());
                reveal.setText(hidden ? "Hide password" : "Show password");
                reveal.setContentDescription(hidden ? "Hide password" : "Show password");
            });
            if (!flow.equals("login")) text("Use at least " + minimum + " characters.", 14).setTextColor(colors[MUTED]);
        }
        if (flow.equals("verify")) pin = field(R.id.account_pin, "Verification PIN", InputType.TYPE_CLASS_NUMBER, null, "");
        if (flow.equals("reset")) token = field(R.id.account_reset_token, "Reset code from your email", InputType.TYPE_CLASS_TEXT, null, "");
        EditText last = token != null ? token : pin != null ? pin : password != null ? password : email;
        last.setImeOptions(EditorInfo.IME_ACTION_DONE | EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING);
        last.setOnEditorActionListener((v, action, event) -> {
            if (action == EditorInfo.IME_ACTION_DONE) { perform(flow); return true; }
            return false;
        });
        String action = flow.equals("verify") ? "Verify email" : flow.equals("request-reset") ? "Email me a reset code" : flow.equals("reset") ? "Set new password" : title;
        Button primary = button(action, () -> perform(flow));
        styleButton(primary, true);
        LinearLayout.LayoutParams primaryLayout = new LinearLayout.LayoutParams(-1, -2);
        primaryLayout.topMargin = dp(16);
        primaryLayout.bottomMargin = dp(8);
        primary.setLayoutParams(primaryLayout);
        if (flow.equals("login")) {
            button("Forgot password?", () -> change("request-reset"));
            button("Create account", () -> change("register"));
        } else {
            if (flow.equals("verify")) button("Resend PIN", () -> perform("resend"));
            button("Back to sign in", () -> change("login"));
        }
        email.requestFocus();
    }
    private void setBusy(boolean value) {
        busy = value;
        setCancelable(!value);
        for (Button button : buttons) { button.setEnabled(!value); button.setAlpha(value ? 0.55f : 1f); }
        for (EditText field : new EditText[]{email, password, pin, token}) if (field != null) { field.setEnabled(!value); field.setAlpha(value ? 0.55f : 1f); }
    }
    private void perform(String action) {
        if (busy || finished) return;
        String address = email.getText().toString().trim();
        String secret = pin != null && action.equals("verify") ? pin.getText().toString() : password != null ? password.getText().toString() : "";
        String resetToken = token == null ? "" : token.getText().toString().trim();
        if (!android.util.Patterns.EMAIL_ADDRESS.matcher(address).matches()) { email.setError("Enter a valid email address"); email.requestFocus(); return; }
        if ((action.equals("login") && secret.isEmpty()) || ((action.equals("register") || action.equals("reset")) && secret.codePointCount(0, secret.length()) < minimum)) {
            password.setError(action.equals("login") ? "Enter your password" : "Use at least " + minimum + " characters"); password.requestFocus(); return;
        }
        if (action.equals("verify") && !secret.matches("[0-9]{6}")) { pin.setError("Enter the six-digit PIN"); pin.requestFocus(); return; }
        if (action.equals("reset") && resetToken.isEmpty()) { token.setError("Enter the reset code from your email"); token.requestFocus(); return; }
        pending = action;
        setBusy(true);
        feedback("Please wait…", false);
        emit(action, address, secret, resetToken);
    }
    private void emit(String action, String email, String secret, String token) {
        try {
            JSONObject message = new JSONObject().put("action", action).put("email", email).put("secret", secret).put("token", token);
            submit.send(sessionId, message.toString());
        } catch (JSONException impossible) { throw new IllegalStateException(impossible); }
    }
    void complete(String error) {
        if (!busy || finished) return;
        setBusy(false);
        if (error != null) { feedback(error, true); return; }
        String address = email.getText().toString().trim();
        if (pending.equals("login")) {
            // Only sign-in proves this exact email/password pair is valid.
            // Registration hides conflicts; reset tokens may belong to another
            // address. Never offer those unverified pairs as saved credentials.
            if (autofill != null) autofill.commit();
            committed = true;
        } else if (!pending.equals("resend") && autofill != null) {
            autofill.cancel();
        }
        switch (pending) {
            case "login": case "verify": dismiss(); break;
            case "register": render("verify", address, "Check your email for a six-digit verification PIN."); break;
            case "resend": feedback("Verification PIN sent if the address exists.", false); break;
            case "request-reset": render("reset", address, "Check your email for a reset code."); break;
            case "reset": render("login", address, "Password updated. Sign in with your new password."); break;
        }
    }
}
