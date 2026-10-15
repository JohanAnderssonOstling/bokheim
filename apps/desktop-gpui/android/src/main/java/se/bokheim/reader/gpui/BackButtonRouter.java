package se.bokheim.reader.gpui;

import android.view.KeyEvent;

/** Consumes both halves of native Back; each press sends one navigation request. */
final class BackButtonRouter {
    private final Runnable navigate;
    private boolean down;

    BackButtonRouter(Runnable navigate) {
        this.navigate = navigate;
    }

    void onBackPressed() {
        navigate.run();
    }

    boolean dispatchKeyEvent(KeyEvent event) {
        if (event.getKeyCode() != KeyEvent.KEYCODE_BACK) return false;
        if (event.getAction() == KeyEvent.ACTION_DOWN) {
            boolean first = !down && event.getRepeatCount() == 0;
            down = true;
            if (first) navigate.run();
        } else if (event.getAction() == KeyEvent.ACTION_UP) {
            down = false;
        }
        return true;
    }
}
