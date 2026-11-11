package se.bokheim.reader.gpui;

import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.os.Bundle;
import android.util.Log;

import androidx.media3.common.AudioAttributes;
import androidx.media3.common.C;
import androidx.media3.common.MediaItem;
import androidx.media3.common.MediaMetadata;
import androidx.media3.common.PlaybackException;
import androidx.media3.common.Player;
import androidx.media3.exoplayer.ExoPlayer;
import androidx.media3.session.MediaSession;
import androidx.media3.session.MediaSessionService;
import androidx.media3.session.CommandButton;
import androidx.media3.session.SessionCommand;
import androidx.media3.session.SessionResult;
import com.google.common.util.concurrent.Futures;
import com.google.common.util.concurrent.ListenableFuture;

import org.json.JSONObject;
import java.io.File;

/** Owns audio independently of the GameActivity, including focus and media buttons. */
@androidx.annotation.OptIn(markerClass = androidx.media3.common.util.UnstableApi.class)
public final class AudiobookService extends MediaSessionService {
    private static native int nativeReadAudio(long id, long offset, byte[] buffer, int bufferOffset, int length);
    private static native void nativeInterruptAudio(long id);
    static java.util.function.LongConsumer sourceInterrupt = AudiobookService::nativeInterruptAudio;

    interface AudioReader { int read(long id, long offset, byte[] buffer, int bufferOffset, int length); }

    static final class AudioSource extends androidx.media3.datasource.BaseDataSource {
        private long id, position, remaining;
        private Uri uri;
        private final AudioReader reader;
        AudioSource() { this(AudiobookService::nativeReadAudio); }
        AudioSource(AudioReader reader) { super(true); this.reader = reader; }
        @Override public long open(androidx.media3.datasource.DataSpec spec) throws java.io.IOException {
            transferInitializing(spec);
            id = Long.parseLong(spec.uri.getHost());
            position = spec.position;
            long length = Long.parseLong(spec.uri.getLastPathSegment());
            if (position > length) throw new java.io.IOException("Audio position exceeds file length");
            remaining = length - position;
            if (spec.length != C.LENGTH_UNSET) remaining = Math.min(remaining, spec.length);
            uri = spec.uri;
            transferStarted(spec);
            return remaining;
        }
        @Override public int read(byte[] buffer, int offset, int length) throws java.io.IOException {
            if (length == 0) return 0;
            if (remaining == 0) return C.RESULT_END_OF_INPUT;
            int requested = (int) Math.min(Math.min(length, remaining), 256 * 1024);
            // Fill Media3's existing buffer instead of allocating a Java array for each read.
            int count = reader.read(id, position, buffer, offset, requested);
            if (count <= 0 || count > requested) throw new java.io.IOException("Could not read audiobook stream");
            position += count;
            remaining -= count;
            bytesTransferred(count);
            return count;
        }
        @Override public Uri getUri() { return uri; }
        @Override public void close() {
            if (uri != null) { uri = null; transferEnded(); }
        }
    }

    static final SessionCommand SKIP_BACK = new SessionCommand("bokheim.skip_back", Bundle.EMPTY);
    static final SessionCommand SKIP_FORWARD = new SessionCommand("bokheim.skip_forward", Bundle.EMPTY);

    static final SessionCommand PREVIOUS_CHAPTER = new SessionCommand("bokheim.previous_chapter", Bundle.EMPTY);
    static final SessionCommand NEXT_CHAPTER = new SessionCommand("bokheim.next_chapter", Bundle.EMPTY);

    static final class PlaybackControls implements MediaSession.Callback {
        @Override public MediaSession.ConnectionResult onConnect(MediaSession session, MediaSession.ControllerInfo controller) {
            MediaSession.ConnectionResult defaults = MediaSession.Callback.super.onConnect(session, controller);
            return new MediaSession.ConnectionResult.AcceptedResultBuilder(session)
                .setAvailableSessionCommands(defaults.availableSessionCommands.buildUpon().add(SKIP_BACK).add(SKIP_FORWARD).add(PREVIOUS_CHAPTER).add(NEXT_CHAPTER).build())
                .setAvailablePlayerCommands(defaults.availablePlayerCommands.buildUpon()
                    .remove(Player.COMMAND_SEEK_TO_PREVIOUS).remove(Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM)
                    .remove(Player.COMMAND_SEEK_TO_NEXT).remove(Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM).build())
                .build();
        }

        @Override public ListenableFuture<SessionResult> onCustomCommand(MediaSession session, MediaSession.ControllerInfo controller, SessionCommand command, Bundle args) {
            ChapterPlayer player = (ChapterPlayer) session.getPlayer();
            if (SKIP_BACK.equals(command)) player.skip(-15_000);
            else if (SKIP_FORWARD.equals(command)) player.skip(30_000);
            else if (PREVIOUS_CHAPTER.equals(command)) player.previousChapter();
            else if (NEXT_CHAPTER.equals(command)) player.nextChapter();
            else return Futures.immediateFuture(new SessionResult(SessionResult.RESULT_ERROR_NOT_SUPPORTED));
            return Futures.immediateFuture(new SessionResult(SessionResult.RESULT_SUCCESS));
        }
    }
    // Commands stay in-process; exported service intents cannot supply file paths.
    private static final java.util.ArrayDeque<JSONObject> pendingCommands = new java.util.ArrayDeque<>();
    private static final Handler MAIN = new Handler(Looper.getMainLooper());
    private static AudiobookService instance;
    private ExoPlayer player;
    private MediaSession session;
    private ChapterPlayer sessionPlayer;
    private long bookId;
    private long sequence;
    private boolean releasing;
    private boolean stopping;
    private long sleepDeadline = -1;
    private long sleepChapterEnd = -1;
    private boolean sleepFinished;
    private long sleepDuration;
    private long[] chapterEnds = new long[0];
    private JSONObject sleepReset;
    private final StringBuilder playbackState = new StringBuilder(384);
    private android.hardware.SensorManager sensors;
    private boolean shakeListening;
    private boolean shakePeak;
    private long firstShakePeak = -1;
    private long lastShakeReset = -1;
    private final android.hardware.SensorEventListener shakeListener = new android.hardware.SensorEventListener() {
        @Override public void onAccuracyChanged(android.hardware.Sensor sensor, int accuracy) {}
        @Override public void onSensorChanged(android.hardware.SensorEvent event) {
            float x = event.values[0], y = event.values[1], z = event.values[2];
            handleShakeAcceleration((float) Math.sqrt(x * x + y * y + z * z) / android.hardware.SensorManager.GRAVITY_EARTH,
                android.os.SystemClock.elapsedRealtime());
        }
    };

    private static native void nativePlaybackState(String state);
    // Injectable transport for service tests; production forwards to Rust persistence.
    static java.util.function.Consumer<String> stateListener = AudiobookService::nativePlaybackState;

    public static void dispatch(Context context, String command) {
        MAIN.post(() -> {
            try {
                JSONObject data = new JSONObject(command);
                if (instance != null && !instance.stopping) {
                    instance.execute(data);
                } else if ("load".equals(data.getString("command")) || !pendingCommands.isEmpty()) {
                    // User-initiated opening from the activity. Media3 promotes
                    // the service to foreground as playback starts.
                    boolean start = pendingCommands.isEmpty();
                    pendingCommands.addLast(data);
                    try { if (start) context.startService(new Intent(context, AudiobookService.class)); }
                    catch (RuntimeException error) { pendingCommands.remove(data); throw error; }
                } else {
                    reportFailure(data.optLong("id"), "The audiobook service has stopped");
                }
            } catch (Exception error) {
                try { reportFailure(new JSONObject(command).optLong("id"), error); }
                catch (Exception ignored) { Log.e("Bokheim", "Invalid audiobook command", error); }
            }
        });
    }

    // Run before GameActivity waits for native teardown, while callbacks can
    // still save the final position and release native playback waiters.
    static void stopForAppClose(Context context) {
        pendingCommands.clear();
        if (instance != null) {
            AudiobookService service = instance;
            service.stopPlayback(true);
            service.releasePlayback();
        }
        context.stopService(new Intent(context, AudiobookService.class));
    }

    private void stopPlayback(boolean stopService) {
        stopping = stopService;
        clearSleepTimer();
        publish(true, null);
        bookId = 0;
        player.stop();
        player.clearMediaItems();
        if (stopping) stopSelf();
    }

    @Override public void onCreate() {
        super.onCreate();
        instance = this;
        sensors = (android.hardware.SensorManager) getSystemService(Context.SENSOR_SERVICE);
        player = new ExoPlayer.Builder(this)
            .setMediaSourceFactory(new androidx.media3.exoplayer.source.DefaultMediaSourceFactory(
                new androidx.media3.datasource.DefaultDataSource.Factory(this, AudioSource::new)))
            .setAudioAttributes(new AudioAttributes.Builder()
                .setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_SPEECH).build(), true)
            .setHandleAudioBecomingNoisy(true)
            .setWakeMode(C.WAKE_MODE_LOCAL)
            .setSeekBackIncrementMs(15_000)
            .setSeekForwardIncrementMs(30_000)
            .build();
        Intent open = new Intent(this, MainActivity.class).setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);
        sessionPlayer = new ChapterPlayer(player, position -> {
            try { stateListener.accept(new JSONObject().put("id", bookId).put("sequence", sequence).put("seekTarget", position).toString()); }
            catch (org.json.JSONException error) { Log.e("Bokheim", "Could not request chapter seek", error); }
        });
        session = new MediaSession.Builder(this, sessionPlayer)
            .setCallback(new PlaybackControls())
            .setMediaButtonPreferences(java.util.Arrays.asList(
                new CommandButton.Builder(CommandButton.ICON_SKIP_BACK_15).setDisplayName(UiText.COMMON_BACK_15_SECONDS)
                    .setSessionCommand(SKIP_BACK).setSlots(CommandButton.SLOT_BACK).build(),
                new CommandButton.Builder(CommandButton.ICON_SKIP_FORWARD_30).setDisplayName(UiText.COMMON_FORWARD_30_SECONDS)
                    .setSessionCommand(SKIP_FORWARD).setSlots(CommandButton.SLOT_FORWARD).build(),
                new CommandButton.Builder(CommandButton.ICON_PREVIOUS).setDisplayName(UiText.AUDIOBOOK_PREVIOUS_CHAPTER)
                    .setSessionCommand(PREVIOUS_CHAPTER).setSlots(CommandButton.SLOT_OVERFLOW).build(),
                new CommandButton.Builder(CommandButton.ICON_NEXT).setDisplayName(UiText.AUDIOBOOK_NEXT_CHAPTER)
                    .setSessionCommand(NEXT_CHAPTER).setSlots(CommandButton.SLOT_OVERFLOW).build()))
            .setSessionActivity(PendingIntent.getActivity(this, 0, open, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE))
            .build();
        // Rust dispatches directly instead of binding a MediaController, so
        // onGetSession is not invoked to register this session automatically.
        // Registration enables Media3 notification and foreground management.
        addSession(session);
        player.addListener(new Player.Listener() {
            @Override public void onPositionDiscontinuity(Player.PositionInfo oldPosition, Player.PositionInfo newPosition, int reason) {
                if (bookId != 0 && reason == Player.DISCONTINUITY_REASON_SEEK) sourceInterrupt.accept(bookId);
            }
            @Override public void onEvents(Player ignored, Player.Events events) {
                publish(false, null, events.contains(Player.EVENT_POSITION_DISCONTINUITY));
            }
            @Override public void onPlayerError(PlaybackException error) { publishFailure(false, error); }
        });
        MAIN.post(tick);
    }

    @Override public MediaSession onGetSession(MediaSession.ControllerInfo controller) {
        return controller.getUid() == android.os.Process.myUid() || controller.isTrusted() ? session : null;
    }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        int result = super.onStartCommand(intent, flags, startId);
        while (!pendingCommands.isEmpty()) {
            JSONObject command = pendingCommands.removeFirst();
            try { execute(command); }
            catch (Exception error) {
                reportFailure(command.optLong("id"), error);
                stopSelf();
            }
        }
        return result;
    }

    private void execute(JSONObject command) throws Exception {
        String action = command.getString("command");
        long id = command.getLong("id");
        if ("load".equals(action)) {
            stopping = false;
            publish(true, null);
            bookId = 0;
            clearSleepTimer();
            player.stop();
            player.clearMediaItems();
            bookId = id;
            sequence = command.optLong("sequence");
            org.json.JSONArray ends = command.optJSONArray("chapterEnds");
            chapterEnds = new long[ends == null ? 0 : ends.length()];
            for (int i = 0; i < chapterEnds.length; i++) chapterEnds[i] = ends.getLong(i);
            MediaMetadata.Builder metadata = new MediaMetadata.Builder().setTitle(command.getString("title"));
            if (!command.isNull("author")) metadata.setArtist(command.getString("author"));
            byte[] cover = MediaArtwork.bytes(command);
            if (cover != null) metadata.setArtworkData(cover, MediaMetadata.PICTURE_TYPE_FRONT_COVER);
            MediaMetadata bookMetadata = metadata.build();
            sessionPlayer.configure(command, bookMetadata);
            MediaItem item = new MediaItem.Builder()
                .setMediaId(Long.toString(id))
                .setUri(command.isNull("path") ? Uri.parse("bokheim-audio://" + id + "/" + command.getLong("length")) : Uri.fromFile(new File(command.getString("path"))))
                .setMimeType("audio/mp4").setMediaMetadata(bookMetadata).build();
            player.setPlaybackSpeed((float) command.optDouble("speed", 1.0));
            player.setMediaItem(item, command.optLong("position"));
            player.setPlayWhenReady(command.optBoolean("playWhenReady", true));
            player.prepare();
        } else if (id == bookId) {
            switch (action) {
                case "pause":
                    player.pause();
                    break;
                case "toggle":
                    if (player.getPlayWhenReady() && player.getPlaybackState() != Player.STATE_ENDED) player.pause();
                    else {
                        if (player.getPlaybackState() == Player.STATE_ENDED) player.seekTo(0);
                        if (player.getPlaybackState() == Player.STATE_IDLE) player.prepare();
                        player.play();
                    }
                    break;
                case "seek":
                    sequence = command.getLong("sequence");
                    sessionPlayer.clearPendingSeek();
                    player.seekTo(command.getLong("position"));
                    break;
                case "artwork":
                    MediaItem current = player.getCurrentMediaItem();
                    if (current != null) {
                        MediaMetadata updated = current.mediaMetadata.buildUpon().setArtworkData(MediaArtwork.bytes(command), MediaMetadata.PICTURE_TYPE_FRONT_COVER).build();
                        sessionPlayer.setArtwork(updated);
                        player.replaceMediaItem(0, current.buildUpon().setMediaMetadata(updated).build());
                    }
                    break;
                case "speed": player.setPlaybackSpeed((float) command.getDouble("speed")); break;
                case "sleep":
                    clearSleepTimer();
                    if (!command.isNull("remaining")) {
                        sleepDuration = Math.max(0, command.getLong("remaining"));
                        sleepDeadline = android.os.SystemClock.elapsedRealtime() + sleepDuration;
                    }
                    if (!command.isNull("chapterEnd")) sleepChapterEnd = command.getLong("chapterEnd");
                    startShakeListening();
                    updateSleepTimer(android.os.SystemClock.elapsedRealtime());
                    break;
                case "detach":
                case "stop":
                    stopPlayback("stop".equals(action));
                    break;
                default: throw new IllegalArgumentException("Unknown audiobook command: " + action);
            }
        }
        publish(false, null);
    }

    private void startShakeListening() {
        if (shakeListening || sensors == null || (sleepDeadline < 0 && sleepChapterEnd < 0)) return;
        android.hardware.Sensor sensor = sensors.getDefaultSensor(android.hardware.Sensor.TYPE_ACCELEROMETER);
        if (sensor != null) shakeListening = sensors.registerListener(shakeListener, sensor, android.hardware.SensorManager.SENSOR_DELAY_GAME);
    }

    private void stopShakeListening() {
        if (shakeListening) sensors.unregisterListener(shakeListener);
        shakeListening = false;
        shakePeak = false;
        firstShakePeak = -1;
        lastShakeReset = -1;
    }

    // Two distinct strong impulses reject ordinary handling; a cooldown avoids
    // extending several chapters during one shake gesture.
    void handleShakeAcceleration(float gravity, long now) {
        if (sleepDeadline < 0 && sleepChapterEnd < 0) return;
        if (gravity < 1.5f) { shakePeak = false; return; }
        if (gravity < 2.7f || shakePeak) return;
        shakePeak = true;
        if (lastShakeReset >= 0 && now - lastShakeReset < 2000) return;
        if (firstShakePeak < 0 || now - firstShakePeak > 700) { firstShakePeak = now; return; }
        firstShakePeak = -1;
        lastShakeReset = now;
        resetSleepFromShake(now);
    }

    private void resetSleepFromShake(long now) {
        try {
            if (sleepDeadline >= 0) {
                sleepDeadline = now + sleepDuration;
                sleepReset = new JSONObject().put("remaining", sleepDuration);
            } else if (sleepChapterEnd >= 0) {
                long after = Math.max(sleepChapterEnd, player.getCurrentPosition());
                long next = -1;
                for (long end : chapterEnds) if (end > after) { next = end; break; }
                if (next < 0) return;
                sleepChapterEnd = next;
                sleepReset = new JSONObject().put("chapterEnd", next);
            } else return;
            player.setVolume(1f);
            publish(false, null);
        } catch (org.json.JSONException error) { Log.e("Bokheim", "Could not reset sleep timer", error); }
    }

    private void clearSleepTimer() {
        sleepDeadline = -1;
        sleepChapterEnd = -1;
        sleepFinished = false;
        sleepReset = null;
        stopShakeListening();
        player.setVolume(1f);
    }

    // Runs in the service, including with the activity or screen asleep.
    void updateSleepTimer(long now) {
        if (sleepDeadline < 0 && sleepChapterEnd < 0) return;
        double remaining = sleepDeadline >= 0 ? sleepDeadline - now
            : (sleepChapterEnd - player.getCurrentPosition()) / (double) player.getPlaybackParameters().speed;
        if (remaining <= 0) {
            sleepDeadline = -1;
            sleepChapterEnd = -1;
            stopShakeListening();
            player.pause();
            player.setVolume(1f);
            sleepFinished = true;
        } else {
            player.setVolume((float) Math.min(1.0, remaining / 30_000.0));
        }
    }

    private final Runnable tick = new Runnable() {
        @Override public void run() { updateSleepTimer(android.os.SystemClock.elapsedRealtime()); sessionPlayer.refresh(); publish(false, null); MAIN.postDelayed(this, 250); }
    };

    private void publish(boolean released, String error) { publish(released, error, false); }

    private void publishFailure(boolean released, Throwable error) {
        publish(released, error.toString(), false, FailureDetails.classify(error));
    }

    private void publish(boolean released, String error, boolean force) {
        publish(released, error, force, null);
    }

    private void publish(boolean released, String error, boolean force, String failure) {
        if (bookId == 0 || releasing) return;
        try {
            // The 250 ms position update contains primitive fields. Reuse its writer rather
            // than creating a JSONObject, map entries and boxed numbers on every tick.
            playbackState.setLength(0);
            playbackState.append("{\"id\":").append(bookId).append(",\"sequence\":").append(sequence)
                .append(",\"position\":").append(Math.max(0, player.getCurrentPosition()))
                .append(",\"playing\":").append(!released && player.isPlaying())
                .append(",\"requested\":").append(!released && player.getPlayWhenReady()
                    && player.getPlaybackSuppressionReason() == Player.PLAYBACK_SUPPRESSION_REASON_NONE)
                .append(",\"buffering\":").append(!released && player.getPlayWhenReady()
                    && player.getPlaybackState() == Player.STATE_BUFFERING
                    && player.getPlaybackSuppressionReason() == Player.PLAYBACK_SUPPRESSION_REASON_NONE)
                .append(",\"ready\":").append(player.getPlaybackState() == Player.STATE_READY || player.getPlaybackState() == Player.STATE_ENDED)
                .append(",\"ended\":").append(player.getPlaybackState() == Player.STATE_ENDED)
                .append(",\"released\":").append(released).append(",\"speed\":").append(player.getPlaybackParameters().speed)
                .append(",\"force\":").append(force).append(",\"sleepFinished\":").append(sleepFinished);
            if (sleepReset != null) playbackState.append(",\"sleepReset\":").append(sleepReset);
            if (error != null) playbackState.append(",\"error\":").append(JSONObject.quote(error));
            if (failure != null) playbackState.append(",\"failure\":").append(JSONObject.quote(failure));
            playbackState.append('}');
            stateListener.accept(playbackState.toString());
            sleepFinished = false;
            sleepReset = null;
        } catch (Exception errorReporting) { Log.e("Bokheim", "Could not report playback state", errorReporting); }
    }

    private static void reportFailure(long id, Throwable error) {
        try { stateListener.accept(FailureDetails.put(new JSONObject().put("id", id).put("released", true), "error", error).toString()); }
        catch (org.json.JSONException reporting) { Log.e("Bokheim", "Could not report audio error", reporting); }
    }

    private static void reportFailure(long id, String error) {
        try { stateListener.accept(new JSONObject().put("id", id).put("released", true).put("error", error).toString()); }
        catch (Exception errorReporting) { Log.e("Bokheim", "Could not report audio error", errorReporting); }
    }

    private void releasePlayback() {
        if (releasing) return;
        publish(true, null);
        releasing = true;
        if (instance == this) instance = null;
        MAIN.removeCallbacks(tick);
        stopShakeListening();
        if (session != null) session.release();
        if (sessionPlayer != null) sessionPlayer.release();
        else if (player != null) player.release();
        stopForeground(STOP_FOREGROUND_REMOVE);
    }

    @Override public void onDestroy() {
        releasePlayback();
        super.onDestroy();
    }
}
