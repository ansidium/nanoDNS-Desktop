---
name: nanoDNS Desktop
description: PS5-inspired clarity for a native Windows DNS utility.
colors:
  ice: "#edf2f5"
  nav-text: "#ffffff"
  canvas: "#0e101d"
  inset: "#16191e"
  surface: "#262a30"
  muted: "#d4dce5"
  mint: "#98e3c2"
  line: "#373d46"
  control-line: "#8b96a5"
  input-line: "#677383"
  error: "#f0a5a5"
  error-surface: "#3a2327"
  hover: "#3a404a"
  active: "#4e5662"
  link: "#a8cbf9"
  selection: "#364152"
typography:
  headline:
    fontFamily: "Segoe UI Light, egui proportional fallback"
    fontSize: "26px"
    fontWeight: 300
  headline-compact:
    fontFamily: "Segoe UI Light, egui proportional fallback"
    fontSize: "24px"
    fontWeight: 300
  brand:
    fontFamily: "Segoe UI, egui proportional fallback"
    fontSize: "13px"
    fontWeight: 400
  body:
    fontFamily: "Segoe UI, egui proportional fallback"
    fontSize: "14px"
    fontWeight: 400
  label:
    fontFamily: "Segoe UI, egui proportional fallback"
    fontSize: "13px"
    fontWeight: 400
  helper:
    fontFamily: "Segoe UI, egui proportional fallback"
    fontSize: "12px"
    fontWeight: 400
  address:
    fontFamily: "egui monospace"
    fontSize: "28px"
  query:
    fontFamily: "egui monospace"
    fontSize: "13px"
rounded:
  control: "5px"
  nav-hit: "8px"
  action: "20px"
spacing:
  item-x: "12px"
  item-y: "8px"
  field-x: "8px"
  control-y: "6px"
  navigation-gap: "12px"
  section-gap: "20px"
  group-gap: "28px"
  column-gap: "28px"
  field-gap: "8px"
  page-inline: "32px"
components:
  button-primary:
    backgroundColor: "{colors.ice}"
    textColor: "{colors.inset}"
    typography: "{typography.body}"
    rounded: "{rounded.action}"
    padding: "6px 10px"
    width: "152px"
    height: "36px"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ice}"
    typography: "{typography.body}"
    rounded: "{rounded.action}"
    padding: "6px 10px"
  button-outlined:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ice}"
    border: "1px solid {colors.control-line}"
    typography: "{typography.body}"
    rounded: "{rounded.action}"
  input:
    backgroundColor: "{colors.inset}"
    textColor: "{colors.ice}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "6px 8px"
  navigation:
    backgroundColor: "transparent"
    textColor: "{colors.nav-text}"
    typography: "{typography.body}"
    rounded: "{rounded.nav-hit}"
    height: "44px"
  navigation-selected:
    backgroundColor: "rgba(0,0,0,0.051)"
    textColor: "{colors.nav-text}"
    rounded: "{rounded.nav-hit}"
    height: "44px"
    border: "1px solid rgba(255,255,255,0.686)"
  recovery-notice:
    backgroundColor: "{colors.error-surface}"
    textColor: "{colors.error}"
    rounded: "{rounded.control}"
    padding: "10px"
---

# Design System: nanoDNS Desktop

## Overview

**Visual direction: console clarity**

The graphite PS5 system-settings background is the visual reference: a broad light fan enters from beyond the upper-left edge, translucent diagonal folds overlap within it, a quiet reflected band sits near the bottom, and the right side remains dark. The opening is subtly warm silver and the diffused light approaches neutral graphite. Light headings and compact labeled navigation adapt that language to a Windows tool. The interface gives the actual address, service state and next action visual priority.

This is a native Rust/egui application. An integrated app title bar replaces the system decoration while labeled controls, tray behavior and system prompts remain part of its interaction model. The PlayStation UI overview and linked screen are visual references; Sony imagery is not bundled.

The token strings use portable `px` notation for egui logical points, not physical screenshot pixels. Extracted from `src/ui.rs`; the source remains the implementation authority.

## Colors

### Primary

Ice is the main foreground and the enabled primary action surface. Start and Save use dark inset text.

### Secondary

Mint communicates running service and successful feedback. Soft error text and a dark red notice surface identify failures. Link and selection colors serve native text interaction rather than decoration.

### Neutral

Canvas is the background; inset and surface distinguish editable areas and secondary controls. Muted text carries instructions and metadata; navigation text is white. The egui theme is pinned to Dark before the custom style is installed, so Windows light-theme events cannot replace native widget text, disclosure controls or input colors. Input-line marks field boundaries; control-line defines control outlines and toggle tracks. Broad panel separator lines are disabled. Hover and active tones belong to standard egui widgets.

Running, stopped, pending and failed states must be named in readable text; color supports that information.

## Typography

Headlines use Windows Segoe UI Light; the title-bar brand, body, labels and helper text use Segoe UI. If a font file cannot be read, use egui's packaged proportional fallback. Addresses, domain masks and query content use egui's monospace family. CJK locales append regional embedded nanoDNS UI Regular and Light subsets derived from Noto Sans CJK under OFL-1.1. Their exact catalog repertoires and reproducible source hashes are recorded in `docs/font-provenance.json`. All 30 locales use the same font families and scale. No runtime font download is required.

The address is the largest operational value. Line heights and letter spacing follow egui font metrics; this system defines no separate tracking or line-height tokens.

## Layout

The native inner window defaults to 900 by 660 logical points, with a minimum of 720 by 620. The application hides system decoration and draws a transparent 44-point title bar over the same continuous canvas, with native window controls. These dimensions describe a resizable desktop window, not browser breakpoints or mobile layouts.

The title bar holds the small network mark, app name, actual service state and three labeled window controls. A running loopback-only service is named "DNS работает локально"; running does not assert that a console has connected. Its empty left region drags the native window; double-click toggles maximize. Minimize, maximize/restore and close use native viewport commands, and close preserves the configured close-to-tray behavior. Six-point edge and corner hit regions start native resizing; title-bar controls stay inside them. The Windows 11 maximize-hover Snap flyout is not supplied by these custom controls.

The toolbar uses the 32-point page-inline margin and an 8-point vertical inset; central content uses the same inline margin and a 24-point vertical inset. All content and fixed actions share a maximum width of 880 points, clamped to the actual available width. Navigation combines each 32-point icon and 14-point label in a 44-point row; minimum widths are 154, 126, 142 and 126 points, separated by 12. Each grows to fit its translated label with 60 points reserved for the icon and padding. The page headline is 26 points, or 24 below 710 points of available content width. Shared spacing uses 28-point column/group gaps, 20-point section gaps and 8-point field gaps.

Connection allocates column bounds on a 55/45 split after the shared 28-point gutter. When only one interface is available, the address group can shrink to its content and the setup group follows with the same 28-point gutter. This compact arrangement was checked at both window sizes. Address and Copy have four-point local gaps; a full-column interface choice sits directly below them when needed, followed by Start/Stop. Long interface names truncate with their complete name in a tooltip. The right group contains a concise console setup instruction, optional detailed help, the bounded startup row and actual firewall state when relevant. Invalid console binding/port produces one recovery notice with a link to DNS settings; the loopback status is not repeated below startup. Spacing separates the two task groups without cards or full-width rules. Start/Stop remains in the left group; other pages keep the compact global action beside service status.

Rules uses equal-width domain and exception editors on the shared grid. Settings uses one form up to 720 logical points wide, reserving 12 points on the right for the floating scrollbar: listening address and upstreams are successive labeled rows, followed by collapsible advanced rows, application behavior, and files/version. The label column takes 36% of the form, clamped to 190–248 points; a 28-point gap separates controls. Main content scrolls when needed. Rules and Settings reserve a fixed transparent 60-point bottom region; Save/Cancel appears there only while edits exist. Startup applies immediately; form changes stay staged until Save.

Boolean settings use rows at least 40 points high, with a visible 14-point label and a right-aligned 36 by 20 toggle. Rows are bounded to their column, never wider than 720 points. The label wraps inside a text region that reserves 60 points for the toggle and gap, increasing row height when needed. A focusable egui button supplies interaction; metadata identifies the actual checkbox state. The hover surface is faint and focus remains visible. Rule syntax and detailed setup use named disclosures; UTC is named in the journal time column.

Journal columns span the content width with 12 points reserved for the scroll bar. Time, query type, and result use 72, 44, and 150 points; the domain takes the remaining width after three 16-point gaps. The header remains above a virtualized vertical list of 32-point rows. Long domain/result values truncate with full values in tooltips. Alternating rows use white at 6/255 opacity; hover uses 14/255. Error and expected override colors retain their meanings. At most the existing 300 in-memory events are retained.

## Elevation & Depth

The visual references are Sony's Audio Focus UI video, viewed at native 1920×1080 / 1080p60: https://www.youtube.com/watch?v=2mcxoinz9zM, and Mystic's original PS5 settings recording at https://www.youtube.com/watch?v=SFzjeVNOo68&t=1355s, with intrinsic video size 3840×2160 and the visible player quality 2160p60. The recordings are from 2025 and 2020 respectively. The reference study and its limits are documented in `docs/PS5-MOTION.md`.

Undimmed official Sony support stills in 1920×1080 establish the field's brightness and shape. A fitted analytic fan widens asymmetrically downward from a fixed upper-left source. Two soft valleys and a diagonal shoulder form overlapping folds; their widths and balance change slowly. A separate low reflection stays calmer. The analytic field is evaluated per fragment on the GPU through the existing eframe/glow context, with one sRGB encode and stationary, shared-RGB dithering below half an 8-bit step. The coarse 96 by 40 mesh remains only as a marked fallback if the shader renderer cannot initialize. Ninety-six subpixel silver grains have very low opacity and local drift. No Sony image is sampled at runtime.

The reference study includes a sparse clean cohort spanning 73.3 seconds, a longer upper-light sequence spanning 233.5 seconds with a midroll gap, and 60 consecutive browser frame steps spanning approximately one second. The long sequence is clipped outside its accepted upper-left region; it is not a full-frame 4K storyboard. Independent clean segments show a coherent repeat near 26.3 seconds. This does not establish randomness in the original micro-particles. nanoDNS uses its own deterministic continuous 26.3-second composition. The ambient repaint request remains 33 ms while the window is visible, including without focus; it is not a delivered FPS guarantee. Text and controls remain still. No textures, video, blur pass, new dependencies or animation worker are introduced. The clock and repaint requests pause while hidden, minimized, or when Windows client-area animation preferences disable motion. Losing focus alone does not stop a visible canvas. Resuming keeps the previous phase. Page changes share the same application-level clock. The title bar and fixed form-action region are transparent over this continuous canvas; Windows owns movement, sizing and compositor effects. Motion acceptance uses live native frames from the production clock. Assigned-phase screenshots alone are insufficient evidence of visible animation.

Navigation hover uses egui's short state transition across the entire 44-point button, with an 8-point corner radius and a graphite tint (68, 74, 86) at alpha up to 72/255. The shared light field remains visible through it. All navigation labels and icons use white to remain readable over the upper-left light, including inactive Connection on other pages. The selected tab has a translucent black 13/255 fill, rising to 22/255 on hover, and its existing one-point white outline at alpha 175/255. Keyboard focus has a stronger 1.5-point outline. Inactive hover adds no outline. The configured hover duration is 0.16 seconds; disabling Windows client-area animations makes it zero and disables ambient movement. Actions are not delayed by motion.
A small three-stroke resize grip sits seven points from the lower-right edges. Its 20-point corner hit area matches the diagonal resize cursor and native drag action. The grip and resize handling are suppressed for maximized/fullscreen windows; no label or extra panel is added.

## Shapes

Primary and secondary actions have rounded pill-like corners. Navigation selection surrounds the complete icon-and-label target with an eight-point corner radius. Input and recovery-notice corners are quieter and tighter. Functional input boundaries remain stronger than dividers.

The native window requests Windows 11's DWM rounded-corner preference. The compositor chooses the actual radius and can keep maximized, snapped or virtualized windows square. Windows 10 may decline the request; rounding is cosmetic and does not change the custom title bar or tray behavior.

**Selection and focus.** A thin inside outline identifies the complete selected navigation tab. Keyboard focus increases its weight and brightness. Action buttons use a separated 1.5-point light focus ring with a three-point gap; hover also changes the surface without changing its geometry.

## Components

### Actions

Copy, Start and Save use the ice surface with dark text when enabled. Copy leads the connection task while DNS is running. Stop uses the graphite surface with a persistent one-point control-line outline, including its compact title-bar instance on other tabs. Cancel keeps the secondary surface. Controls are real labeled egui buttons; scoped native widget visuals supply rest, hover, pressed and disabled states without replacing button semantics. Primary surfaces brighten to white on hover and dim to muted ice while pressed; graphite surfaces brighten using the existing hover and active tones. Stop's outline brightens to ice on hover and press. Focus remains a separate light ring. No extra motion or scaling is added.

Connection's Copy and Start/Stop are all 36 logical points high. Copy has a minimum width of 112 points; Start/Stop has a minimum width of 152 points. Other pages retain the compact 152 by 30 title-bar control. Save is 120 by 36 and Cancel is 104 by 36 logical points.

### Navigation

Connection, Rules, Settings and Journal combine a line icon with a visible name. Selection highlights the complete 44-point tab with a restrained fill and thin outline. The 32-point icon allocation remains unframed, using scale 0.75 and a 1.6-point stroke; the network glyph has at least five points of horizontal clearance within it. Hover gently darkens the complete hit area while labels remain white. The selected state remains a labeled egui button state. The line icons are drawn in code rather than loaded from Sony assets.

The title-bar network mark uses scale 0.65, approximately 19.2 points of ink, beside the 13-point brand. Its center stays at x=24 and the wordmark starts at x=43, retaining approximately nine points of separation. It is subordinate to the navigation and connection task.

The Settings heading is "Настройки DNS". Journal search and clearing are separate actions: the "Очистить журнал" button clears the visible list; its tooltip states that the log file is retained. Its width is measured from the translated label with a 144-point minimum. Search reserves the actual action width, the gap and text-edit margins.

### Inputs

Editable fields have a dark inset fill and a distinct one-point input-line boundary, calmer than the toggle track and hover border. Active field boundaries use the main light foreground. Field focus and selection use native egui states. Inputs are associated with their visible labels. Do not remove their functional boundaries to imitate the unframed navigation.

### Address and setup

Show the actual listening address with a separate Copy button and interface choice when more than one local address exists. In GUI automatic mode, saved IPv4 `0.0.0.0` resolves at runtime to the selected LAN IPv4 rather than a wildcard socket; the saved profile remains unchanged. Known internal NAT adapters are rejected for automatic selection so the VM gateway stays with SharedAccess. The picker is disabled while DNS runs: stop, choose the connection, then start. Running address text, Copy and the status tooltip derive from the successful listener. If that automatic address disappears, stop with recovery feedback instead of selecting another network silently. Explicit IPv4/IPv6 settings and headless wildcard behavior retain their existing meanings. With no local address, show a network recovery instruction. A concise PlayStation instruction is visible beside the address; detailed setup stays in one named disclosure. Startup and firewall progress share that secondary group, while the immediate startup behavior remains available in its tooltip.

### Recovery and feedback

A failure notice uses the error surface, readable title and concrete recovery text. Address and port bind failures explain what to change; technical details expand separately. Success feedback uses mint text. These notices report actual state and must not be decorative placeholders.

### Fixed form actions

Save and Cancel stay outside the scrolling form in a fixed-height reserved region. Clean forms render no buttons and leave no invisible focus targets. Edited forms show both buttons and the existing short state text. Save is unavailable while firewall provisioning is pending; Cancel remains available. Success feedback occupies this same reserved region, keeping the form still after saving. Disabled primary buttons revert to the secondary surface. Appearing or disappearing actions do not move the form.

## Do's and Don'ts

### Do:
- **Do** keep the actual address, named DNS state and relevant action easy to find.
- **Do** place Connection's Start/Stop below its address and preserve the compact global action elsewhere.
- **Do** retain labeled native controls, visible input boundaries and thin keyboard focus.
- **Do** preserve immediate startup feedback and staged Save/Cancel behavior.
- **Do** use the existing default and minimum native capture sets when checking visual changes.

### Don't:
- **Don't** restore thick persistent navigation outlines or bordered navigation cards.
- **Don't** add fake game covers, account data or Sony artwork to express the PS5 reference.
- **Don't** add busy scenery, bright star-like particles, invented shadows or web-only layout rules. Limit ambient movement to the soft shafts, reflected veil and low-opacity dust, with the pause policy above.
- **Don't** describe synthetic focus captures as Windows Narrator or full keyboard-runtime verification.

Current acceptance and its limits are recorded in [docs/VALIDATION.md](docs/VALIDATION.md).
