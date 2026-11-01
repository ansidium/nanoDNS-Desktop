# nanoDNS local eframe patch

Base: eframe 0.33.3 from the Cargo registry, egui commit
`44cdd653e2317d300fb8a6c9c36b03f23991e803` (`crates/eframe`).
Upstream: https://github.com/emilk/egui/tree/44cdd653e2317d300fb8a6c9c36b03f23991e803/crates/eframe

Modified 2026-10-29 for nanoDNS Desktop 0.1.7. Only `src/native/run.rs`
differs from upstream Rust sources. No public APIs, dependency versions or
other platforms' event loop behavior were changed.

On Windows, consuming the last repaint deadline selected `ControlFlow::Poll`.
A hidden window need not emit `RedrawRequested`, so the event loop could remain
in Poll indefinitely and use one CPU core. When no repaint deadlines remain,
the local patch explicitly selects `ControlFlow::Wait` on Windows. Native paint,
input and activation messages still wake winit; future deadlines use WaitUntil.

The regression check measures the actual EXE after minimize/reopen/close-to-tray
and exercises native menu commands, repeated launches and graceful exit. The complete modified source is included in this directory.

The upstream MIT and Apache-2.0 licenses are retained in this directory. They
were obtained from the exact commit above; the original crate archive refers to
licenses in its parent repository.
