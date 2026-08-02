# html-layout

The production dependency graph is acyclic. The `html-pipeline` and
`html-style` dev-dependencies are intentional test-only cycles so layout tests
can keep using `DocumentFactory` without widening layout internals. Text is
shaped with deterministic test metrics, so testing this crate does not require
a windowing framework.
