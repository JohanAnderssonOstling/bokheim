let active = null;

// Browser audio owns the clock; UI repaint timers never drive playback.
export class BrowserAudio {
    constructor(url, artwork, metadata, initial, save, direct = null, renew = null) {
        if (active) active.dispose();
        active = this;
        this.audio = new Audio();
        this.direct = direct;
        this.renew = renew;
        this.wantsPlay = false;
        this.playGeneration = 0;
        this.recovery = null;
        this.recoveryAttempts = 0;
        this.validUntil = direct ? Date.now() + direct.expires_in_seconds * 1000 : Infinity;
        if (direct) this.audio.crossOrigin = 'anonymous';
        // The source owner retains the file or remote range session.
        this.url = url;
        this.audio.src = this.url;
        this.artworkUrl = artwork.length ? URL.createObjectURL(new Blob([new Uint8Array(artwork)])) : null;
        if (this.artworkUrl) metadata.artwork = [{src: this.artworkUrl}];
        this.audio.preload = 'metadata';
        this.initial = initial;
        this.lastConfirmed = initial;
        this.seekPending = false;
        this.waiting = false;
        this.ready = false;
        this.disposed = false;
        this.error = null;
        this.lastSaved = null;
        this.save = save;
        this.handlers = [];
        this.media = globalThis.navigator?.mediaSession;
        this.listen('loadedmetadata', () => {
            if (this.disposed) return;
            this.seekPending = true;
            this.audio.currentTime = this.clamp(this.initial);
            this.ready = true;
            // A no-op seek need not produce a seeked event.
            if (!this.audio.seeking) { this.seekPending = false; this.update(false); }
        });
        this.listen('waiting', () => { this.waiting = true; });
        this.listen('pause', () => {
            if (!this.recovery) this.wantsPlay = false;
        });
        this.listen('playing', () => { this.waiting = false; this.recoveryAttempts = 0; });
        this.listen('seeked', () => {
            if (this.audio.seeking) return;
            this.seekPending = false;
            this.waiting = false;
            this.update(true);
        });
        for (const event of ['timeupdate', 'playing', 'ratechange']) this.listen(event, () => this.update(false));
        for (const event of ['pause', 'ended']) this.listen(event, () => this.update(true));
        this.listen('error', () => {
            if (this.direct && this.wantsPlay && !this.disposed && this.recoveryAttempts < 8) {
                this.recoverDirect();
                return;
            }
            this.error = `Could not play this audiobook (media error ${this.audio.error?.code ?? 'unknown'}).`;
            this.update(true);
        });
        this.visibilitychange = () => this.update(true);
        this.pagehide = () => {
            this.update(true);
            if (this.direct) {
                this.pause();
                this.revokeDirect(this.url);
                // A back/forward-cache restore keeps this player alive. Its
                // next explicit Play must mint a new capability.
                this.validUntil = 0;
            }
        };
        globalThis.addEventListener('pagehide', this.pagehide);
        globalThis.document?.addEventListener('visibilitychange', this.visibilitychange);
        if (this.media) {
            if (globalThis.MediaMetadata) this.media.metadata = new MediaMetadata(metadata);
            const actions = {
                play: () => this.play(), pause: () => this.pause(),
                seekbackward: () => this.seek(this.position() - 15),
                seekforward: () => this.seek(this.position() + 30),
                seekto: details => this.seek(details.seekTime),
            };
            for (const [action, handler] of Object.entries(actions)) {
                try { this.media.setActionHandler(action, handler); this.handlers.push(action); }
                catch { /* Individual actions vary by browser. */ }
            }
        }
    }
    listen(event, handler) { this.audio.addEventListener(event, handler); }
    clamp(position) {
        return Math.max(0, Math.min(Number.isFinite(this.audio.duration) ? this.audio.duration : Infinity, position));
    }
    setVolume(volume) { this.audio.volume = volume; }
    position() { return this.waiting ? this.lastConfirmed : this.ready ? this.audio.currentTime : this.initial; }
    confirmedPosition() {
        if (!this.ready || this.seekPending || this.audio.seeking) return undefined;
        const position = this.waiting ? this.lastConfirmed : this.audio.currentTime;
        return Number.isFinite(position) && position >= 0 ? position : undefined;
    }
    lastConfirmedPosition() { return this.lastConfirmed; }
    buffering() { return !this.disposed && this.waiting && this.playing(); }
    playing() { return !!(this.recovery && this.wantsPlay) || (!this.audio.paused && !this.audio.ended); }
    rate() { return this.audio.playbackRate; }
    play() {
        if (this.disposed) return;
        this.wantsPlay = true;
        if (this.direct && (this.audio.error || Date.now() >= this.validUntil)) {
            this.recoveryAttempts = 0;
            this.recoverDirect();
            return;
        }
        this.startMedia();
    }
    startMedia() {
        const generation = ++this.playGeneration;
        if (this.audio.ended) this.seek(0);
        this.error = null;
        const result = this.audio.play();
        if (result) result.catch(error => {
            if (this.disposed || generation !== this.playGeneration || error.name === 'AbortError') return;
            if (this.direct && this.wantsPlay && error.name !== 'NotAllowedError' && this.recoveryAttempts < 8) {
                this.recoverDirect();
                return;
            }
            this.wantsPlay = false;
            this.error = error.name === 'NotAllowedError'
                ? 'Tap Play to allow audiobook playback in this browser.' : `Could not start playback: ${error.message}`;
        });
    }
    revokeDirect(url) {
        if (this.direct && url) globalThis.fetch(url, {method: 'DELETE', credentials: 'omit', keepalive: true}).catch(() => {});
    }
    cancelRecovery() {
        if (this.recovery) { this.recovery.abort(); this.recovery = null; }
    }
    async recoverDirect() {
        if (this.recovery || this.disposed || !this.wantsPlay) return;
        const recovery = new AbortController();
        this.recovery = recovery;
        ++this.playGeneration;
        this.waiting = true;
        this.audio.pause();
        const position = this.seekPending ? this.initial : this.lastConfirmed;
        try {
            while (!recovery.signal.aborted && this.recoveryAttempts < 8) {
                const delay = Math.min(500 * 2 ** this.recoveryAttempts++, 8000);
                await new Promise(resolve => {
                    const done = () => { clearTimeout(timer); recovery.signal.removeEventListener('abort', done); resolve(); };
                    const timer = setTimeout(done, delay);
                    recovery.signal.addEventListener('abort', done, {once: true});
                });
                if (recovery.signal.aborted) return;
                let next;
                try { next = await this.renew(); }
                catch { continue; }
                if (this.disposed || recovery.signal.aborted || !this.wantsPlay) { this.revokeDirect(next.url); return; }
                if (next.checksum !== this.direct.checksum) { this.revokeDirect(next.url); throw Error('Audiobook revision changed; reopen it.'); }
                const previous = this.url;
                this.url = next.url;
                this.direct = next;
                this.validUntil = Date.now() + next.expires_in_seconds * 1000;
                this.initial = position;
                this.ready = false;
                this.seekPending = true;
                this.audio.src = this.url;
                this.audio.load();
                this.recovery = null;
                this.revokeDirect(previous);
                this.startMedia();
                return;
            }
            if (!recovery.signal.aborted) throw Error('Playback connection was lost. Tap Play to retry.');
        } catch (error) {
            if (!recovery.signal.aborted && !this.disposed) { this.wantsPlay = false; this.error = error.message; this.waiting = false; }
        } finally { if (this.recovery === recovery) this.recovery = null; }
    }
    toggle() { if (this.playing()) this.pause(); else this.play(); }
    pause() { if (!this.disposed) { ++this.playGeneration; this.wantsPlay = false; this.cancelRecovery(); this.audio.pause(); } }
    seek(position) {
        if (!Number.isFinite(position) || this.disposed) return;
        this.initial = this.clamp(position);
        if (this.direct && (this.recovery || Date.now() >= this.validUntil)) {
            this.cancelRecovery();
            this.seekPending = true;
            if (this.wantsPlay) this.recoverDirect();
            return;
        }
        if (this.ready) {
            this.seekPending = true;
            this.audio.currentTime = this.initial;
            if (!this.audio.seeking) { this.seekPending = false; this.update(true); }
        }
    }
    setRate(rate) { this.audio.defaultPlaybackRate = rate; this.audio.playbackRate = rate; this.update(false); }
    takeError() { const error = this.error; this.error = null; return error; }
    failed() { return !this.recovery && this.audio.error != null; }
    update(force) {
        if (this.disposed || !this.ready) return;
        const position = this.confirmedPosition();
        if (position === undefined) return;
        this.lastConfirmed = position;
        const duration = this.audio.duration;
        if (force || this.lastSaved === null || Math.abs(position - this.lastSaved) >= 5) {
            this.lastSaved = position;
            this.save(position, Number.isFinite(duration) && duration > 0 ? position / duration : 0);
        }
        if (active === this && this.media) {
            this.media.playbackState = this.playing() ? 'playing' : 'paused';
            if (this.media.setPositionState && Number.isFinite(duration) && duration > 0) {
                try { this.media.setPositionState({duration, playbackRate: this.rate(), position: this.clamp(position)}); }
                catch { /* Playback continues if the browser rejects position reporting. */ }
            }
        }
    }
    dispose() {
        if (this.disposed) return;
        this.update(true);
        this.disposed = true;
        this.wantsPlay = false;
        this.cancelRecovery();
        this.revokeDirect(this.url);
        this.audio.pause();
        this.audio.removeAttribute('src');
        this.audio.load();
        if (this.artworkUrl) URL.revokeObjectURL(this.artworkUrl);
        globalThis.removeEventListener('pagehide', this.pagehide);
        globalThis.document?.removeEventListener('visibilitychange', this.visibilitychange);
        if (active === this) {
            if (this.media) {
                for (const action of this.handlers) this.media.setActionHandler(action, null);
                this.media.metadata = null;
                this.media.playbackState = 'none';
                if (this.media.setPositionState) this.media.setPositionState();
            }
            active = null;
        }
    }
}
