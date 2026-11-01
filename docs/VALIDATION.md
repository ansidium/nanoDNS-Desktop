# Release validation

Version 0.2.1 targets Windows x64. Linux and macOS ports are planned.

| Check | Verified scope |
| --- | --- |
| Catalogs | 30 catalogs × 184 source keys; exact coverage, placeholders, line breaks and UTF-8/NFC. |
| Rust checks | Formatting, Clippy and all 54 tests passed, including DNS, activation, shutdown, motion pause/resume and translation regressions. |
| Release DNS | Six real UDP queries passed: A/ANY overrides, AAAA NODATA, exception forwarding, case preservation and port release. |
| Malay | Ten critical messages received model-based semantic review. Segoe UI and Light cover all 70 catalog characters. |
| Native Malay UI | Four tabs captured and inspected at 900×660 and 720×620 on a 2560×1440 Windows VM, 96 DPI. No clipping or missing glyphs found; normal exit passed. |
| CJK | Eight licensed regional font subsets; exact catalog glyph coverage and headless egui layout checked. |
| Packaging | Archive contents, checksums, required licenses and source correspondence are verified before publication. |

The native UI check covers Malay at the sizes listed above. It does not establish
native-language human proofreading, complete accessibility certification or
sustained frame-rate measurements. Raw QA captures and credentials are excluded
from distribution.
