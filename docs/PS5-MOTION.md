# PS5-inspired motion and appearance

nanoDNS Desktop keeps a dark, console-inspired interface with a quiet
procedural background rendered by the application.
Sony shader code, console artwork, and reference video frames are not included
in the application.

## References and interpretation

The original visual reference is Mystic's 2020
[PlayStation 5 UI Walkthrough (60FPS)](https://www.youtube.com/watch?v=SFzjeVNOo68),
viewed at its available 3840×2160 / 60fps setting. A newer settings reference is
Sony's 2025 [Audio Focus UI video](https://www.youtube.com/watch?v=2mcxoinz9zM),
alongside the current [Sony Audio Focus guide](https://www.playstation.com/en-us/support/hardware/audio-focus-ps5/).
The support page was checked on 2026-11-01. These references cover different
screens and software generations; one observed screen is not a specification
for every console screen.

The application coordinates the field's position, width, and depth through one
**26.3-second cycle**. Reference samples do not establish Sony's exact algorithm,
random motion, or an identical cycle. The study sampled the three-minute
4K/60fps passage; it did not compare every frame.

## Runtime behavior

Visible animation requests a repaint approximately every 33 ms. This scheduling
interval is not a measured frame-rate guarantee. The phase pauses while hidden
or minimized and when reduced motion is active; resuming preserves its position
without advancing through the time spent hidden. Windows' client-area animation
preference supplies the reduced-motion setting.

The PS5-inspired palette and soft background remain visible through hovered
controls. Hover fill uses an alpha value of **72/255**, preserving translucency.
Localized navigation adapts to label width. Visual acceptance must still check
legibility, clipping, focus states, DPI scaling, and the background in the native
Windows application.

Source clips, extracted reference frames, and private QA captures are not
redistributed. See [validation scope](VALIDATION.md) for accepted checks and their limits.
