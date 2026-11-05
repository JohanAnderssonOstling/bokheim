package se.bokheim.reader.gpui;

import android.content.Context;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.speech.tts.TextToSpeech;
import android.speech.tts.UtteranceProgressListener;
import android.speech.tts.Voice;
import java.io.File;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.TimeUnit;
import org.json.JSONArray;
import org.json.JSONObject;

/** Produces PCM from installed local voices. Rust alone plays the generated audio. */
final class DeviceSpeech {
    private static final Handler MAIN = new Handler(Looper.getMainLooper());
    private static final ConcurrentHashMap<String, CompletableFuture<Engine>> ENGINES = new ConcurrentHashMap<>();
    private static final ConcurrentHashMap<Long, Request> REQUESTS = new ConcurrentHashMap<>();
    private static native boolean nativeBegin(long id, int rate, int format, int channels);
    private static native boolean nativeAudio(long id, byte[] bytes);
    private static native void nativeComplete(long id, String error);

    interface Events {
        boolean begin(long id, int rate, int format, int channels);
        boolean audio(long id, byte[] bytes);
        void complete(long id, String error);
    }
    static Events events = new Events() {
        public boolean begin(long id, int rate, int format, int channels) { return nativeBegin(id, rate, format, channels); }
        public boolean audio(long id, byte[] bytes) { return nativeAudio(id, bytes); }
        public void complete(long id, String error) { nativeComplete(id, error); }
    };
    interface Factory { TextToSpeech create(Context context, TextToSpeech.OnInitListener listener, String engine); }
    static Factory factory = (context, listener, engine) -> engine.isEmpty() ? new TextToSpeech(context, listener) : new TextToSpeech(context, listener, engine);

    private static final class Engine {
        final TextToSpeech tts;
        volatile long used = android.os.SystemClock.uptimeMillis();
        Engine(TextToSpeech tts) { this.tts = tts; }
    }
    private static final class Request {
        final long id;
        final String engine;
        volatile TextToSpeech tts;
        volatile File output;
        Request(long id, String engine) { this.id = id; this.engine = engine; }
    }

    static boolean localInstalled(Voice voice) {
        return voice != null && !voice.isNetworkConnectionRequired()
            && (voice.getFeatures() == null || !voice.getFeatures().contains(TextToSpeech.Engine.KEY_FEATURE_NOT_INSTALLED));
    }

    private static CompletableFuture<Engine> engine(Context context, String name) {
        return ENGINES.computeIfAbsent(name, key -> {
            CompletableFuture<Engine> future = new CompletableFuture<>();
            MAIN.post(() -> {
                final TextToSpeech[] holder = new TextToSpeech[1];
                try {
                    holder[0] = factory.create(context, status -> MAIN.post(() -> {
                        if (future.isDone()) {
                            if (holder[0] != null) holder[0].shutdown();
                            return;
                        }
                        if (status != TextToSpeech.SUCCESS || holder[0] == null) {
                            if (holder[0] != null) holder[0].shutdown();
                            ENGINES.remove(key, future);
                            future.completeExceptionally(new IllegalStateException(UiText.ANDROID_THE_INSTALLED_LOCAL_SPEECH_ENGINE_COULD_NOT_INITIALIZE));
                            return;
                        }
                        holder[0].setOnUtteranceProgressListener(listener());
                        Engine ready = new Engine(holder[0]);
                        future.complete(ready);
                        reap(key, future, ready);
                    }), key);
                    MAIN.postDelayed(() -> {
                        if (future.completeExceptionally(new IllegalStateException(UiText.ANDROID_THE_LOCAL_SPEECH_ENGINE_TOOK_TOO_LONG_TO_INITIALIZE))) {
                            ENGINES.remove(key, future);
                            if (holder[0] != null) holder[0].shutdown();
                        }
                    }, 15000);
                } catch (RuntimeException error) { ENGINES.remove(key, future); future.completeExceptionally(error); }
            });
            return future;
        });
    }

    private static void reap(String key, CompletableFuture<Engine> future, Engine engine) {
        MAIN.postDelayed(() -> {
            if (ENGINES.get(key) != future) return;
            boolean busy = REQUESTS.values().stream().anyMatch(request -> request.tts == engine.tts);
            if (!busy && android.os.SystemClock.uptimeMillis() - engine.used >= 60000 && ENGINES.remove(key, future)) engine.tts.shutdown();
            else reap(key, future, engine);
        }, 60000);
    }

    private static UtteranceProgressListener listener() {
        return new UtteranceProgressListener() {
            @Override public void onStart(String utterance) { }
            @Override public void onBeginSynthesis(String utterance, int rate, int format, int channels) {
                Request request = request(utterance);
                if (request != null && !events.begin(request.id, rate, format, channels)) MAIN.post(() -> cancel(request.id));
            }
            @Override public void onAudioAvailable(String utterance, byte[] bytes) {
                Request request = request(utterance);
                if (request != null && !events.audio(request.id, bytes)) MAIN.post(() -> cancel(request.id));
            }
            @Override public void onDone(String utterance) { finish(utterance, null); }
            @Override public void onError(String utterance) { finish(utterance, UiText.ANDROID_THE_INSTALLED_LOCAL_VOICE_COULD_NOT_GENERATE_AUDIO_TRY_ANOTHER_LOCAL_VOICE); }
            @Override public void onError(String utterance, int code) {
                String message;
                switch (code) {
                    case TextToSpeech.ERROR_NOT_INSTALLED_YET: message = UiText.ANDROID_LOCAL_VOICE_DATA_IS_NOT_INSTALLED_INSTALL_IT_IN_DEVICE_SETTINGS_OR; break;
                    case TextToSpeech.ERROR_NETWORK:
                    case TextToSpeech.ERROR_NETWORK_TIMEOUT: message = UiText.ANDROID_THIS_VOICE_ATTEMPTED_NETWORK_SYNTHESIS_CHOOSE_AN_INSTALLED_OFFLINE_VOICE; break;
                    case TextToSpeech.ERROR_INVALID_REQUEST: message = UiText.ANDROID_THE_LOCAL_VOICE_REJECTED_THIS_READING_SAMPLE; break;
                    default:
                        android.util.Log.w("Bokheim", "Local speech synthesis failed: " + code);
                        message = UiText.ANDROID_LOCAL_SYNTHESIS_FAILED;
                }
                finish(utterance, message);
            }
            @Override public void onStop(String utterance, boolean interrupted) { finish(utterance, UiText.ANDROID_LOCAL_VOICE_SYNTHESIS_WAS_INTERRUPTED_START_READING_AGAIN_TO_RETRY); }
        };
    }
    private static Request request(String utterance) {
        try { return REQUESTS.get(Long.parseLong(utterance)); } catch (NumberFormatException ignored) { return null; }
    }
    private static void finish(String utterance, String error) {
        Request request = request(utterance);
        if (request == null || !REQUESTS.remove(request.id, request)) return;
        events.complete(request.id, error);
        if (request.output != null) request.output.delete();
    }
    private static void cancel(long id) {
        Request request = REQUESTS.remove(id);
        if (request == null) return;
        if (request.tts != null) request.tts.stop();
        if (request.output != null) request.output.delete();
    }

    private static String list(Context context) throws Exception {
        long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(18);
        Engine bootstrap = engine(context, "").get(15, TimeUnit.SECONDS);
        List<TextToSpeech.EngineInfo> installed = bootstrap.tts.getEngines();
        JSONArray catalogue = new JSONArray();
        if (installed == null) return catalogue.toString();
        for (TextToSpeech.EngineInfo info : installed) {
            long remaining = deadline - System.nanoTime();
            if (remaining <= 0) break;
            try {
                Engine engine = engine(context, info.name).get(Math.min(remaining, TimeUnit.SECONDS.toNanos(3)), TimeUnit.NANOSECONDS);
                engine.used = android.os.SystemClock.uptimeMillis();
                Set<Voice> voices = engine.tts.getVoices();
                if (voices == null) continue;
                List<Voice> sorted = new ArrayList<>(voices);
                sorted.sort(Comparator.comparing(Voice::getName));
                for (Voice voice : sorted) {
                    if (!localInstalled(voice)) continue;
                    catalogue.put(new JSONObject().put("id", new JSONArray().put(info.name).put(voice.getName()).toString())
                        .put("name", voice.getName()).put("language", voice.getLocale().toLanguageTag()));
                }
            } catch (Exception unavailable) { /* Another installed engine can still provide local voices. */ }
        }
        return catalogue.toString();
    }

    @SuppressWarnings("deprecation")
    private static void synthesize(Context context, JSONObject command) throws Exception {
        long id = command.getLong("id");
        JSONArray choice = new JSONArray(command.getString("voice"));
        if (choice.length() != 2) throw new IllegalArgumentException(UiText.ANDROID_INVALID_LOCAL_VOICE_IDENTITY);
        String packageName = choice.getString(0), voiceName = choice.getString(1), text = command.getString("text");
        float rate = (float) command.getDouble("rate");
        if (text.length() > TextToSpeech.getMaxSpeechInputLength() || !Float.isFinite(rate) || rate < 0.5f || rate > 3f) throw new IllegalArgumentException(UiText.ANDROID_INVALID_READING_SAMPLE_OR_SPEED);
        Request request = new Request(id, packageName);
        REQUESTS.put(id, request);
        engine(context, "").thenCompose(bootstrap -> {
            if (bootstrap.tts.getEngines().stream().noneMatch(info -> info.name.equals(packageName))) throw new IllegalStateException(UiText.ANDROID_THE_SELECTED_LOCAL_SPEECH_ENGINE_IS_NO_LONGER_INSTALLED);
            return engine(context, packageName);
        }).whenComplete((engine, error) -> MAIN.post(() -> {
            if (REQUESTS.get(id) != request) return;
            if (error != null) { finish(Long.toString(id), UiText.ANDROID_THE_SELECTED_LOCAL_SPEECH_ENGINE_COULD_NOT_INITIALIZE_TRY_ANOTHER_INSTALLED_VOICE); return; }
            try {
                engine.used = android.os.SystemClock.uptimeMillis();
                request.tts = engine.tts;
                Set<Voice> available = engine.tts.getVoices();
                Voice voice = available == null ? null : available.stream().filter(candidate -> candidate.getName().equals(voiceName)).findFirst().orElse(null);
                if (!localInstalled(voice)) throw new IllegalStateException(UiText.ANDROID_THE_SELECTED_VOICE_IS_NOT_INSTALLED_FOR_OFFLINE_USE_CHOOSE_ANOTHER_LOCAL);
                if (engine.tts.setVoice(voice) != TextToSpeech.SUCCESS || !localInstalled(engine.tts.getVoice()) || !voiceName.equals(engine.tts.getVoice().getName())) throw new IllegalStateException(UiText.ANDROID_THE_SELECTED_LOCAL_VOICE_COULD_NOT_BE_ACTIVATED);
                if (engine.tts.setSpeechRate(rate) != TextToSpeech.SUCCESS) throw new IllegalStateException(UiText.ANDROID_THE_INSTALLED_LOCAL_VOICE_DOES_NOT_SUPPORT_THIS_READING_SPEED);
                request.output = File.createTempFile("bokheim-local-", ".wav", context.getCacheDir());
                Bundle options = new Bundle();
                options.putString(TextToSpeech.Engine.KEY_FEATURE_EMBEDDED_SYNTHESIS, "true");
                // Never set the network-synthesis feature, even to a false string.
                if (engine.tts.synthesizeToFile(text, options, request.output, Long.toString(id)) != TextToSpeech.SUCCESS) throw new IllegalStateException(UiText.ANDROID_THE_INSTALLED_LOCAL_VOICE_REJECTED_THE_READING_SAMPLE);
            } catch (Exception failure) { finish(Long.toString(id), failure.getMessage()); }
        }));
    }

    // Release initialized engines and pending request files after speech has stopped.
    static void shutdown() {
        for (Long id : REQUESTS.keySet()) cancel(id);
        for (CompletableFuture<Engine> future : ENGINES.values()) {
            if (future.isCompletedExceptionally()) continue;
            Engine ready = future.getNow(null);
            if (ready != null) ready.tts.shutdown();
            else future.completeExceptionally(new IllegalStateException(UiText.ANDROID_LOCAL_SPEECH_STOPPED));
        }
        ENGINES.clear();
    }

    static String dispatch(Context context, String encoded) {
        try {
            JSONObject command = new JSONObject(encoded);
            switch (command.getString("command")) {
                case "voices":
                    if (Looper.myLooper() == Looper.getMainLooper()) throw new IllegalStateException(UiText.ANDROID_LOCAL_VOICE_DISCOVERY_MUST_RUN_IN_THE_BACKGROUND);
                    return list(context);
                case "synthesize": synthesize(context, command); return "";
                case "cancel": long id = command.getLong("id"); MAIN.post(() -> cancel(id)); return "";
                default: throw new IllegalArgumentException(UiText.ANDROID_UNKNOWN_LOCAL_SPEECH_COMMAND);
            }
        } catch (Exception error) {
            String message = error.getMessage();
            try { return new JSONObject().put("error", message == null ? UiText.ANDROID_LOCAL_VOICE_DISCOVERY_FAILED_TRY_AGAIN : message).toString(); }
            catch (Exception ignored) { return "{\"error\":\"Local speech is unavailable.\"}"; }
        }
    }
}
