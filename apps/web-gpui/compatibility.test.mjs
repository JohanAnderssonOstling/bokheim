import assert from 'node:assert/strict';
import test from 'node:test';
import {requiredCapabilities} from './compatibility.mjs';

function supportedEnvironment(wasm) {
    return {
        navigator: {storage: {getDirectory() {}}, locks: {request() {}}},
        isSecureContext: true,
        crossOriginIsolated: true,
        SharedArrayBuffer() {},
        Atomics: {waitAsync() {}},
        Worker() {},
        SharedWorker() {},
        WebAssembly: wasm,
    };
}

test('browsers with only legacy exception handling receive an update message', () => {
    const issues = requiredCapabilities(supportedEnvironment({JSTag: {}, validate: () => false}));
    assert.equal(issues.length, 1);
    assert.match(issues[0], /exception handling.*Update your browser/);
});

test('modern exception instructions require the JavaScript exception tag too', () => {
    const issues = requiredCapabilities(supportedEnvironment({validate: () => true}));
    assert.equal(issues.length, 1);
    assert.match(issues[0], /exception handling/);
});

test('supported exception handling passes the capability gate', () => {
    assert.deepEqual(requiredCapabilities(supportedEnvironment({JSTag: {}, validate: () => true})), []);
});

test('a restricted WebAssembly validator reports incompatibility without throwing', () => {
    const issues = requiredCapabilities(supportedEnvironment({JSTag: {}, validate() { throw new Error('blocked'); }}));
    assert.equal(issues.length, 1);
    assert.match(issues[0], /exception handling/);
});
