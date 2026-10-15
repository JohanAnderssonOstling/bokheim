'use strict';
const grid = document.querySelector('#samples');
const status = document.querySelector('#status');
const speed = document.querySelector('#speed');
const accent = document.querySelector('#accent');
const audios = [];
const manifest = JSON.parse(document.querySelector('#sample-manifest').textContent);
const synth = window.speechSynthesis;
const voicePicker = document.querySelector('#browser-voice');
const browserPlay = document.querySelector('#browser-play');
const browserPause = document.querySelector('#browser-pause');
const browserStop = document.querySelector('#browser-stop');
const browserStatus = document.querySelector('#browser-status');
const browserCard = document.querySelector('.browser-card');
let availableVoices = [];
let currentUtterance = null;
let speechSession = 0;
let browserPaused = false;
let pendingTimer = null;
const time = seconds => `${Math.floor(seconds / 60)}:${String(Math.round(seconds % 60)).padStart(2, '0')}`;

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function card(sample, index) {
  const article = element('article', 'voice-card');
  const top = element('div', 'card-top');
  top.append(element('span', `engine ${sample.engine.toLowerCase()}`, sample.engine.toUpperCase()), element('span', 'duration', time(sample.duration)));
  const title = element('h3', '', sample.name);
  title.id = `${sample.id}-title`;
  article.setAttribute('aria-labelledby', title.id);
  article.append(top, title, element('p', 'subtitle', sample.description));
  const ns = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(ns, 'svg');
  svg.classList.add('waveform');
  svg.setAttribute('viewBox', '0 0 432 56');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('preserveAspectRatio', 'none');
  sample.peaks.forEach((peak, i) => {
    const bar = document.createElementNS(ns, 'rect');
    const height = Math.max(3, peak * 50);
    for (const [key, value] of Object.entries({ x: i * 6, y: (56 - height) / 2, width: 3, height, rx: 1.5, fill: 'currentColor' })) bar.setAttribute(key, value);
    svg.append(bar);
  });
  const audio = element('audio');
  audio.controls = true;
  audio.preload = 'metadata';
  audio.src = sample.file;
  audio.playbackRate = Number(speed.value);
  audio.setAttribute('aria-label', `Play ${sample.engine} ${sample.name} English sample`);
  const bottom = element('div', 'card-bottom');
  const state = element('span', 'state', 'Ready to play');
  const download = element('a', '', 'Download MP3 ↗');
  download.href = sample.file;
  download.download = `${sample.id}.mp3`;
  bottom.append(state, download);
  article.append(svg, audio, bottom);
  audios.push(audio);
  audio.addEventListener('play', () => {
    stopBrowser(false);
    audios.forEach(other => { if (other !== audio) other.pause(); });
    article.classList.add('active');
    state.textContent = 'Playing';
    status.textContent = `Listening to ${sample.engine} ${sample.name} at ${speed.value}×.`;
  });
  audio.addEventListener('pause', () => { article.classList.remove('active'); state.textContent = audio.currentTime ? 'Paused' : 'Ready to play'; });
  audio.addEventListener('ended', () => { article.classList.remove('active'); state.textContent = 'Finished'; status.textContent = `${sample.engine} ${sample.name} finished. Try another voice.`; });
  audio.addEventListener('error', () => { state.textContent = 'Could not load audio'; status.textContent = 'Audio could not load. Reload the page or try the MP3 download.'; });
  audio.addEventListener('loadedmetadata', () => { top.querySelector('.duration').textContent = time(audio.duration); });
  return article;
}

speed.addEventListener('change', () => {
  audios.forEach(audio => { audio.playbackRate = Number(speed.value); });
  if (currentUtterance) {
    stopBrowser();
    browserStatus.textContent = 'Rate changed. Select Read passage to start at the new rate.';
  }
  status.textContent = `Playback speed set to ${speed.value}×. Browser voices use this value as their speaking rate.`;
});
document.querySelector('#reset').addEventListener('click', () => {
  stopBrowser();
  audios.forEach(audio => { audio.pause(); audio.currentTime = 0; });
  document.querySelectorAll('.voice-card').forEach(article => article.classList.remove('active'));
  document.querySelectorAll('.state').forEach(state => { state.textContent = 'Ready to play'; });
  status.textContent = 'All samples are back at the beginning.';
});

function load() {
  try {
    if (!manifest.samples.length) throw new Error('Sample manifest incomplete');
    renderSamples();
    grid.setAttribute('aria-busy', 'false');
    document.querySelector('#passage').textContent = manifest.text;
  } catch (error) {
    grid.replaceChildren(element('p', 'loading', 'The recordings could not load. Please reload the page.'));
    grid.setAttribute('aria-busy', 'false');
    status.textContent = 'Sample loading failed.';
    console.error(error);
  }
}

function renderSamples() {
  audios.forEach(audio => audio.pause());
  audios.length = 0;
  const samples = manifest.samples.filter(sample => accent.value === 'all' || sample.lang === accent.value);
  grid.replaceChildren(...samples.map(card));
}

function normalizedLanguage(value) {
  return value.replaceAll('_', '-').toLowerCase().replace(/^en-uk\b/, 'en-gb');
}

function refreshVoices() {
  if (!synth || !window.SpeechSynthesisUtterance) {
    browserStatus.textContent = 'Speech synthesis is unavailable in this browser. The saved samples still work.';
    return;
  }
  const oldVoice = availableVoices[Number(voicePicker.value)];
  const requestedAccent = accent.value.toLowerCase();
  availableVoices = synth.getVoices().filter(voice => {
    const language = normalizedLanguage(voice.lang);
    return voice.localService && /^en(?:-|$)/.test(language) && (requestedAccent === 'all' || language === requestedAccent || (requestedAccent !== 'en' && language.startsWith(`${requestedAccent}-`)));
  });
  availableVoices.sort((a, b) => a.name.localeCompare(b.name));
  voicePicker.replaceChildren();
  availableVoices.forEach((voice, i) => {
    const option = element('option', '', `${voice.name} · ${voice.lang}`);
    option.value = String(i);
    voicePicker.append(option);
  });
  if (oldVoice) {
    const index = availableVoices.findIndex(voice => voice.voiceURI === oldVoice.voiceURI && voice.name === oldVoice.name && voice.lang === oldVoice.lang);
    if (index >= 0) voicePicker.value = String(index);
  }
  voicePicker.disabled = !availableVoices.length;
  browserPlay.disabled = !availableVoices.length;
  document.querySelector('#browser-service').textContent = `${availableVoices.length} local ${availableVoices.length === 1 ? 'voice' : 'voices'}`;
  if (!availableVoices.length) {
    const label = accent.value === 'en-GB' ? 'British English' : accent.value === 'en-US' ? 'American English' : 'English';
    voicePicker.append(element('option', '', `No local ${label} voices available`));
    browserStatus.textContent = `No local ${label} voices are available in this browser. Try All English to see other installed voices. Firefox on Linux may need Speech Dispatcher and a configured speech engine.`;
  } else if (!currentUtterance) {
    browserStatus.textContent = 'Ready. Choose a voice and select Read passage.';
  }
}

function stopBrowser(showStatus = true) {
  const wasActive = currentUtterance !== null;
  speechSession += 1;
  currentUtterance = null;
  browserPaused = false;
  clearTimeout(pendingTimer);
  if (wasActive && synth) synth.cancel();
  browserCard.classList.remove('active');
  browserPause.textContent = 'Pause';
  browserPause.disabled = true;
  browserStop.disabled = true;
  if (wasActive && showStatus) browserStatus.textContent = 'Stopped. Select Read passage to start from the beginning.';
}

browserPlay.addEventListener('click', () => {
  const voice = availableVoices[Number(voicePicker.value)];
  if (!voice || !synth) return;
  stopBrowser(false);
  audios.forEach(audio => audio.pause());
  const session = speechSession;
  const utterance = new SpeechSynthesisUtterance(manifest.text);
  utterance.voice = voice;
  utterance.lang = voice.lang;
  utterance.rate = Number(speed.value);
  currentUtterance = utterance;
  const requestedAt = performance.now();
  let startDelay = null;
  browserStop.disabled = false;
  browserStatus.textContent = `Starting ${voice.name}…`;
  pendingTimer = setTimeout(() => {
    if (session === speechSession && startDelay === null) browserStatus.textContent = 'Waiting for the speech service to start. You can select Stop, then try another installed voice.';
  }, 10000);
  utterance.onstart = () => {
    if (session !== speechSession) return;
    clearTimeout(pendingTimer);
    startDelay = ((performance.now() - requestedAt) / 1000).toFixed(2);
    browserCard.classList.add('active');
    browserPause.disabled = false;
    browserStatus.textContent = `Speaking with ${voice.name} · Start delay ${startDelay} s · Rate ${speed.value}×.`;
  };
  utterance.onend = () => {
    if (session !== speechSession) return;
    stopBrowser(false);
    browserStatus.textContent = `Finished with ${voice.name}${startDelay === null ? '' : ` · Start delay ${startDelay} s`}.`;
  };
  utterance.onerror = event => {
    if (session !== speechSession) return;
    stopBrowser(false);
    browserStatus.textContent = `The speech service could not read the passage (${event.error || 'unknown error'}). Try another voice or check the system speech service.`;
  };
  synth.speak(utterance);
});

browserPause.addEventListener('click', () => {
  if (!currentUtterance) return;
  if (browserPaused) {
    synth.resume();
    browserPaused = false;
    browserPause.textContent = 'Pause';
    browserStatus.textContent = 'Browser reading resumed.';
  } else {
    synth.pause();
    browserPaused = true;
    browserPause.textContent = 'Resume';
    browserStatus.textContent = 'Browser reading paused.';
  }
});
browserStop.addEventListener('click', () => stopBrowser());
voicePicker.addEventListener('change', () => stopBrowser());
accent.addEventListener('change', () => {
  stopBrowser();
  renderSamples();
  refreshVoices();
  status.textContent = `${accent.selectedOptions[0].textContent} selected.`;
});
window.addEventListener('pagehide', () => stopBrowser(false));
if (synth) synth.addEventListener('voiceschanged', refreshVoices);
load();
refreshVoices();

function renderBenchmarks() {
  const data = JSON.parse(document.querySelector('#benchmark-manifest').textContent);
  const rows = data.samples.map(result => {
    const row = element('tr');
    const title = element('th', '', result.label);
    title.scope = 'row';
    row.append(title);
    [
      `${result.median_seconds.toFixed(2)} s`,
      `${result.audio_seconds.toFixed(1)} s`,
      `${result.times_realtime.toFixed(1)}×`,
      `${result.peak_process_ram_mib.toLocaleString('en')} MiB`,
      `${result.model_files_mb.toFixed(1)} MB`,
    ].forEach(value => row.append(element('td', '', value)));
    return row;
  });
  const browser = element('tr');
  const title = element('th', '', 'Browser voice');
  title.scope = 'row';
  browser.append(title);
  ['Live speech', 'Voice dependent', 'Voice dependent', 'Not measured', 'Provided by the system'].forEach(value => browser.append(element('td', '', value)));
  document.querySelector('#benchmark-rows').replaceChildren(...rows, browser);
}
renderBenchmarks();
