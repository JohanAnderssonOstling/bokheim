package se.bokheim.reader.gpui;

import android.app.Service;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.ServiceConnection;
import android.os.Binder;
import android.os.Build;
import android.os.Bundle;
import android.os.IBinder;
import android.os.Parcel;
import android.os.Process;
import android.os.RemoteException;
import java.io.File;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicReference;

/** A private, one-job process. Its taxonomy globals and SQLite handles must die
 * before the main process promotes any prepared files.
 */
public final class UpdateMigrationService extends Service {
    private static final int PID = IBinder.FIRST_CALL_TRANSACTION;
    private static final int PREPARE = PID + 1;
    private static final int CONNECTED = IBinder.FIRST_CALL_TRANSACTION;
    private static final String CALLBACK = "migration_callback";
    private final AtomicBoolean started = new AtomicBoolean();
    private static native void nativePrepare(String job, String taxonomy);

    private final Binder binder = new Binder() {
        @Override protected boolean onTransact(int code, Parcel data, Parcel reply, int flags) throws RemoteException {
            if (code == PID) { reply.writeInt(Process.myPid()); return true; }
            if (code != PREPARE || !started.compareAndSet(false, true)) return false;
            String path = data.readString();
            new Thread(() -> {
                try {
                    File root = new File(getFilesDir(), "updates/generations").getCanonicalFile();
                    File job = new File(path).getCanonicalFile();
                    if (!job.toPath().startsWith(root.toPath()) || !job.getName().equals("prepare-job.json")) {
                        throw new IllegalArgumentException("Invalid private migration job");
                    }
                    System.loadLibrary("desktop_gpui");
                    nativePrepare(job.getPath(), TaxonomyAssets.prepare(getApplicationContext()).getAbsolutePath());
                } catch (Throwable error) {
                    android.util.Log.e("Bokheim", "Update migration helper failed", error);
                } finally {
                    // Binder death proves all native handles in this process closed.
                    Process.killProcess(Process.myPid());
                }
            }, "update-migration").start();
            return true;
        }
    };

    @Override public IBinder onBind(Intent intent) { return binder; }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        Bundle extras = intent == null ? null : intent.getExtras();
        IBinder callback = extras == null ? null : extras.getBinder(CALLBACK);
        if (callback == null) {
            stopSelf(startId);
            return START_NOT_STICKY;
        }
        Parcel request = Parcel.obtain();
        try {
            request.writeStrongBinder(binder);
            if (!callback.transact(CONNECTED, request, null, IBinder.FLAG_ONEWAY)) {
                throw new RemoteException("Migration client rejected helper");
            }
        } catch (RemoteException error) {
            android.util.Log.e("Bokheim", "Cannot connect migration helper", error);
            stopSelf(startId);
        } finally { request.recycle(); }
        return START_NOT_STICKY;
    }

    /** Called from the native startup worker, never Android's main thread. */
    public static void prepare(Context context, String path) throws Exception {
        if (android.os.Looper.myLooper() == android.os.Looper.getMainLooper()) {
            throw new IllegalStateException("Migration cannot block Android's main thread");
        }
        CountDownLatch exited = new CountDownLatch(1);
        AtomicInteger pid = new AtomicInteger();
        AtomicReference<IBinder> bound = new AtomicReference<>();
        AtomicBoolean requested = new AtomicBoolean();
        AtomicBoolean closed = new AtomicBoolean();
        AtomicReference<Exception> failure = new AtomicReference<>();
        ServiceConnection connection = new ServiceConnection() {
            @Override public void onServiceConnected(ComponentName name, IBinder service) {
                if (closed.get() || !requested.compareAndSet(false, true)) return;
                bound.set(service);
                try {
                    service.linkToDeath(exited::countDown, 0);
                    Parcel request = Parcel.obtain(), reply = Parcel.obtain();
                    try {
                        if (!service.transact(PID, request, reply, 0)) throw new RemoteException("Missing helper PID");
                        pid.set(reply.readInt());
                    } finally { request.recycle(); reply.recycle(); }
                    request = Parcel.obtain();
                    try {
                        request.writeString(path);
                        if (!service.transact(PREPARE, request, null, IBinder.FLAG_ONEWAY)) throw new RemoteException("Helper rejected job");
                    } finally { request.recycle(); }
                } catch (Exception error) { failure.set(error); exited.countDown(); }
            }
            @Override public void onServiceDisconnected(ComponentName name) { exited.countDown(); }
            @Override public void onNullBinding(ComponentName name) {
                failure.set(new IllegalStateException("Migration helper unavailable")); exited.countDown();
            }
            @Override public void onBindingDied(ComponentName name) {
                failure.set(new IllegalStateException("Migration helper binding died")); exited.countDown();
            }
        };
        Intent intent = new Intent(context, UpdateMigrationService.class);
        // GameActivity's main thread can be waiting for the native startup
        // thread to accept its surface. Deliver binding callbacks independently
        // so startup can finish migration before it begins polling that surface.
        ExecutorService callbacks = Executors.newSingleThreadExecutor();
        boolean useBinding = Build.VERSION.SDK_INT >= 29;
        boolean registered = false;
        try {
            if (useBinding) {
                registered = context.bindService(intent, Context.BIND_AUTO_CREATE, callbacks, connection);
                if (!registered) throw new IllegalStateException("Cannot bind migration helper");
            } else {
                // API 26–28 cannot choose a service-binding callback executor.
                // Start the private helper and receive its endpoint directly on
                // a Binder thread instead of the blocked application main loop.
                Bundle extras = new Bundle();
                extras.putBinder(CALLBACK, new Binder() {
                    @Override protected boolean onTransact(int code, Parcel data, Parcel reply, int flags) {
                        if (code != CONNECTED) return false;
                        IBinder service = data.readStrongBinder();
                        if (service == null) connection.onNullBinding(intent.getComponent());
                        else connection.onServiceConnected(intent.getComponent(), service);
                        return true;
                    }
                });
                intent.putExtras(extras);
                registered = context.startService(intent) != null;
                if (!registered) throw new IllegalStateException("Cannot start migration helper");
            }
            if (!exited.await(30, TimeUnit.MINUTES)) throw new IllegalStateException("Migration helper timed out");
            if (failure.get() != null) throw failure.get();
            // Rust reads the atomically written result; death without a result fails.
        } finally {
            closed.set(true);
            int child = pid.get();
            IBinder service = bound.get();
            if (service != null && service.isBinderAlive() && child > 0 && child != Process.myPid()) Process.killProcess(child);
            try {
                if (registered) {
                    if (useBinding) context.unbindService(connection);
                    context.stopService(intent);
                }
            } finally {
                callbacks.shutdownNow();
            }
        }
    }
}
