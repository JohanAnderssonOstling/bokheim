<script lang="ts">
  import { onMount } from "svelte";
  import { ChevronDown, ChevronUp, Pause, Play, RotateCcw, RotateCw, X } from "@lucide/svelte";
  import type { MobileAudiobookSession } from "../lib/audiobook";
  import {
    closeNativeAudiobook,
    getNativeAudiobookState,
    isIgnoringBatteryOptimizations,
    nativeAudiobookAvailable,
    observeNativeAudiobookState,
    openBatteryOptimizationSettings,
    seekNativeAudiobookBy,
    seekNativeAudiobookTo,
    setNativeAudiobookPlaybackRate,
    startNativeAudiobook,
    toggleNativeAudiobook,
    type NativeAudiobookState,
  } from "../lib/native-audiobook";

  let { session, aboveNavigation, onclose }: { session: MobileAudiobookSession; aboveNavigation: boolean; onclose: () => void } = $props();

  let audio = $state<HTMLAudioElement>();
  let playing = $state(false);
  let currentTime = $state(0);
  let duration = $state(0);
  let expanded = $state(false);
  let playbackRate = $state(1);
  let error = $state("");
  let initializedKey = "";
  let lastSavedSeconds = -1;
  let showBatteryNudge = $state(false);
  const useNativeAudio = nativeAudiobookAvailable();
  const BATTERY_NUDGE_SNOOZE_KEY = "bokheim-background-audio-nudge-snoozed-until";
  const BATTERY_NUDGE_DELAY_MS = 2_000;
  const BATTERY_NUDGE_SNOOZE_MS = 30 * 24 * 60 * 60 * 1000;

  const progress = $derived(duration > 0 ? Math.min(1, Math.max(0, currentTime / duration)) : 0);
  const activeChapterIndex = $derived.by(() => {
    const positionMs = currentTime * 1000;
    const exact = session.chapters.findIndex((chapter) => positionMs >= chapter.start_ms && positionMs < chapter.end_ms);
    if (exact >= 0) return exact;
    for (let index = session.chapters.length - 1; index >= 0; index -= 1) {
      if (positionMs >= session.chapters[index].start_ms) return index;
    }
    return 0;
  });

  function formatTime(seconds: number) {
    if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
    const whole = Math.floor(seconds);
    const hours = Math.floor(whole / 3600);
    const minutes = Math.floor((whole % 3600) / 60);
    const remainder = whole % 60;
    return hours > 0 ? `${hours}:${String(minutes).padStart(2, "0")}:${String(remainder).padStart(2, "0")}` : `${minutes}:${String(remainder).padStart(2, "0")}`;
  }

  async function persist(force = false) {
    const position = useNativeAudio ? currentTime : audio?.currentTime;
    if (position === undefined || !Number.isFinite(position)) return;
    if (!force && lastSavedSeconds >= 0 && Math.abs(position - lastSavedSeconds) < 5) return;
    lastSavedSeconds = position;
    const total = !useNativeAudio && audio && Number.isFinite(audio.duration) && audio.duration > 0 ? audio.duration : duration;
    const savedProgress = total > 0 ? Math.min(100, Math.max(0, position / total * 100)) : 0;
    try {
      await session.savePosition(Math.round(position * 1000), savedProgress);
    } catch (reason) {
      console.warn("Could not save audiobook position", reason);
    }
  }

  function updateMediaPosition() {
    if (!("mediaSession" in navigator) || !audio || !Number.isFinite(audio.duration) || audio.duration <= 0) return;
    try {
      navigator.mediaSession.setPositionState({ duration: audio.duration, playbackRate: audio.playbackRate, position: Math.min(audio.duration, Math.max(0, audio.currentTime)) });
    } catch {
      // Some WebViews expose MediaSession before position state is supported.
    }
  }

  function applyPlaybackRate() {
    if (useNativeAudio) {
      setNativeAudiobookPlaybackRate(playbackRate);
      return;
    }
    if (audio) audio.playbackRate = playbackRate;
    updateMediaPosition();
  }

  function seekBy(seconds: number) {
    if (useNativeAudio) {
      currentTime = Math.min(duration > 0 ? duration : Number.MAX_SAFE_INTEGER, Math.max(0, currentTime + seconds));
      seekNativeAudiobookBy(seconds * 1000);
      void persist(true);
      return;
    }
    if (!audio) return;
    const end = Number.isFinite(audio.duration) ? audio.duration : Number.MAX_SAFE_INTEGER;
    audio.currentTime = Math.min(end, Math.max(0, audio.currentTime + seconds));
    currentTime = audio.currentTime;
    updateMediaPosition();
    void persist(true);
  }

  function seekToFraction(fraction: number) {
    if (useNativeAudio) {
      if (duration > 0) {
        currentTime = duration * Math.min(1, Math.max(0, fraction));
        seekNativeAudiobookTo(currentTime * 1000);
        void persist(true);
      }
      return;
    }
    if (!audio || !Number.isFinite(audio.duration) || audio.duration <= 0) return;
    audio.currentTime = audio.duration * Math.min(1, Math.max(0, fraction));
    currentTime = audio.currentTime;
    updateMediaPosition();
    void persist(true);
  }

  function seekToChapter(startMs: number) {
    const seconds = Math.max(0, startMs / 1000);
    currentTime = seconds;
    if (useNativeAudio) seekNativeAudiobookTo(startMs);
    else if (audio) {
      audio.currentTime = seconds;
      updateMediaPosition();
    }
    void persist(true);
  }

  async function togglePlayback() {
    if (useNativeAudio) {
      error = "";
      toggleNativeAudiobook();
      return;
    }
    if (!audio) return;
    error = "";
    try {
      if (audio.paused) await audio.play(); else audio.pause();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    }
  }

  function closePlayer() {
    void persist(true);
    if (useNativeAudio) closeNativeAudiobook();
    else if (audio) audio.pause();
    if ("mediaSession" in navigator) navigator.mediaSession.metadata = null;
    onclose();
  }

  function openExpandedFromDock() {
    if (!useNativeAudio || expanded) return;
    expanded = true;
  }

  function batteryNudgeIsSnoozed() {
    try {
      return Number(localStorage.getItem(BATTERY_NUDGE_SNOOZE_KEY) ?? 0) > Date.now();
    } catch {
      return false;
    }
  }

  function refreshBatteryNudge() {
    showBatteryNudge = useNativeAudio
      && isIgnoringBatteryOptimizations() === false
      && !batteryNudgeIsSnoozed();
  }

  function snoozeBatteryNudge() {
    showBatteryNudge = false;
    try {
      localStorage.setItem(BATTERY_NUDGE_SNOOZE_KEY, String(Date.now() + BATTERY_NUDGE_SNOOZE_MS));
    } catch {
      // The nudge can remain session-only when WebView storage is unavailable.
    }
  }

  function reviewBatterySetting() {
    showBatteryNudge = false;
    openBatteryOptimizationSettings();
  }

  function initialize() {
    if (useNativeAudio || !audio || initializedKey === session.key) return;
    initializedKey = session.key;
    lastSavedSeconds = session.initialPositionMs / 1000;
    const initialSeconds = Math.max(0, session.initialPositionMs / 1000);
    if (Number.isFinite(audio.duration) && audio.duration > 0) audio.currentTime = Math.min(audio.duration, initialSeconds);
    currentTime = audio.currentTime;
    duration = Number.isFinite(audio.duration) && audio.duration > 0 ? audio.duration : (session.expectedDurationMs ?? 0) / 1000;
    if ("mediaSession" in navigator) {
      navigator.mediaSession.metadata = new MediaMetadata({ title: session.title, artist: session.author || undefined, artwork: session.coverUrl ? [{ src: session.coverUrl }] : undefined });
    }
    updateMediaPosition();
    void audio.play().catch(() => {
      // Mobile autoplay policy can require a second explicit press.
    });
  }

  onMount(() => {
    const saveBeforeSuspend = () => void persist(true);
    const saveWhenHidden = () => { if (document.visibilityState === "hidden") void persist(true); };
    window.addEventListener("pagehide", saveBeforeSuspend);
    document.addEventListener("visibilitychange", saveWhenHidden);
    let unobserveNative: () => void = () => {};
    let batteryNudgeTimer: ReturnType<typeof setTimeout> | undefined;
    if (useNativeAudio) {
      const initialState = getNativeAudiobookState();
      const applyNativeState = (state: NativeAudiobookState) => {
        if (!state.active || state.key !== session.nativeKey) return;
        currentTime = state.positionMs / 1000;
        duration = (state.durationMs ?? session.expectedDurationMs ?? 0) / 1000;
        playing = state.playing;
        playbackRate = state.playbackRate;
        error = state.error ?? "";
        void persist();
      };
      applyNativeState(initialState);
      unobserveNative = observeNativeAudiobookState(applyNativeState);
      if (!initialState.active || initialState.key !== session.nativeKey) startNativeAudiobook(session);
      batteryNudgeTimer = setTimeout(refreshBatteryNudge, BATTERY_NUDGE_DELAY_MS);
      window.addEventListener("focus", refreshBatteryNudge);
    } else if ("mediaSession" in navigator) {
      navigator.mediaSession.setActionHandler("play", () => void audio?.play());
      navigator.mediaSession.setActionHandler("pause", () => audio?.pause());
      navigator.mediaSession.setActionHandler("seekbackward", (details) => seekBy(-(details.seekOffset ?? 15)));
      navigator.mediaSession.setActionHandler("seekforward", (details) => seekBy(details.seekOffset ?? 30));
      navigator.mediaSession.setActionHandler("seekto", (details) => {
        if (details.seekTime !== undefined && audio) {
          audio.currentTime = details.seekTime;
          currentTime = audio.currentTime;
          void persist(true);
        }
      });
    }
    return () => {
      saveBeforeSuspend();
      window.removeEventListener("pagehide", saveBeforeSuspend);
      document.removeEventListener("visibilitychange", saveWhenHidden);
      unobserveNative();
      if (batteryNudgeTimer) clearTimeout(batteryNudgeTimer);
      window.removeEventListener("focus", refreshBatteryNudge);
      if (!useNativeAudio && "mediaSession" in navigator) {
        for (const action of ["play", "pause", "seekbackward", "seekforward", "seekto"] as MediaSessionAction[]) navigator.mediaSession.setActionHandler(action, null);
        navigator.mediaSession.metadata = null;
      }
    };
  });
</script>

<audio
  bind:this={audio}
  src={useNativeAudio ? undefined : session.source}
  preload="auto"
  onloadedmetadata={initialize}
  ondurationchange={() => { if (audio && Number.isFinite(audio.duration)) duration = audio.duration; }}
  onplay={() => { playing = true; if ("mediaSession" in navigator) navigator.mediaSession.playbackState = "playing"; }}
  onpause={() => { playing = false; if ("mediaSession" in navigator) navigator.mediaSession.playbackState = "paused"; void persist(true); }}
  ontimeupdate={() => { if (audio) { currentTime = audio.currentTime; updateMediaPosition(); void persist(); } }}
  onended={() => { playing = false; void persist(true); }}
  onerror={() => (error = "This device could not play the audiobook file")}
></audio>

{#if expanded}
  <div class="expanded-overlay" role="dialog" aria-modal="true" aria-label="Audiobook controls">
    <button class="overlay-scrim" type="button" aria-label="Collapse audiobook controls" onclick={() => (expanded = false)}></button>
    <section class="expanded-sheet">
      <header>
        <strong>Now playing</strong>
        <div>
          <button type="button" aria-label="Collapse audiobook controls" onclick={() => (expanded = false)}><ChevronDown size={22} /></button>
          <button type="button" aria-label="Close audiobook player" onclick={closePlayer}><X size={22} /></button>
        </div>
      </header>
      <div class="expanded-book">
        <div class="expanded-cover" aria-hidden="true">
          {#if session.coverUrl}<img src={session.coverUrl} alt="" />{:else}<span>{session.title}</span>{/if}
        </div>
        <div class="expanded-copy">
          <strong>{session.title}</strong>
          <span>{session.author || "Unknown author"}</span>
          {#if error}<small role="alert">{error}</small>{/if}
        </div>
      </div>
      <div class="expanded-timeline">
        <input aria-label="Audiobook position" type="range" min="0" max="1000" value={Math.round(progress * 1000)} oninput={(event) => seekToFraction(Number(event.currentTarget.value) / 1000)} />
        <div><span>{formatTime(currentTime)}</span><span>{formatTime(duration)}</span></div>
      </div>
      <div class="expanded-transport" aria-label="Playback controls">
        <button type="button" aria-label="Back 15 seconds" onclick={() => seekBy(-15)}><RotateCcw size={26} /><small>15</small></button>
        <button class="play" type="button" aria-label={playing ? "Pause" : "Play"} onclick={togglePlayback}>{#if playing}<Pause size={34} />{:else}<Play size={34} />{/if}</button>
        <button type="button" aria-label="Forward 30 seconds" onclick={() => seekBy(30)}><RotateCw size={26} /><small>30</small></button>
      </div>
      <section class="chapter-browser" aria-label="Chapters">
        <header><strong>Chapters</strong><small>{session.chapters.length}</small></header>
        {#if session.chapters.length}
          <div class="chapter-list">
            {#each session.chapters as chapter, index (`${chapter.start_ms}-${index}`)}
              <button
                class:active={index === activeChapterIndex}
                type="button"
                aria-current={index === activeChapterIndex ? "true" : undefined}
                onclick={() => seekToChapter(chapter.start_ms)}
              >
                <span><small>{index + 1}</small><strong>{chapter.title || `Chapter ${index + 1}`}</strong></span>
                <time>{formatTime(Math.max(0, chapter.end_ms - chapter.start_ms) / 1000)}</time>
              </button>
            {/each}
          </div>
        {:else}
          <p>No chapter information is embedded in this audiobook.</p>
        {/if}
      </section>
      <label class="speed-control">Playback speed
        <select bind:value={playbackRate} onchange={applyPlaybackRate}>
          {#each [0.75, 1, 1.25, 1.5, 1.75, 2] as rate}<option value={rate}>{rate}×</option>{/each}
        </select>
      </label>
    </section>
  </div>
{/if}

<section class:above-navigation={aboveNavigation} class:expanded class:tap-expandable={useNativeAudio} class="audiobook-player" aria-label="Audiobook player">
  {#if showBatteryNudge}
    <aside class="battery-nudge" aria-label="Background playback setting">
      <span><strong>Keep listening in the background</strong><small>Android may restrict playback to save battery. You can allow unrestricted battery use in system settings.</small></span>
      <div>
        <button type="button" onclick={reviewBatterySetting}>Review setting</button>
        <button type="button" onclick={snoozeBatteryNudge}>Not now</button>
      </div>
    </aside>
  {/if}
  <div class="dock">
    {#if useNativeAudio}<button class="dock-expander" type="button" aria-label="Show more audiobook controls" onclick={openExpandedFromDock}></button>{/if}
    <div class="cover" aria-hidden="true">
      {#if session.coverUrl}<img src={session.coverUrl} alt="" />{:else}<span>{session.title}</span>{/if}
    </div>
    <div class="identity"><strong>{session.title}</strong><span>{session.author || "Audiobook"}</span></div>
    <div class="transport">
      <button type="button" aria-label="Back 15 seconds" onclick={() => seekBy(-15)}><RotateCcw size={20} /></button>
      <button class="play" type="button" aria-label={playing ? "Pause" : "Play"} onclick={togglePlayback}>{#if playing}<Pause size={23} />{:else}<Play size={23} />{/if}</button>
      <button type="button" aria-label="Forward 30 seconds" onclick={() => seekBy(30)}><RotateCw size={20} /></button>
    </div>
    <div class="timeline">
      <input aria-label="Audiobook position" type="range" min="0" max="1000" value={Math.round(progress * 1000)} oninput={(event) => seekToFraction(Number(event.currentTarget.value) / 1000)} />
      <div><span>{formatTime(currentTime)}</span><span>{formatTime(duration)}</span></div>
    </div>
    <button class="expand" type="button" aria-label={expanded ? "Collapse audiobook player" : "Expand audiobook player"} onclick={() => (expanded = !expanded)}>{#if expanded}<ChevronDown size={20} />{:else}<ChevronUp size={20} />{/if}</button>
    <button class="close" type="button" aria-label="Close audiobook player" onclick={closePlayer}><X size={20} /></button>
  </div>
</section>

<style>
  audio { display:none; }
  .audiobook-player { position:fixed; z-index:28; right:0; bottom:0; left:0; border-top:1px solid var(--color-border); color:var(--color-ink-text); background:var(--color-surface-bg); box-shadow:0 -.5rem 1.5rem color-mix(in srgb,var(--color-box-shadow) 24%,transparent); font-family:var(--font-interface-family); }
  .audiobook-player.above-navigation { bottom:calc(4.45rem + env(safe-area-inset-bottom)); }
  .audiobook-player.tap-expandable .dock { cursor:pointer; }
  .expanded-overlay { position:fixed; z-index:40; inset:0; display:grid; place-items:end stretch; color:var(--color-ink-text); font-family:var(--font-interface-family); }
  .overlay-scrim { position:absolute; inset:0; width:100%; height:100%; border:0; background:var(--color-overlay-scrim); }
  .expanded-sheet { position:relative; display:grid; gap:1.15rem; max-height:calc(100dvh - env(safe-area-inset-top) - 1rem); overflow:auto; padding:0 max(1.25rem,env(safe-area-inset-right)) calc(1.25rem + env(safe-area-inset-bottom)) max(1.25rem,env(safe-area-inset-left)); border-top:1px solid var(--color-border); background:var(--color-surface-bg); box-shadow:0 -1rem 2.5rem color-mix(in srgb,var(--color-box-shadow) 32%,transparent); }
  .expanded-sheet header { display:flex; min-height:58px; align-items:center; justify-content:space-between; border-bottom:1px solid var(--color-border); }
  .expanded-sheet header > strong { font-size:.78rem; text-transform:uppercase; letter-spacing:.08em; }
  .expanded-sheet header > div { display:flex; }
  .expanded-book { display:grid; grid-template-columns:82px minmax(0,1fr); align-items:center; gap:1rem; }
  .expanded-cover { display:flex; width:82px; height:110px; align-items:flex-end; overflow:hidden; padding:.3rem; box-sizing:border-box; border:1px solid var(--color-border); background:var(--color-raised-bg); }
  .expanded-cover img { width:100%; height:100%; object-fit:cover; }
  .expanded-cover span { display:-webkit-box; overflow:hidden; font-size:.7rem; font-weight:650; line-height:1.1; line-clamp:5; -webkit-box-orient:vertical; -webkit-line-clamp:5; }
  .expanded-copy { min-width:0; display:grid; gap:.3rem; }
  .expanded-copy strong { overflow:hidden; font-size:1.15rem; line-height:1.25; text-overflow:ellipsis; }
  .expanded-copy span { overflow:hidden; color:var(--color-subtle-text); font-size:.82rem; text-overflow:ellipsis; white-space:nowrap; }
  .expanded-copy small { color:var(--color-danger-text); }
  .expanded-timeline { display:grid; gap:.2rem; }
  .expanded-timeline input { width:100%; min-height:32px; margin:0; accent-color:var(--color-accent); }
  .expanded-timeline div { display:flex; justify-content:space-between; color:var(--color-subtle-text); font-size:.7rem; font-variant-numeric:tabular-nums; }
  .expanded-transport { display:flex; align-items:center; justify-content:center; gap:1.25rem; }
  .expanded-transport button { position:relative; width:56px; height:56px; }
  .expanded-transport button.play { width:68px; height:68px; }
  .expanded-transport button small { position:absolute; font-size:.56rem; font-weight:750; }
  .chapter-browser { min-height:0; border:1px solid var(--color-border); background:var(--color-raised-bg); }
  .chapter-browser > header { display:flex; min-height:42px; align-items:center; justify-content:space-between; padding:0 .75rem; border-bottom:1px solid var(--color-border); }
  .chapter-browser > header strong { font-size:.78rem; text-transform:none; letter-spacing:normal; }
  .chapter-browser > header small,.chapter-browser > p { color:var(--color-subtle-text); font-size:.7rem; }
  .chapter-browser > p { margin:0; padding:1rem .75rem; }
  .chapter-list { max-height:min(32dvh,260px); overflow:auto; }
  .chapter-list button { display:flex; width:100%; min-height:48px; align-items:center; justify-content:space-between; gap:1rem; padding:.45rem .75rem; border-bottom:1px solid var(--color-border); text-align:left; }
  .chapter-list button:last-child { border-bottom:0; }
  .chapter-list button.active { background:var(--color-active-highlight); }
  .chapter-list button > span { display:grid; grid-template-columns:2rem minmax(0,1fr); min-width:0; align-items:center; }
  .chapter-list button > span small { color:var(--color-subtle-text); font-size:.65rem; font-variant-numeric:tabular-nums; }
  .chapter-list button > span strong { overflow:hidden; font-size:.76rem; font-weight:650; text-overflow:ellipsis; white-space:nowrap; }
  .chapter-list time { flex:none; color:var(--color-subtle-text); font-size:.68rem; font-variant-numeric:tabular-nums; }
  .speed-control { display:grid; grid-template-columns:1fr minmax(92px,auto); align-items:center; gap:1rem; min-height:48px; border-top:1px solid var(--color-border); color:var(--color-subtle-text); font-size:.75rem; }
  .battery-nudge { display:grid; grid-template-columns:minmax(0,1fr) auto; align-items:center; gap:.75rem; padding:.7rem max(.75rem,env(safe-area-inset-right)) .7rem max(.75rem,env(safe-area-inset-left)); border-bottom:1px solid var(--color-border); background:var(--color-active-highlight); }
  .battery-nudge > span { display:grid; gap:.15rem; min-width:0; }
  .battery-nudge strong { font-size:.78rem; font-weight:650; }
  .battery-nudge small { color:var(--color-subtle-text); font-size:.68rem; line-height:1.35; }
  .battery-nudge > div { display:flex; gap:.25rem; }
  .battery-nudge button { width:auto; min-width:0; min-height:40px; padding:0 .6rem; border:1px solid var(--color-border); background:var(--color-raised-bg); font-size:.68rem; font-weight:650; white-space:nowrap; }
  .dock { position:relative; display:grid; grid-template-columns:42px minmax(0,1fr) auto auto auto; align-items:center; gap:.55rem; min-height:72px; padding:.38rem max(.55rem,env(safe-area-inset-right)) .38rem max(.55rem,env(safe-area-inset-left)); }
  .dock > :not(.dock-expander) { position:relative; z-index:1; }
  .dock > .cover,.dock > .identity { pointer-events:none; }
  button.dock-expander { position:absolute; z-index:0; inset:0; width:100%; height:100%; min-width:0; min-height:0; }
  .cover { display:flex; align-items:flex-end; width:42px; height:58px; overflow:hidden; padding:.22rem; border:1px solid var(--color-border); }
  .cover img { width:100%; height:100%; object-fit:cover; }
  .cover span { display:-webkit-box; overflow:hidden; font-size:.55rem; font-weight:600; line-height:1.05; line-clamp:4; -webkit-box-orient:vertical; -webkit-line-clamp:4; }
  .identity { min-width:0; display:grid; gap:.15rem; }
  .identity strong,.identity span { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .identity strong { font-size:.78rem; font-weight:600; } .identity span { color:var(--color-subtle-text); font-size:.66rem; }
  button { display:grid; place-items:center; min-width:44px; min-height:44px; padding:0; border:0; border-radius:0; color:inherit; background:transparent; }
  button.play { border:1px solid var(--color-accent); color:var(--color-surface-bg); background:var(--color-accent); }
  .transport { display:flex; align-items:center; gap:.1rem; } .timeline { display:none; min-width:8rem; }
  .timeline input { width:100%; accent-color:var(--color-accent); } .timeline div { display:flex; justify-content:space-between; color:var(--color-subtle-text); font-size:.62rem; }
  label { display:grid; gap:.25rem; color:var(--color-subtle-text); font-size:.68rem; } select { min-height:40px; border:1px solid var(--color-border); border-radius:0; color:var(--color-ink-text); background:var(--color-raised-bg); }
  @media (max-width:36rem) { .transport button:first-child,.transport button:last-child { display:none; } .expand { display:none; } .battery-nudge { grid-template-columns:1fr; } .battery-nudge > div { justify-content:flex-end; } }
  @media (min-width:45rem) { .audiobook-player,.audiobook-player.above-navigation { bottom:0; left:var(--layout-sidebar-width); } .dock { grid-template-columns:48px minmax(10rem,14rem) auto minmax(10rem,1fr) auto auto; min-height:88px; padding:.45rem 1rem; } .cover { width:48px; height:64px; } .timeline { display:block; } .expanded-sheet { width:min(540px,calc(100vw - var(--layout-sidebar-width) - 2rem)); margin-left:var(--layout-sidebar-width); justify-self:center; box-sizing:border-box; } }
</style>
