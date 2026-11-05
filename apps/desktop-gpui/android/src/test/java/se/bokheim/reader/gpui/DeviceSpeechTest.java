package se.bokheim.reader.gpui;

import static org.junit.Assert.*;
import static org.robolectric.Shadows.shadowOf;
import android.content.Context;
import android.os.Bundle;
import android.os.Looper;
import android.speech.tts.TextToSpeech;
import android.speech.tts.UtteranceProgressListener;
import android.speech.tts.Voice;
import java.io.File;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import org.json.JSONArray;
import org.json.JSONObject;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.RuntimeEnvironment;
import org.robolectric.annotation.Config;
import org.robolectric.annotation.LooperMode;

@RunWith(RobolectricTestRunner.class)
@Config(sdk = 28)
@LooperMode(LooperMode.Mode.PAUSED)
public class DeviceSpeechTest {
    private Context context;
    private FakeTts tts;
    private DeviceSpeech.Factory previousFactory;
    private DeviceSpeech.Events previousEvents;
    private final List<String> completions = new ArrayList<>();
    private final Voice local = voice("local", false, Set.of());

    private static Voice voice(String name, boolean network, Set<String> features) {
        return new Voice(name, Locale.forLanguageTag("sv-SE"), Voice.QUALITY_NORMAL, Voice.LATENCY_NORMAL, network, features);
    }
    private static final class FakeTts extends TextToSpeech {
        Set<Voice> voices = new HashSet<>();
        Voice selected;
        boolean substitute, stopped;
        int syntheses;
        File output;
        Bundle options;
        UtteranceProgressListener listener;
        FakeTts(Context context) { super(context, null, "test.engine"); }
        @Override public List<EngineInfo> getEngines() {
            EngineInfo info = new EngineInfo(); info.name = "test.engine";
            return List.of(info);
        }
        @Override public Set<Voice> getVoices() { return voices; }
        @Override public int setVoice(Voice voice) { selected = substitute ? voice("remote", true, Set.of()) : voice; return SUCCESS; }
        @Override public Voice getVoice() { return selected; }
        @Override public int setSpeechRate(float rate) { return SUCCESS; }
        @Override public int setOnUtteranceProgressListener(UtteranceProgressListener listener) { this.listener = listener; return SUCCESS; }
        @Override public int synthesizeToFile(CharSequence text, Bundle params, File file, String id) {
            syntheses++; output = file; options = params; return SUCCESS;
        }
        @Override public int speak(CharSequence text, int queueMode, Bundle params, String id) { throw new AssertionError("Rust owns playback"); }
        @Override public int stop() { stopped = true; return SUCCESS; }
        @Override public void shutdown() { }
    }
    @Before public void setup() {
        context = RuntimeEnvironment.getApplication();
        previousFactory = DeviceSpeech.factory;
        previousEvents = DeviceSpeech.events;
        tts = new FakeTts(context);
        tts.voices.add(local);
        DeviceSpeech.factory = (ctx, listener, engine) -> { listener.onInit(TextToSpeech.SUCCESS); return tts; };
        DeviceSpeech.events = new DeviceSpeech.Events() {
            public boolean begin(long id, int rate, int format, int channels) { return true; }
            public boolean audio(long id, byte[] bytes) { return true; }
            public void complete(long id, String error) { completions.add(id + ":" + error); }
        };
    }
    @After public void teardown() {
        shadowOf(Looper.getMainLooper()).idle();
        DeviceSpeech.shutdown();
        DeviceSpeech.factory = previousFactory;
        DeviceSpeech.events = previousEvents;
    }
    private String catalogue() throws Exception {
        CompletableFuture<String> future = CompletableFuture.supplyAsync(() -> DeviceSpeech.dispatch(context, "{\"command\":\"voices\"}"));
        long deadline = System.nanoTime() + java.util.concurrent.TimeUnit.SECONDS.toNanos(5);
        while (!future.isDone() && System.nanoTime() < deadline) {
            shadowOf(Looper.getMainLooper()).idle();
            Thread.yield();
        }
        return future.get(1, java.util.concurrent.TimeUnit.SECONDS);
    }
    private void synthesize(long id) throws Exception {
        String command = new JSONObject().put("command", "synthesize").put("id", id)
            .put("voice", new JSONArray().put("test.engine").put("local").toString())
            .put("text", "Ett lokalt prov.").put("rate", 1.25).toString();
        assertEquals("", DeviceSpeech.dispatch(context, command));
        shadowOf(Looper.getMainLooper()).idle();
    }
    @Test public void catalogueExcludesNetworkAndMissingOfflineData() throws Exception {
        tts.voices.add(voice("network", true, Set.of()));
        tts.voices.add(voice("download", false, Set.of(TextToSpeech.Engine.KEY_FEATURE_NOT_INSTALLED)));
        assertFalse(DeviceSpeech.localInstalled(null));
        JSONArray choices = new JSONArray(catalogue());
        assertEquals(1, choices.length());
        assertEquals("local", choices.getJSONObject(0).getString("name"));
        assertEquals("sv-SE", choices.getJSONObject(0).getString("language"));
    }
    @Test public void disappearedOrNetworkVoiceFailsBeforeSynthesis() throws Exception {
        catalogue();
        tts.voices.clear();
        tts.voices.add(voice("local", true, Set.of()));
        synthesize(41);
        assertEquals(0, tts.syntheses);
        assertTrue(completions.get(0).contains("offline"));
    }
    @Test public void engineSubstitutionIsRejectedBeforeSynthesis() throws Exception {
        catalogue();
        tts.substitute = true;
        synthesize(42);
        assertEquals(0, tts.syntheses);
        assertTrue(completions.get(0).contains("Choose another voice"));
    }
    @Test public void cancellationDeletesPrivateAudioAndIgnoresLateCompletion() throws Exception {
        catalogue();
        synthesize(43);
        assertEquals(1, tts.syntheses);
        assertTrue(tts.output.exists());
        assertFalse(tts.options.containsKey(TextToSpeech.Engine.KEY_FEATURE_NETWORK_SYNTHESIS));
        assertEquals("true", tts.options.getString(TextToSpeech.Engine.KEY_FEATURE_EMBEDDED_SYNTHESIS));
        assertEquals("", DeviceSpeech.dispatch(context, "{\"command\":\"cancel\",\"id\":43}"));
        shadowOf(Looper.getMainLooper()).idle();
        assertTrue(tts.stopped);
        assertFalse(tts.output.exists());
        tts.listener.onDone("43");
        assertTrue(completions.isEmpty());
    }
}
