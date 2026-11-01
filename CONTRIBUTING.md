# Contributing

The supported target is currently Windows with the MSVC Rust toolchain and the
Windows SDK. Use Rust 1.99.1, matching `build.ps1` and CI, keep the application
lockfile, and preserve the vendored eframe patch.
Linux and macOS work is planned in [docs/PORTABILITY.md](docs/PORTABILITY.md).

## Local checks

From the repository root in PowerShell:

```powershell
python scripts/check-locales.py
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
python verify-release.py target/release/nanodns-desktop.exe
```

The UDP smoke check runs the built executable with a temporary profile and a
local test resolver. Native GUI acceptance is separate: exercise window/tray
behavior, keyboard focus, DPI scaling, long labels, CJK rendering, translucent
hover states, and visible/hidden/reduced-motion behavior. See
[docs/VALIDATION.md](docs/VALIDATION.md) for the accepted checks and their limits.

## Translations and fonts

All 30 catalogs share 184 English source keys. Follow
[locales/README.md](locales/README.md), preserve placeholder order and line
breaks, and update every catalog when adding a key. IP addresses, domains,
paths, network names, and unknown user values must stay verbatim. A semantic
review of warnings and network instructions is needed in addition to structural
validation. Report model review and native-language human review separately.

CJK fallbacks use the selected region's Regular and Light fonts, subset from
pinned official Noto sources. Keep FontTools and full downloads inside the
ignored local directory; do not install fonts or tooling system-wide:

```powershell
python -m venv .font-build/venv
.\.font-build\venv\Scripts\python.exe -m pip install fonttools==4.68.0
.\.font-build\venv\Scripts\python.exe scripts/subset-cjk-fonts.py --check
```

If a CJK catalog changes its character repertoire, regenerate and check again:

```powershell
.\.font-build\venv\Scripts\python.exe scripts/subset-cjk-fonts.py
.\.font-build\venv\Scripts\python.exe scripts/subset-cjk-fonts.py --check
```

Commit the eight small subsets, their notices, and the updated
[font provenance](docs/font-provenance.json). Retain the complete OFL license and
upstream copyright/trademark metadata. Full source fonts and `.font-build/`
do not belong in commits or release archives. Re-run native CJK layout checks
after translation or font changes.

## Appearance and evidence

Preserve the PS5-inspired dark interface, coherent background motion, and
translucent hover fill with alpha 72/255. Explain visual changes with application
screenshots and measured behavior. Keep source clips and private QA recordings
out of commits. Reference the original material by link, and distinguish the
application's procedural model from observations about Sony's implementation;
see [docs/PS5-MOTION.md](docs/PS5-MOTION.md).
