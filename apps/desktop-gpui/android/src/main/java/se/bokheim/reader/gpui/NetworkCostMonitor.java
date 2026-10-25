package se.bokheim.reader.gpui;

import android.content.Context;
import android.net.ConnectivityManager;
import android.net.Network;
import android.net.NetworkCapabilities;
import android.os.Build;
import android.util.Log;
import java.util.function.IntConsumer;

/** One application-scoped listener, independent of Activity recreation. */
final class NetworkCostMonitor extends ConnectivityManager.NetworkCallback {
    private static NetworkCostMonitor instance;
    private final IntConsumer publish;
    private Network current;

    NetworkCostMonitor(IntConsumer publish) {
        this.publish = publish;
    }

    static synchronized void start(Context context) {
        if (instance != null) return;
        instance = new NetworkCostMonitor(MainActivity::nativeNetworkCostChanged);
        instance.register(context.getApplicationContext().getSystemService(ConnectivityManager.class));
    }

    private synchronized void register(ConnectivityManager manager) {
        // Callbacks take the same lock so the initial snapshot cannot overwrite
        // a newer observation. Later callbacks use their supplied capabilities.
        try {
            manager.registerDefaultNetworkCallback(this);
            current = manager.getActiveNetwork();
            publish.accept(cost(manager.getNetworkCapabilities(current)));
        } catch (RuntimeException error) {
            Log.w("Bokheim", "Could not initialize network cost monitoring", error);
            publish.accept(0);
        }
    }

    private static int cost(NetworkCapabilities capabilities) {
        if (capabilities == null) return 0;
        // Android 11+ can waive metering temporarily without setting NOT_METERED.
        // Capability updates also revoke that waiver, suspending transfers again.
        boolean unmetered = capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
                || (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R
                    && capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_TEMPORARILY_NOT_METERED));
        return unmetered ? 1 : 2;
    }

    @Override public synchronized void onAvailable(Network network) {
        current = network;
        // API 26+ delivers onCapabilitiesChanged next. Do not query Binder here.
    }

    @Override public synchronized void onCapabilitiesChanged(Network network, NetworkCapabilities capabilities) {
        if (network.equals(current)) publish.accept(cost(capabilities));
    }

    @Override public synchronized void onLost(Network network) {
        if (network.equals(current)) {
            current = null;
            publish.accept(0);
        }
    }
}
