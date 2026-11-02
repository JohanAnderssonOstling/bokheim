package se.bokheim.reader.gpui;

import static org.junit.Assert.*;

import android.content.Context;
import android.content.ComponentName;
import android.content.ContextWrapper;
import android.content.Intent;
import android.content.ServiceConnection;
import android.os.Binder;
import android.os.Build;
import android.os.Bundle;
import android.os.IBinder;
import android.os.Parcel;
import android.os.RemoteException;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.Executor;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.Robolectric;
import org.robolectric.RuntimeEnvironment;
import org.robolectric.android.controller.ServiceController;
import org.robolectric.annotation.Config;
import org.robolectric.annotation.LooperMode;

@RunWith(RobolectricTestRunner.class)
@Config(sdk = {26, 28, 30})
@LooperMode(LooperMode.Mode.PAUSED)
public class UpdateMigrationServiceTest {
    @Test public void migrationStartsAndFinishesWhileTheMainLooperIsBlocked() throws Exception {
        CountDownLatch requested = new CountDownLatch(1);
        IBinder.DeathRecipient[] death = new IBinder.DeathRecipient[1];
        Binder helper = new Binder() {
            @Override public void linkToDeath(IBinder.DeathRecipient recipient, int flags) {
                death[0] = recipient;
            }
            @Override protected boolean onTransact(int code, Parcel data, Parcel reply, int flags) throws RemoteException {
                if (code == IBinder.FIRST_CALL_TRANSACTION) {
                    reply.writeInt(0);
                    return true;
                }
                assertEquals(IBinder.FIRST_CALL_TRANSACTION + 1, code);
                assertEquals("prepare-job.json", data.readString());
                requested.countDown();
                return true;
            }
        };
        BindingContext context = new BindingContext(helper);
        ExecutorService startup = Executors.newSingleThreadExecutor();
        Future<?> result = startup.submit(() -> {
            UpdateMigrationService.prepare(context, "prepare-job.json");
            return null;
        });
        try {
            // Do not drain the main looper: GameActivity can be blocked there
            // until this native startup worker finishes migration.
            assertTrue("migration must start without main-loop callbacks", requested.await(5, TimeUnit.SECONDS));
            assertFalse(result.isDone());
            death[0].binderDied();
            result.get(5, TimeUnit.SECONDS);
            assertEquals(Build.VERSION.SDK_INT >= 29, context.unbound);
            assertTrue(context.stopped);
            if (context.callbacks != null) assertTrue(context.callbacks.isShutdown());
        } finally {
            result.cancel(true);
            startup.shutdownNow();
        }
    }

    @Test public void failedHelperLaunchReleasesResources() throws Exception {
        BindingContext context = new BindingContext(null);
        ExecutorService startup = Executors.newSingleThreadExecutor();
        try {
            startup.submit(() -> {
                IllegalStateException error = assertThrows(IllegalStateException.class,
                    () -> UpdateMigrationService.prepare(context, "prepare-job.json"));
                assertEquals(Build.VERSION.SDK_INT >= 29 ? "Cannot bind migration helper" : "Cannot start migration helper", error.getMessage());
            }).get(5, TimeUnit.SECONDS);
            if (context.callbacks != null) assertTrue(context.callbacks.isShutdown());
            assertFalse(context.unbound);
        } finally {
            startup.shutdownNow();
        }
    }

    @Test public void startedHelperReturnsItsBinderWithoutAClientMainLoopCallback() throws Exception {
        ServiceController<UpdateMigrationService> controller = Robolectric.buildService(UpdateMigrationService.class).create();
        try {
            UpdateMigrationService service = controller.get();
            CountDownLatch connected = new CountDownLatch(1);
            Bundle extras = new Bundle();
            extras.putBinder("migration_callback", new Binder() {
                @Override protected boolean onTransact(int code, Parcel data, Parcel reply, int flags) {
                    assertEquals(IBinder.FIRST_CALL_TRANSACTION, code);
                    assertEquals(IBinder.FLAG_ONEWAY, flags);
                    assertSame(service.onBind(new Intent()), data.readStrongBinder());
                    connected.countDown();
                    return true;
                }
            });
            assertEquals(android.app.Service.START_NOT_STICKY, service.onStartCommand(new Intent().putExtras(extras), 0, 1));
            assertTrue(connected.await(5, TimeUnit.SECONDS));
        } finally { controller.destroy(); }
    }

    private static final class BindingContext extends ContextWrapper {
        private final IBinder helper;
        ExecutorService callbacks;
        boolean unbound;
        boolean stopped;

        BindingContext(IBinder helper) {
            super(RuntimeEnvironment.getApplication());
            this.helper = helper;
        }

        @Override public boolean bindService(Intent intent, int flags, Executor executor, ServiceConnection connection) {
            assertEquals(Context.BIND_AUTO_CREATE, flags);
            callbacks = (ExecutorService) executor;
            if (helper == null) return false;
            executor.execute(() -> connection.onServiceConnected(intent.getComponent(), helper));
            return true;
        }

        @Override public boolean bindService(Intent intent, ServiceConnection connection, int flags) {
            throw new AssertionError("Binding callbacks must not depend on Android's main thread");
        }

        @Override public ComponentName startService(Intent intent) {
            if (helper == null) return null;
            Parcel request = Parcel.obtain();
            try {
                request.writeStrongBinder(helper);
                assertTrue(intent.getExtras().getBinder("migration_callback").transact(
                    IBinder.FIRST_CALL_TRANSACTION, request, null, IBinder.FLAG_ONEWAY));
            } catch (RemoteException error) {
                throw new AssertionError(error);
            } finally { request.recycle(); }
            return intent.getComponent();
        }

        @Override public void unbindService(ServiceConnection connection) { unbound = true; }
        @Override public boolean stopService(Intent intent) { stopped = true; return true; }
    }
}
