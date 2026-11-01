# Desktop portability assessment

Assessed: 2026-11-01. This is a source and documentation assessment, not a Linux or macOS implementation or acceptance report.

## Recommendation and status

Keep the native Rust application and its PS5-inspired interface. Verify Windows first, port Linux next, and consider macOS after the shared boundaries are established and a real Mac is available for testing. Reuse the DNS policy, configuration, translations and egui interface. Each port still needs platform integration, packaging and runtime acceptance.

| Target | Current project status | Next decision |
| --- | --- | --- |
| Windows x64 | Current implementation and current release target | Maintain Windows acceptance and portable packaging. |
| Linux | **PLANNED — not implemented or verified as a working application** | Implement and test a bounded desktop port after Windows. |
| macOS | **PLANNED — not implemented or verified as a working application** | Proceed after Linux boundary work and access to real target hardware. |

Windows x64 is the only release target. The cross-platform framework does not make the current Windows integration portable.

## What can be shared

[eframe 0.33.3 supports native Linux, macOS and Windows applications](https://github.com/emilk/egui/blob/0.33.3/crates/eframe/README.md). Keep one DNS implementation and one UI across platforms.

- Share rule and exception matching, DNS packet parsing and response semantics, upstream selection, deadlines, bounded logging, configuration validation and localized messages. Preserve behavior while replacing the OS-specific I/O boundary.
- Share the Connection, Rules, Settings and Log screens, dark palette, restrained motion and PS5-inspired visual hierarchy. Adapt window controls, keyboard shortcuts and desktop integration to each OS.
- Share translation catalogs and licensed bundled fonts, including the appropriate Simplified Chinese, Traditional Chinese, Japanese and Korean faces. Check text shaping, glyph coverage, wrapping, scaling and input methods on each target.
- Keep the console workflow: computer and PlayStation on the same network, selected LAN address, UDP port 53, and both console DNS fields pointing to that computer. A local high-port smoke test does not prove this workflow works.

The DNS module still depends on Windows: `readable` directly uses Winsock `select` and Windows socket handles. See [src/dns.rs](../src/dns.rs), especially `readable`, `forward` and `Service::start`.

## Boundaries that need platform implementations

| Boundary | Current Windows coupling | Linux / macOS work |
| --- | --- | --- |
| Build and dependencies | [Cargo.toml](../Cargo.toml) unconditionally includes `winreg` and Windows APIs; eframe and winit default features are disabled. [build.rs](../build.rs) conditionally embeds Windows resources. | Target-scope Windows-only dependencies and imports. Explicitly select and test Linux display and tray backends. Keep the existing vendored eframe changes under regression coverage. |
| Socket readiness and shutdown | `AsRawSocket`, Winsock `FD_SET`, `select` and `WSAGetLastError` in [src/dns.rs](../src/dns.rs). | Supply Unix readiness/error handling while retaining the current total deadline, cancellation, malformed-packet, retry and listener-release contracts. A new I/O runtime is not required just to make this boundary portable. |
| Port 53 rights | The Windows path opens the UDP listener directly. | Design a narrow permission mechanism for each OS. Keep the GUI running as the ordinary user; distinguish permission denial from port occupation and an unavailable address. See the port discussion below. |
| LAN selection and resolver coexistence | `local_addresses`, `interface_rank`, `listener_settings` and `refresh_interfaces` in [src/ui.rs](../src/ui.rs) select a real LAN address and stop on loss of that address. | Validate interface names, VPNs, bridges, Wi-Fi/Ethernet changes, IPv4/IPv6 behavior and suspend/resume on each OS. Preserve the host resolver and diagnose existing port owners. |
| Firewall and access state | [src/platform.rs](../src/platform.rs) provisions a program-specific inbound UDP rule through UAC-elevated PowerShell and verifies its configuration. | Provide OS-specific inspection and bounded setup. Preserve accurate states for unchecked, denied, pending and allowed access. A successful bind alone does not prove console reachability. |
| Tray and autostart | [src/ui.rs](../src/ui.rs) creates a tray icon; [src/platform.rs](../src/platform.rs) uses the current user's registry Run key and `--tray`. | Select a supported Linux tray backend and handle desktops without a visible tray. Use desktop autostart on Linux and native login-item registration on macOS. An app must remain recoverable when its tray/menu-bar item is absent. |
| Single instance and activation | [src/instance.rs](../src/instance.rs) combines a profile lock with HWNDs, registered Win32 messages, named events and per-caller acknowledgements. `ui::run` installs a Windows winit message hook. | Keep per-profile ownership and ordered activation/shutdown semantics, using an appropriate Unix IPC mechanism and OS window activation rules. Do not replace this with a PID-only check. |
| Window, motion and desktop actions | [src/platform.rs](../src/platform.rs) uses DWM corners, Win32 restore/focus/visibility and animation preferences, Explorer and Windows Settings URLs. | Implement normal window restoration, resize/drag behavior, reduced-motion preferences, folder/browser opening and network settings per OS. Wayland focus requests and macOS activation need runtime checks. |
| Paths, locale and fonts | `platform::data_dir` uses `LOCALAPPDATA`; [src/i18n.rs](../src/i18n.rs) selects the Windows UI language and currently returns English for other OSes. | Use platform-appropriate settings/log locations and native locale detection. Font selection must work without Windows font paths or redistributing unlicensed OS fonts. Preserve profile locking, atomic saves and readable Unicode paths. |

The framework's [Linux features include explicit `wayland` and `x11` backends](https://github.com/emilk/egui/blob/0.33.3/crates/eframe/Cargo.toml). The project's current feature selection does not enable either. [tray-icon 0.25.1](https://docs.rs/crate/tray-icon/0.25.1) supports AppIndicator or KSNI on Linux; AppIndicator needs the corresponding system libraries and event-loop integration, while KSNI uses its own worker. On macOS tray creation belongs on the main thread. Its cross-platform support does not make the current Windows callbacks portable.

## Port 53, the host resolver and access

### Linux

The kernel's default first unprivileged port is 1024; binding port 53 therefore normally needs root or `CAP_NET_BIND_SERVICE`. The threshold is configurable per network namespace, so inspect the target rather than assuming it. See the [Linux kernel IP sysctl documentation](https://docs.kernel.org/networking/ip-sysctl.html).

For an installed Linux build, evaluate a small service/helper that receives only the required bind privilege while the GUI stays unprivileged. A system service can run as a dedicated user with a restricted capability set; systemd documents [capability bounding and ambient capabilities](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html#AmbientCapabilities=). Socket activation is another design option, but would require accepting pre-opened sockets: the current `Service::start` creates its own. Neither option is implemented here. Do not solve installation by lowering the machine-wide privileged-port threshold or running the whole UI as root.

`systemd-resolved` commonly listens on loopback port 53 at `127.0.0.53` and `127.0.0.54`, as described by [DNSStubListener](https://www.freedesktop.org/software/systemd/man/latest/resolved.conf.html#DNSStubListener=). Prefer the selected LAN address over wildcard ownership, inspect other listeners such as dnsmasq, and test coexistence in both startup orders. Do not disable the host resolver or rewrite `/etc/resolv.conf` to make the console DNS work. Configure upstreams so the service cannot forward back into itself. Existing listeners can still prevent binding to the selected LAN address.

Firewall integration must identify the target's active firewall and scope an authorized rule to the chosen interface/subnet and UDP 53. Declare the tested distributions and desktop sessions instead of claiming every Linux setup. For GUI autostart, follow the [Desktop Application Autostart Specification](https://specifications.freedesktop.org/autostart/latest/); keep GUI login startup distinct from any privileged DNS service lifetime.

### macOS

Verify UDP 53 binding as the ordinary GUI user on the actual supported macOS version and hardware. Separate permission errors, address availability and conflicting listeners. Apple's [bind reference](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/bind.2.html) documents these error categories, but is archived and is not evidence of current low-port behavior. Decide whether a narrow helper or launchd-managed socket is needed from the target tests; do not transfer Linux capabilities or Windows UAC assumptions to macOS.

Network access needs its own acceptance path. Since macOS 15, local-network privacy can affect sending UDP replies to the console even when incoming unicast reception succeeds. Bundle metadata, explanatory permission text, denial handling and retry behavior must be tested. Apple's [TN3179](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy) distinguishes these operations and their exceptions. Check the [macOS firewall](https://support.apple.com/guide/mac-help/change-firewall-settings-on-mac-mh11783/mac) separately; do not report permission as allowed merely because the socket opened.

Use the native application/menu-bar lifecycle and [SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice) for supported login items or helper services. Preserve normal host DNS operation. Test sleep/wake, Wi-Fi changes, Dock and menu-bar restoration, duplicate launches and exit on real hardware. Scope Apple Silicon first if that is the available machine; claim Intel or Universal support only after those artifacts are built and tested.

## Staged acceptance

1. **Windows first.** Keep the current platform behavior and finish its existing DNS, lifecycle, localization and native-UI acceptance before publishing the Windows artifact. Verify the exact packaged executable.
2. **Shared boundaries, then Linux.** Separate socket readiness and platform services without altering rule semantics or the common screens. Start with one declared distribution and desktop configuration, verify the selected display/tray backends, and exercise DNS on an unprivileged test port before the installed UDP 53/LAN path. Preserve Windows checks throughout this work.
3. **Linux release gate.** Test actual console queries, overrides, exceptions, upstream failure, busy port and denied permissions; prove the host resolver still works; verify tray recovery, autostart off/on, duplicate launch, graceful stop and upgrade/removal of any helper or firewall rule. Record X11 and Wayland coverage separately. A CI compile alone does not meet this gate.
4. **macOS after real target testing.** Add the native integration and an application bundle, then repeat the DNS and lifecycle cases on the chosen Mac/OS. Include local-network permission allowed/denied, firewall state and downloaded-app launch behavior. Keep macOS marked PLANNED until this is complete.

Apple documents [Developer ID signing and notarization](https://developer.apple.com/developer-id/) for the normal direct-download experience under Gatekeeper. Decide the macOS distribution approach when that port is requested, and do not disable Gatekeeper as a product setup step.
