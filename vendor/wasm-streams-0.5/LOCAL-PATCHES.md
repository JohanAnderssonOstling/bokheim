# Local wasm-streams 0.5 patch

Upstream: crates.io wasm-streams 0.5.0, MIT OR Apache-2.0.
Archive SHA-256: 9d1ec4f6517c9e11ae630e200b2b65d193279042e28edd4a2cda233e46670bbb.

Reqwest 0.13 selects wasm-streams 0.5, while the existing 0.4 override serves
older callers. Both versions must retain the application's WebAssembly unwind
boundary policy. Upstream 0.5 guards its future bodies but leaves exported
adapter state without the unwind-safety traits required by wasm-bindgen 0.2.129.

Reuse the existing wasm-callback AbortOnPanic state and future guards for the
three exported readable/byte-readable/writable adapters. A panic at those
boundaries terminates the instance before partially changed user state can
be resumed; ordinary stream errors and cancellation retain upstream behavior.
The operation implementations and sink error handling remain from upstream 0.5.
No blanket unwind-safety assertion is added to arbitrary user streams or sinks.

Remove this override when Reqwest can use a compatible upstream stream adapter.
