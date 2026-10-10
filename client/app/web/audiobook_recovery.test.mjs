import {test} from 'node:test';
import assert from 'node:assert/strict';
import {setImmediate} from 'node:timers/promises';
import {AudioRangeRecovery} from './audiobook_source.js';

const temporary = () => Object.assign(Error('offline'), {retryable: true});
const signal = () => new AbortController().signal;


test('exhaustion is latched across later range requests', async () => {
    let reads = 0;
    const recovery = new AudioRangeRecovery(async () => { reads++; throw temporary(); }, async () => {});
    await assert.rejects(recovery.range(0, 2, signal()), /offline/);
    await assert.rejects(recovery.range(0, 2, signal()), /offline/);
    assert.equal(reads, 8);
});


test('cancelled in-flight result cannot poison or satisfy a later request', async () => {
    let fail;
    const recovery = new AudioRangeRecovery(() => new Promise((_, reject) => { fail = reject; }));
    const cancelled = new AbortController();
    const result = recovery.range(0, 1, cancelled.signal);
    const rejected = assert.rejects(result, /cancelled/);
    cancelled.abort();
    await rejected;
    fail(Error('late old failure'));
    await setImmediate();
    assert.equal(recovery.failure, null);
    recovery.read = async () => new Uint8Array([7]);
    assert.deepEqual(await recovery.range(10, 1, signal()), new Uint8Array([7]));
});

test('short response is retried rather than accepted as EOF', async () => {
    let reads = 0;
    const recovery = new AudioRangeRecovery(async () => new Uint8Array(++reads === 1 ? 0 : 2), async () => {});
    assert.equal((await recovery.range(0, 2, signal())).length, 2);
    assert.equal(reads, 2);
});

test('closing during backoff clears its live timer without another read', async t => {
    const timers = new Set();
    t.mock.method(globalThis, 'setTimeout', (_, ms) => {
        const timer = {ms};
        timers.add(timer);
        return timer;
    });
    t.mock.method(globalThis, 'clearTimeout', timer => timers.delete(timer));
    let reads = 0;
    const recovery = new AudioRangeRecovery(async () => { reads++; throw temporary(); });
    const result = recovery.range(0, 1, signal());
    const rejected = assert.rejects(result, /cancelled/);
    await setImmediate();
    assert.equal(timers.size, 1);
    assert.equal([...timers][0].ms, 500);
    recovery.close();
    await rejected;
    assert.equal(timers.size, 0);
    assert.equal(reads, 1);
});

test('request cancellation during backoff leaves the source usable', async t => {
    const timers = new Set();
    t.mock.method(globalThis, 'setTimeout', () => { const timer = {}; timers.add(timer); return timer; });
    t.mock.method(globalThis, 'clearTimeout', timer => timers.delete(timer));
    const recovery = new AudioRangeRecovery(async () => { throw temporary(); });
    const request = new AbortController();
    const result = recovery.range(0, 1, request.signal);
    const rejected = assert.rejects(result, /cancelled/);
    await setImmediate();
    request.abort();
    await rejected;
    assert.equal(timers.size, 0);
    assert.equal(recovery.failure, null);
    recovery.read = async () => new Uint8Array([3]);
    assert.deepEqual(await recovery.range(10, 1, signal()), new Uint8Array([3]));
});

test('synchronous close during read observes a later rejection', async () => {
    const recovery = new AudioRangeRecovery(() => {
        recovery.close();
        return Promise.reject(Error('read failed after close'));
    });
    await assert.rejects(recovery.range(0, 1, signal()), /cancelled/);
    await setImmediate(); // Node's test runner also detects unhandled rejections.
    assert.equal(recovery.failure, null);
});
