This is the upstream sherpa-onnx-sys 1.13.8 crate, with its Apache-2.0 license.

The `runtime-tts` feature resolves the six offline TTS functions with libloading
instead of linking native libraries at build time. It is enabled only for
Bokheim's Android reader. Desktop builds retain upstream static linking.
Other Sherpa API families are not supported by this feature.

The host must package the matching Sherpa and ONNX Runtime shared libraries.
`initialize_tts()` reports loading/symbol errors before model creation. The
library handle stays alive for the process lifetime, including model/audio
destruction and voice switching. Failed loads can be retried. Successful loads
are shared across synthesis sessions.

Loader error tests can run without installing an inference runtime:
`cargo test --manifest-path vendor/sherpa-onnx-sys/Cargo.toml --no-default-features --features runtime-tts --lib`.
