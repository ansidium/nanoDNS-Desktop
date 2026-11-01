use crate::{
    background,
    config::{Settings, parse_rules},
    dns::{QueryEvent, Service},
    fonts, i18n, instance, platform,
};
use eframe::egui::{self, Color32, FontId, RichText, Stroke, Vec2};
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::{
        atomic::Ordering,
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};
use tray_icon::{
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
use winit::platform::windows::EventLoopBuilderExtWindows;

const BG: Color32 = Color32::from_rgb(14, 16, 29);
const SIDE: Color32 = Color32::from_rgb(22, 25, 30);
const SURFACE: Color32 = Color32::from_rgb(38, 42, 48);
const TEXT: Color32 = Color32::from_rgb(237, 242, 245);
const NAV_TEXT: Color32 = Color32::WHITE;
const MUTED: Color32 = Color32::from_rgb(212, 220, 229);
const MINT: Color32 = Color32::from_rgb(152, 227, 194);
const LINE: Color32 = Color32::from_rgb(55, 61, 70);
const CONTROL_LINE: Color32 = Color32::from_rgb(139, 150, 165);
const INPUT_LINE: Color32 = Color32::from_rgb(103, 115, 131);
const ERROR: Color32 = Color32::from_rgb(240, 165, 165);
const PAGE_INLINE: i8 = 32;
const CONTENT_WIDTH: f32 = 880.0;
const FORM_WIDTH: f32 = 720.0;
const COLUMN_GAP: f32 = 28.0;
const GROUP_GAP: f32 = 28.0;
const SECTION_GAP: f32 = 20.0;
const FIELD_GAP: f32 = 8.0;
// Apparent repeat measured from a continuous minute of the console settings
// reference. This is a reconstruction, not Sony's internal animation clock.
const BACKGROUND_PERIOD_SECONDS: f64 = 26.3;

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Overview,
    Rules,
    Settings,
    Journal,
}

enum TrayAction {
    Show,
    Toggle,
    Quit,
    RestoreError(String),
}

#[derive(Clone, Copy)]
enum WindowControl {
    Minimize,
    Maximize,
    Restore,
    Close,
}

fn window_control(ui: &mut egui::Ui, control: WindowControl) -> egui::Response {
    let label = match control {
        WindowControl::Minimize => "Свернуть окно",
        WindowControl::Maximize => "Развернуть окно",
        WindowControl::Restore => "Восстановить размер окна",
        WindowControl::Close => "Закрыть окно",
    };
    let label = i18n::tr(label);
    let response = ui.add_sized(
        [44.0, 32.0],
        egui::Button::new("")
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .corner_radius(4),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(
            response.rect,
            4,
            if matches!(control, WindowControl::Close) {
                Color32::from_rgb(158, 43, 55)
            } else {
                SURFACE
            },
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.shrink(1.0),
            4,
            Stroke::new(1.0_f32, TEXT),
            egui::StrokeKind::Inside,
        );
    }
    let center = response.rect.center();
    let p = ui.painter();
    let stroke = Stroke::new(1.2_f32, TEXT);
    match control {
        WindowControl::Minimize => {
            p.line_segment(
                [center + Vec2::new(-5.0, 2.0), center + Vec2::new(5.0, 2.0)],
                stroke,
            );
        }
        WindowControl::Maximize => {
            p.rect_stroke(
                egui::Rect::from_center_size(center, Vec2::splat(9.0)),
                0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        WindowControl::Restore => {
            let back =
                egui::Rect::from_center_size(center + Vec2::new(1.5, -1.5), Vec2::splat(8.0));
            p.rect_stroke(back, 0, stroke, egui::StrokeKind::Inside);
            let front =
                egui::Rect::from_center_size(center + Vec2::new(-1.5, 1.5), Vec2::splat(8.0));
            p.rect_filled(
                front,
                0,
                if response.hovered() || response.has_focus() {
                    SURFACE
                } else {
                    BG
                },
            );
            p.rect_stroke(front, 0, stroke, egui::StrokeKind::Inside);
        }
        WindowControl::Close => {
            p.line_segment(
                [center + Vec2::new(-4.0, -4.0), center + Vec2::new(4.0, 4.0)],
                stroke,
            );
            p.line_segment(
                [center + Vec2::new(-4.0, 4.0), center + Vec2::new(4.0, -4.0)],
                stroke,
            );
        }
    }
    response.on_hover_text(label)
}

fn resize_direction(
    rect: egui::Rect,
    point: egui::Pos2,
) -> Option<egui::viewport::ResizeDirection> {
    use egui::viewport::ResizeDirection;
    if !rect.contains(point) {
        return None;
    }
    if point.x >= rect.right() - 20.0 && point.y >= rect.bottom() - 20.0 {
        return Some(ResizeDirection::SouthEast);
    }
    let left = point.x < rect.left() + 6.0;
    let right = point.x > rect.right() - 6.0;
    let top = point.y < rect.top() + 6.0;
    let bottom = point.y > rect.bottom() - 6.0;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, _, _, _) => Some(ResizeDirection::West),
        (_, true, _, _) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (_, _, _, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

fn window_resize(ctx: &egui::Context) {
    use egui::viewport::ResizeDirection;
    if ctx.input(|i| {
        i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
    }) {
        return;
    }
    let rect = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("resize_grip"),
    ));
    for length in [3.0, 7.0, 11.0] {
        painter.line_segment(
            [
                egui::pos2(rect.right() - 7.0 - length, rect.bottom() - 7.0),
                egui::pos2(rect.right() - 7.0, rect.bottom() - 7.0 - length),
            ],
            Stroke::new(1.0_f32, MUTED.gamma_multiply(0.65)),
        );
    }
    let direction = ctx
        .input(|i| i.pointer.latest_pos())
        .and_then(|point| resize_direction(ctx.content_rect(), point));
    if let Some(direction) = direction {
        let cursor = match direction {
            ResizeDirection::North | ResizeDirection::South => egui::CursorIcon::ResizeVertical,
            ResizeDirection::East | ResizeDirection::West => egui::CursorIcon::ResizeHorizontal,
            ResizeDirection::NorthWest | ResizeDirection::SouthEast => egui::CursorIcon::ResizeNwSe,
            ResizeDirection::NorthEast | ResizeDirection::SouthWest => egui::CursorIcon::ResizeNeSw,
        };
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
}

struct Editor {
    settings: Settings,
    upstreams: String,
    rules: String,
    exceptions: String,
}

impl Editor {
    fn new(settings: &Settings) -> Self {
        Self {
            settings: settings.clone(),
            upstreams: settings.upstreams.join("\n"),
            rules: settings.rule_text(),
            exceptions: settings.exceptions.join("\n"),
        }
    }
    fn read(&self) -> Result<Settings, String> {
        let mut settings = self.settings.clone();
        settings.bind = settings.bind.trim().to_owned();
        settings.bind6 = settings.bind6.trim().to_owned();
        settings.upstreams = self
            .upstreams
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        settings.rules = parse_rules(&self.rules)?;
        settings.exceptions = self
            .exceptions
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
            .map(|s| s.trim_end_matches('.').to_ascii_lowercase())
            .collect();
        settings.validate()?;
        Ok(settings)
    }
    fn changed(&self, settings: &Settings) -> bool {
        self.settings != *settings
            || self.upstreams != settings.upstreams.join("\n")
            || self.rules != settings.rule_text()
            || self.exceptions != settings.exceptions.join("\n")
    }
}

struct Qa {
    dir: PathBuf,
    stage: usize,
    wait_until: Instant,
    waiting: bool,
    window_restore: Option<Receiver<Result<bool, String>>>,
    action_position: Option<egui::Pos2>,
    action_injected: bool,
    cancel_verified: bool,
    save_verified: bool,
    queries_before_save: u64,
    motion: bool,
    started: Instant,
    motion_samples: Vec<serde_json::Value>,
    pending_motion_sample: Option<serde_json::Value>,
}

fn interface_rank(name: &str) -> (u8, u8, String) {
    let name = name.to_ascii_lowercase();
    // Names are a preference, not proof of reachability. Keep every adapter in
    // the picker, including external Hyper-V switches and hosted networks.
    let isolated = [
        "default switch",
        "wsl",
        "docker",
        "host-only",
        "host only",
        "internal",
        "hosted",
    ]
    .iter()
    .any(|&marker| name.contains(marker));
    let tunnel = [
        "singbox",
        "sing-box",
        "wireguard",
        "tailscale",
        "zerotier",
        "vpn",
        "tunnel",
    ]
    .iter()
    .any(|&marker| name.contains(marker))
        || name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|part| {
                part == "tun"
                    || part == "tap"
                    || part == "wintun"
                    || ["tun", "tap"].iter().any(|prefix| {
                        part.strip_prefix(*prefix).is_some_and(|suffix| {
                            !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
                        })
                    })
            });
    let network = if isolated {
        2
    } else if tunnel {
        1
    } else {
        0
    };
    let medium = if name.contains("wi-fi") || name.contains("wifi") || name.contains("wlan") {
        0
    } else if name.contains("ethernet") {
        1
    } else {
        2
    };
    (network, medium, name)
}

fn local_addresses() -> Result<Vec<(String, String)>, String> {
    let mut ips = if_addrs::get_if_addrs()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter_map(|a| match a.ip() {
            std::net::IpAddr::V4(ip)
                if !ip.is_loopback() && !ip.is_unspecified() && !ip.is_link_local() =>
            {
                Some((ip.to_string(), a.name))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    ips.sort_by_key(|(_, name)| interface_rank(name));
    ips.dedup();
    Ok(ips)
}

fn repaint_delay(running: bool, active: bool, busy: bool) -> Option<Duration> {
    if !active {
        None
    } else if busy {
        Some(Duration::from_millis(200))
    } else if running {
        Some(Duration::from_secs(1))
    } else {
        None
    }
}

fn background_delay(active: bool, reduced_motion: bool) -> Option<Duration> {
    (active && !reduced_motion).then_some(Duration::from_millis(33))
}

#[derive(Default)]
struct BackgroundMotion {
    phase: f64,
    previous_frame: Option<Instant>,
}

impl BackgroundMotion {
    fn tick(&mut self, now: Instant, playing: bool) -> f32 {
        if playing {
            if let Some(previous) = self.previous_frame {
                // Capping the step prevents a
                // stall or suspended computer from jumping across the scene.
                let elapsed = now
                    .saturating_duration_since(previous)
                    .as_secs_f64()
                    .min(0.25);
                self.phase = (self.phase
                    + elapsed * std::f64::consts::TAU / BACKGROUND_PERIOD_SECONDS)
                    % std::f64::consts::TAU;
            }
            self.previous_frame = Some(now);
        } else {
            // No timer, worker or accumulated hidden time to catch up later.
            self.previous_frame = None;
        }
        self.phase as f32
    }
}

fn needs_lan_firewall(settings: &Settings) -> bool {
    // Service::start validates both addresses before this is used. `off` is
    // intentionally not an address and therefore adds no IPv6 listener.
    let exposed = |address: &str| {
        address
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| match ip {
                std::net::IpAddr::V4(ip) => !ip.is_loopback(),
                std::net::IpAddr::V6(ip) => {
                    !ip.is_loopback() && !ip.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
                }
            })
    };
    exposed(&settings.bind) || exposed(&settings.bind6)
}

fn console_interface(bind: &str, ips: &[(String, String)], selected: usize) -> Option<usize> {
    if bind == "0.0.0.0" {
        if ips.get(selected).is_some() {
            Some(selected)
        } else {
            (!ips.is_empty()).then_some(0)
        }
    } else {
        ips.iter().position(|(address, _)| address == bind)
    }
}

fn listener_settings(
    settings: &Settings,
    ips: &[(String, String)],
    selected: usize,
) -> Result<Settings, String> {
    let mut runtime = settings.clone();
    if settings.bind == "0.0.0.0" {
        let (address, name) = ips.get(selected).ok_or(
            "Нет подключения к локальной сети. Подключите компьютер к Wi-Fi или Ethernet.",
        )?;
        // Own only the console-facing address. SharedAccess must retain the
        // private VM gateway; automatic mode must never reserve the wildcard.
        if interface_rank(name).0 == 2 {
            return Err("Выберите подключение к сети консоли, а не внутренний коммутатор виртуальных машин.".into());
        }
        runtime.bind.clone_from(address);
    }
    Ok(runtime)
}

fn console_warnings(bind: &str, port: u16, address: Option<&str>) -> [Option<&'static str>; 2] {
    let restricted = if bind
        .parse::<std::net::Ipv4Addr>()
        .is_ok_and(|ip| ip.is_loopback())
    {
        Some(
            "Консоль не может подключиться: DNS слушает только этот компьютер. В «Настройках» укажите IPv4 0.0.0.0.",
        )
    } else if bind != "0.0.0.0" && address != Some(bind) {
        Some(
            "Консоль не может подключиться: адрес прослушивания отсутствует в локальной сети. Проверьте IPv4 в «Настройках».",
        )
    } else {
        None
    };
    [
        restricted,
        (port != 53).then_some(
            "Для подключения PlayStation нужен порт UDP 53. Измените порт в «Настройках».",
        ),
    ]
}

pub struct App {
    settings: Settings,
    editor: Editor,
    dir: PathBuf,
    service: Option<Service>,
    page: Page,
    tray: Option<TrayIcon>,
    tray_toggle: MenuItem,
    tray_rx: Receiver<TrayAction>,
    main_window: Option<platform::MainWindow>,
    startup: bool,
    error: Option<String>,
    notice: Option<String>,
    log: VecDeque<QueryEvent>,
    search: String,
    ips: Vec<(String, String)>,
    selected_ip: usize,
    interfaces_refreshed: Instant,
    background: BackgroundMotion,
    background_renderer: Option<background::Renderer>,
    background_renderer_error: Option<String>,
    hidden: bool,
    quitting: bool,
    firewall_check: Option<Receiver<Result<bool, String>>>,
    firewall_wait: Option<Instant>,
    firewall_status: String,
    qa: Option<Qa>,
    // Keep ownership until DNS, tray and all other app fields have dropped.
    _instance: instance::Instance,
}

fn text(ui: &mut egui::Ui, value: impl Into<String>, size: f32, color: Color32) {
    let value = value.into();
    ui.label(
        RichText::new(i18n::message(&value).into_owned())
            .size(size)
            .color(color),
    );
}

fn heading(ui: &mut egui::Ui, title: &str, description: &str) {
    let size = if ui.available_width() < 710.0 {
        24.0
    } else {
        26.0
    };
    ui.label(
        RichText::new(i18n::tr(title))
            .font(console_font(size))
            .color(TEXT),
    );
    if !description.is_empty() {
        ui.add_space(6.0);
        text(ui, description, 14.0, MUTED);
    }
    ui.add_space(SECTION_GAP);
}

fn console_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("console-display".into()))
}

#[derive(Clone, Copy)]
enum ButtonStyle {
    Primary,
    Secondary,
    Outlined,
}

fn button(ui: &mut egui::Ui, label: &str, style: ButtonStyle, size: Vec2) -> egui::Response {
    let label = i18n::tr(label);
    let accent = matches!(style, ButtonStyle::Primary) && ui.is_enabled();
    let outlined = matches!(style, ButtonStyle::Outlined) && ui.is_enabled();
    // Restore the palette without adding a child UI: action buttons also live
    // in fixed footers, whose measured height must not change with their style.
    let previous_widgets = ui.visuals().widgets.clone();
    {
        let widgets = &mut ui.visuals_mut().widgets;
        for (visuals, fill, stroke) in [
            (
                &mut widgets.inactive,
                if accent { TEXT } else { SURFACE },
                if outlined {
                    CONTROL_LINE
                } else {
                    Color32::TRANSPARENT
                },
            ),
            (
                &mut widgets.hovered,
                if accent {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(58, 64, 74)
                },
                if outlined {
                    TEXT
                } else {
                    Color32::from_white_alpha(100)
                },
            ),
            (
                &mut widgets.active,
                if accent {
                    MUTED
                } else {
                    Color32::from_rgb(78, 86, 98)
                },
                if outlined { TEXT } else { CONTROL_LINE },
            ),
        ] {
            visuals.bg_fill = fill;
            visuals.weak_bg_fill = fill;
            visuals.bg_stroke = if accent {
                Stroke::NONE
            } else {
                Stroke::new(1.0_f32, stroke)
            };
            visuals.expansion = 0.0;
        }
    }
    let response = ui.add_sized(
        size,
        egui::Button::new(
            RichText::new(label)
                .size(14.0)
                .color(if accent { SIDE } else { TEXT }),
        )
        .corner_radius(egui::CornerRadius::same(20)),
    );
    ui.visuals_mut().widgets = previous_widgets;
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(3.0),
            23,
            Stroke::new(1.5_f32, TEXT),
            egui::StrokeKind::Outside,
        );
    }
    response
}

fn switch_row(ui: &mut egui::Ui, value: &mut bool, label: &str) -> egui::Response {
    let label = i18n::tr(label);
    let width = ui.available_width().min(FORM_WIDTH);
    let color = if ui.is_enabled() { TEXT } else { MUTED };
    let label = ui.painter().layout(
        label.to_owned(),
        FontId::proportional(14.0),
        color,
        (width - 60.0).max(64.0),
    );
    let height = (label.size().y + 16.0).max(40.0);
    let mut response = ui.add_sized(
        [width, height],
        egui::Button::new("")
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .corner_radius(5),
    );
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            *value,
            label.text(),
        )
    });
    if response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(response.rect.expand(4.0), 5, Color32::from_white_alpha(12));
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(4.0),
            5,
            Stroke::new(1.0_f32, TEXT),
            egui::StrokeKind::Inside,
        );
    }
    let center = response.rect.center();
    ui.painter().galley(
        egui::pos2(response.rect.left(), center.y - label.size().y * 0.5),
        label,
        color,
    );
    let track = egui::Rect::from_center_size(
        egui::pos2(response.rect.right() - 18.0, center.y),
        Vec2::new(36.0, 20.0),
    );
    ui.painter()
        .rect_filled(track, 10, if *value { TEXT } else { CONTROL_LINE });
    ui.painter().circle_filled(
        egui::pos2(
            if *value {
                track.right() - 10.0
            } else {
                track.left() + 10.0
            },
            center.y,
        ),
        7.0,
        if *value { SIDE } else { BG },
    );
    response
}

fn setting_row(ui: &mut egui::Ui, label: &str, contents: impl FnOnce(&mut egui::Ui, egui::Id)) {
    let label = i18n::tr(label);
    let width = ui.available_width();
    let label_width = (width * 0.36).clamp(190.0, 248.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = COLUMN_GAP;
        let label = ui.allocate_ui_with_layout(
            Vec2::new(label_width, 32.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(label_width);
                ui.label(RichText::new(label).size(14.0).color(TEXT))
            },
        );
        ui.allocate_ui_with_layout(
            Vec2::new(width - label_width - COLUMN_GAP, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_width(width - label_width - COLUMN_GAP);
                contents(ui, label.inner.id);
            },
        );
    });
}

fn journal_cell(ui: &mut egui::Ui, width: f32, height: f32, value: RichText) -> egui::Response {
    ui.allocate_ui_with_layout(
        Vec2::new(width, height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(Vec2::new(width, height));
            ui.add(egui::Label::new(value).truncate())
        },
    )
    .inner
}

fn journal_search_row(
    ui: &mut egui::Ui,
    search: &mut String,
    has_rows: bool,
    language: i18n::Language,
) -> egui::InnerResponse<bool> {
    ui.horizontal(|ui| {
        let search_label = ui.label(i18n::translate(language, "Поиск"));
        let clear_label = i18n::translate(language, "Очистить журнал");
        let clear_width = (ui
            .painter()
            .layout_no_wrap(clear_label.to_owned(), FontId::proportional(14.0), TEXT)
            .size()
            .x
            + 2.0 * ui.spacing().button_padding.x)
            .ceil()
            .max(144.0);
        // TextEdit's desired width excludes its horizontal margin. Reserve
        // both that margin and the gap before the label-sized clear action.
        let width =
            (ui.available_width() - clear_width - ui.spacing().item_spacing.x - 16.0).max(180.0);
        ui.add(
            egui::TextEdit::singleline(search)
                .hint_text(i18n::translate(language, "Домен или IP клиента"))
                .desired_width(width)
                .margin(egui::Margin::symmetric(8, 6)),
        )
        .labelled_by(search_label.id)
        .on_hover_text(i18n::tr("Фильтр журнала по домену или IP клиента"));
        ui.add_enabled_ui(has_rows, |ui| {
            button(
                ui,
                clear_label,
                ButtonStyle::Secondary,
                Vec2::new(clear_width, 32.0),
            )
            .on_hover_text(i18n::tr(
                "Очистить список в этом окне. Файл журнала сохраняется.",
            ))
            .clicked()
        })
        .inner
    })
}

fn ambient_row(v: f32, phase: f32) -> impl Fn(f32) -> Color32 {
    // Reference-derived curtains open independently over a quiet upper veil.
    // Keep this fallback identical to the shader; prepare row terms once.
    let angle = phase + 2.680_045;
    let (wave, cosine) = angle.sin_cos();
    let (cross, cross_cosine) = (2.0 * angle).sin_cos();
    let mix = 0.5 + 0.5 * (0.852_524_5 * wave + 0.522_687_26 * cosine);
    let balance = 0.696_706_7 * wave + 0.717_356_1 * cosine;
    let width_scale = 0.822_103_6 + 0.35 * (0.995_534_3 * wave + 0.094_400_67 * cosine);
    let axis = 0.149_638_24
        + (0.177_776_7 + 0.003 * cross) * v
        + 0.007_364_050_5 * (0.952_703_8 * wave + 0.303_900_42 * cosine);
    let left_width = (0.088_045_79 + 0.327_058_37 * v) * (1.0 + 0.07 * balance) * 0.822_103_6;
    let right_width = (0.090_456_17 + 0.339_814_22 * v) * (1.0 - 0.05 * balance) * width_scale;
    let fold_axes = [
        0.16 + 0.12 * v + 0.03 * (0.623_774_65 * wave + 0.781_604_23 * cosine),
        0.21 + 0.36 * v + 0.03 * (-0.320_670_4 * wave + 0.947_190_8 * cosine),
        0.18 + 0.35 * v,
    ];
    let fold_widths = [
        (0.012 + 0.018 * v) * (0.9 + 0.3 * mix) * 2.263_834_2,
        (0.025 + 0.035 * v) * (1.15 - 0.25 * mix) * 2.263_834_2,
        (0.06 + 0.12 * v) * (1.0 + 0.15 * cross),
    ];
    let fold_fade = (-((v - 0.2).max(0.0) / 0.24).powi(2)).exp();
    let fold_strengths = [
        -0.352_587_2 * (0.14 + 0.70 * mix),
        -0.352_587_2 * (0.13 + 0.73 * (1.0 - mix)),
        0.493_545_7 * mix,
    ]
    .map(|strength| strength * fold_fade);
    // Limit compensation for occluded energy. The fan redistributes between
    // soft overlapping folds rather than shrinking into a moving spotlight.
    let integral = 0.892_98 * left_width + 0.886_227 * right_width;
    let mut occupied = 0.0;
    for index in 0..3 {
        let distance = fold_axes[index] - axis;
        let z = distance
            / if distance < 0.0 {
                left_width
            } else {
                right_width
            };
        let exponent = if distance < 0.0 {
            z.abs().powi(3)
        } else {
            z * z
        };
        occupied += fold_strengths[index] * 1.772_454 * fold_widths[index] * (-exponent).exp();
    }
    let gain = (1.0 + 0.60 * (integral / (integral + occupied).max(0.03) - 1.0)).clamp(0.92, 1.32);
    let attenuation = (1.0 + 3.449_008 * v).powf(1.837_618_8);
    let depth_wave = 0.881_555_8 * (1.0 + 0.08 * (0.921_061 * wave - 0.389_418_33 * cosine))
        / width_scale.powf(0.25);
    let depth = depth_wave * gain / attenuation;
    let curtain = 0.5 + 0.5 * (-0.099_539_52 * cross + 0.995_033_6 * cross_cosine);
    let reflection_wave = -0.895_364_3 * wave - 0.445_334_52 * cosine;
    let reflection_cross = 0.603_354_3 * cross + 0.797_473_25 * cross_cosine;
    let reflected = (-((v - 0.880_812_9 - 0.002 * reflection_wave) / 0.109_852_17).powi(2)).exp()
        * (1.0 + 0.01 * reflection_cross);
    let v2 = v * v;
    let base = [
        0.004_391_442 + 0.003_631_751 * v2,
        0.005_181_517 + 0.003_952_542 * v2,
        0.012_286_488 + 0.003_709_805 * v2,
    ];
    move |u| {
        let distance = u - axis;
        let scaled = distance
            / if distance < 0.0 {
                left_width
            } else {
                right_width
            };
        let exponent = if distance < 0.0 {
            scaled.abs().powi(3)
        } else {
            scaled * scaled
        };
        let mut modulation = 1.0;
        for index in 0..3 {
            let layer = (-((u - fold_axes[index]) / fold_widths[index]).powi(2)).exp();
            modulation += fold_strengths[index] * layer;
        }
        let fan = (-exponent).exp() * depth * modulation.max(0.0)
            + fold_fade
                * 0.062_920_74
                * (-((u - (0.34 + 0.25 * v)) / (0.2 + 0.2 * v)).powi(2)).exp()
                / attenuation
            + fold_fade
                * 0.149_999_08
                * curtain
                * (-((u - (0.285 + 0.4 * v)) / (0.03 + 0.03 * v)).powi(2)).exp()
                / attenuation;
        let reflected_distance = u - 0.198_684_9;
        let reflected_width = if reflected_distance < 0.0 {
            0.355_730_4
        } else {
            0.236_570_57
        };
        let reflection = (-(reflected_distance / reflected_width).powi(2)).exp() * reflected;
        Color32::from(egui::Rgba::from_rgb(
            base[0] + 0.268_452_23 * fan + 0.044_386_417 * reflection,
            base[1] + 0.197_439_45 * fan * (1.0 + 0.5 * v2) + 0.044_445_373 * reflection,
            base[2] + 0.169_893_6 * fan * (1.0 + v2) + 0.043_620_296 * reflection,
        ))
    }
}

fn console_background(ctx: &egui::Context, phase: f32, renderer: Option<&background::Renderer>) {
    let rect = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());
    if let Some(renderer) = renderer {
        painter.add(renderer.paint_callback(rect, phase));
    } else {
        let mut mesh = egui::Mesh::default();
        mesh.vertices.reserve(97 * 41);
        mesh.indices.reserve(96 * 40 * 6);
        for y in 0_u16..=40 {
            let v = f32::from(y) / 40.0;
            let colour = ambient_row(v, phase);
            for x in 0_u16..=96 {
                let u = f32::from(x) / 96.0;
                mesh.colored_vertex(
                    egui::pos2(
                        rect.left() + u * rect.width(),
                        rect.top() + v * rect.height(),
                    ),
                    colour(u),
                );
            }
        }
        for y in 0_u32..40 {
            for x in 0_u32..96 {
                let a = y * 97 + x;
                mesh.add_triangle(a, a + 1, a + 97);
                mesh.add_triangle(a + 1, a + 98, a + 97);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
    }
    // The source's fine texture has no clearly trackable bright star field.
    // Subpixel silver grains drift locally; there are no falling-dot wraps.
    for index in 0_u16..96 {
        let seed = f32::from(index);
        let depth = index % 4;
        let drift = (phase + seed).sin();
        let alpha = (1.0 + (1.0 + drift) * 0.3 * f32::from(depth)).round() as u8;
        let u = 0.02 + 0.96 * (seed * 0.618_034).fract() + 0.0015 * drift;
        let v = 0.04 + 0.92 * (seed * 0.381_966).fract() + 0.002 * (phase + seed * 0.7).cos();
        let point = egui::pos2(
            rect.left() + u * rect.width(),
            rect.top() + v * rect.height(),
        );
        let radius = 0.15 + f32::from(depth) * 0.10;
        painter.circle_filled(
            point,
            radius,
            Color32::from_rgba_unmultiplied(229, 232, 238, alpha),
        );
    }
}

fn console_icon(p: &egui::Painter, c: egui::Pos2, page: Page, color: Color32, scale: f32) {
    let stroke = Stroke::new(1.6_f32, color);
    let point = |x: f32, y: f32| c + Vec2::new(x, y) * scale;
    match page {
        Page::Overview => {
            for endpoint in [point(-10.0, 8.0), point(10.0, 8.0), point(0.0, -10.0)] {
                p.line_segment([c, endpoint], stroke);
                p.circle_stroke(endpoint, 3.5 * scale, stroke);
            }
            p.circle_filled(c, 2.5 * scale, color);
        }
        Page::Rules => {
            let points = vec![
                point(-11.0, -9.0),
                point(11.0, -9.0),
                point(3.0, 0.0),
                point(3.0, 10.0),
                point(-3.0, 7.0),
                point(-3.0, 0.0),
            ];
            p.add(egui::Shape::closed_line(points, stroke));
        }
        Page::Settings => {
            for (x, y) in [(-9.0, -4.0), (0.0, 5.0), (9.0, -1.0)] {
                p.line_segment([point(x, -11.0), point(x, 11.0)], stroke);
                p.circle_filled(point(x, y), 3.0 * scale, color);
            }
        }
        Page::Journal => {
            for y in [-8.0, 0.0, 8.0] {
                p.circle_filled(point(-10.0, y), 1.8 * scale, color);
                p.line_segment([point(-4.0, y), point(11.0, y)], stroke);
            }
        }
    }
}

fn theme(ctx: &egui::Context) {
    // set_style changes only the active theme. Windows light-theme events must
    // not swap in an uncustomized palette underneath the dark app surfaces.
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
        fonts
            .font_data
            .insert("segoe".into(), egui::FontData::from_owned(bytes).into());
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "segoe".into());
        }
    }
    let fallback = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let display = if let Ok(bytes) = fs::read("C:\\Windows\\Fonts\\segoeuil.ttf") {
        fonts.font_data.insert(
            "console-light".into(),
            egui::FontData::from_owned(bytes).into(),
        );
        vec!["console-light".into()]
    } else {
        fallback
    };
    fonts
        .families
        .insert(egui::FontFamily::Name("console-display".into()), display);
    fonts::add_cjk(&mut fonts, i18n::language());
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = BG;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.weak_text_color = Some(MUTED);
    style.visuals.extreme_bg_color = SIDE;
    style.visuals.faint_bg_color = SURFACE;
    style.visuals.hyperlink_color = Color32::from_rgb(168, 203, 249);
    style.visuals.selection.bg_fill = Color32::from_rgb(54, 65, 82);
    style.visuals.selection.stroke = Stroke::new(1.0_f32, TEXT);
    style.visuals.widgets.inactive.bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, INPUT_LINE);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, MUTED);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(58, 64, 74);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, CONTROL_LINE);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(78, 86, 98);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, TEXT);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(5);
    style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(58, 64, 74);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(78, 86, 98);
    style.spacing.item_spacing = Vec2::new(12.0, 8.0);
    style.spacing.interact_size.y = 28.0;
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, FontId::monospace(13.0));
    let reduce_motion = !platform::client_animations_enabled();
    style.animation_time = if reduce_motion { 0.0 } else { 0.16 };
    ctx.set_style(style);
}

impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        settings: Settings,
        dir: PathBuf,
        instance: instance::Instance,
        tray_start: bool,
        qa: Option<PathBuf>,
        qa_motion: bool,
    ) -> Result<Self, String> {
        theme(&cc.egui_ctx);
        cc.egui_ctx
            .send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
        let (sender, tray_rx) = mpsc::channel();
        let window_result = platform::MainWindow::from_context(cc);
        let main_window = window_result.as_ref().ok().copied();
        let menu = Menu::new();
        let show = MenuItem::new(i18n::tr("Открыть nanoDNS"), true, None);
        let toggle = MenuItem::new(i18n::tr("Запустить DNS"), true, None);
        let quit = MenuItem::new(i18n::tr("Выйти"), true, None);
        menu.append_items(&[&show, &toggle, &PredefinedMenuItem::separator(), &quit])
            .map_err(|e| e.to_string())?;
        let show_id = show.id().clone();
        let toggle_id = toggle.id().clone();
        let quit_id = quit.id().clone();
        let ctx = cc.egui_ctx.clone();
        let menu_sender = sender.clone();
        let menu_hook = instance.hook();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            platform::lifecycle_trace(format_args!("menu {:?}", event.id));
            let action = if event.id == show_id {
                Some(TrayAction::Show)
            } else if event.id == toggle_id {
                Some(TrayAction::Toggle)
            } else if event.id == quit_id {
                Some(TrayAction::Quit)
            } else {
                None
            };
            if let Some(action) = action {
                let _ = menu_sender.send(TrayAction::Show);
                if !matches!(&action, TrayAction::Show) {
                    let _ = menu_sender.send(action);
                }
                // Restore from the outer loop after this native callback and
                // any surrounding eframe visibility commands have completed.
                if let Some(window) = main_window
                    && let Err(error) = menu_hook.wake(window)
                {
                    let _ = menu_sender.send(TrayAction::RestoreError(error));
                }
                ctx.request_repaint();
            }
        }));
        let ctx = cc.egui_ctx.clone();
        let tray_hook = instance.hook();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                let _ = sender.send(TrayAction::Show);
                if let Some(window) = main_window
                    && let Err(error) = tray_hook.wake(window)
                {
                    let _ = sender.send(TrayAction::RestoreError(error));
                }
                ctx.request_repaint();
            }
        }));
        let image = image::load_from_memory(include_bytes!("../assets/nanodns.png"))
            .map_err(|e| e.to_string())?
            .into_rgba8();
        let tray_icon =
            tray_icon::Icon::from_rgba(image.into_raw(), 256, 256).map_err(|e| e.to_string())?;
        let tray_result = TrayIconBuilder::new()
            .with_icon(tray_icon)
            .with_menu(Box::new(menu))
            .with_tooltip(i18n::tr("nanoDNS — DNS остановлен"))
            .with_menu_on_left_click(false)
            .build();
        let tray_error = tray_result
            .as_ref()
            .err()
            .map(|e| format!("Трей недоступен: {e}"));
        let ips = local_addresses().unwrap_or_default();
        let startup = platform::startup_enabled();
        let (background_renderer, background_renderer_error) = match cc.gl.as_deref() {
            Some(gl) => match background::Renderer::new(gl) {
                Ok(renderer) => (Some(renderer), None),
                Err(error) => {
                    eprintln!("Background renderer fallback: {error}");
                    (None, Some(error))
                }
            },
            None => (None, Some("OpenGL context unavailable".into())),
        };
        let mut app = Self {
            editor: Editor::new(&settings),
            settings,
            dir,
            service: None,
            page: Page::Overview,
            tray: tray_result.ok(),
            tray_toggle: toggle,
            tray_rx,
            main_window,
            startup: startup.as_ref().copied().unwrap_or(false),
            error: tray_error
                .or_else(|| startup.err())
                .or_else(|| window_result.err()),
            notice: None,
            log: VecDeque::new(),
            search: String::new(),
            ips,
            selected_ip: 0,
            interfaces_refreshed: Instant::now(),
            background: BackgroundMotion::default(),
            background_renderer,
            background_renderer_error,
            hidden: false,
            quitting: false,
            firewall_check: None,
            firewall_wait: None,
            firewall_status: "Проверяется при запуске DNS".into(),
            qa: qa.map(|dir| Qa {
                dir,
                stage: 0,
                wait_until: Instant::now() + Duration::from_millis(650),
                waiting: false,
                window_restore: None,
                action_position: None,
                action_injected: false,
                cancel_verified: false,
                save_verified: false,
                queries_before_save: 0,
                motion: qa_motion,
                started: Instant::now(),
                motion_samples: Vec::new(),
                pending_motion_sample: None,
            }),
            _instance: instance,
        };
        if app.qa.is_none() && app.settings.start_dns {
            let initial_error = app.error.take();
            app.start();
            // Starting DNS clears previous DNS errors. Preserve initialization
            // failures so an autostart never hides a window needing attention.
            app.error = app.error.or(initial_error);
        }
        if tray_start && app.tray.is_some() && app.main_window.is_some() && app.error.is_none() {
            app.hide_window(&cc.egui_ctx);
        }
        if let Some(window) = main_window {
            app._instance.attach(window, cc.egui_ctx.clone())?;
        }
        Ok(app)
    }

    fn refresh_interfaces(&mut self) {
        self.interfaces_refreshed = Instant::now();
        let Ok(ips) = local_addresses() else {
            return;
        };
        let listening = self
            .service
            .as_ref()
            .map(|service| service.listen_ipv4().ip().to_string());
        let previous = listening
            .as_deref()
            .or_else(|| self.ips.get(self.selected_ip).map(|ip| ip.0.as_str()));
        self.selected_ip = previous
            .and_then(|address| ips.iter().position(|ip| ip.0 == address))
            .unwrap_or(0);
        self.ips = ips;
        if self.settings.bind == "0.0.0.0"
            && listening.is_some_and(|address| !self.ips.iter().any(|ip| ip.0 == address))
        {
            self.stop();
            self.error = Some("Подключение, на котором работал DNS, недоступно. Выберите сеть консоли и запустите DNS снова.".into());
        }
    }

    fn hide_window(&mut self, ctx: &egui::Context) {
        if let Some(window) = self.main_window {
            match window.minimized() {
                Ok(minimized) => {
                    // Native minimize (taskbar/Win32) can leave winit's style
                    // cache stale. Synchronize before Visible(false) rewrites
                    // window styles, preserving the real normal placement.
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(minimized));
                }
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            }
        }
        self.hidden = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        self.hidden = false;
        self.refresh_interfaces();
        if let Some(window) = self.main_window
            && let Err(error) = window.restore()
        {
            self.error = Some(error);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        // Native restoration is synchronous on this UI thread. winit now reads
        // IsIconic=false without another SW_RESTORE, and this command also
        // clears egui-winit's cached minimized flag after minimize + tray hide.
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn start(&mut self) {
        if self.service.is_some() {
            return;
        }
        self.refresh_interfaces();
        self.error = None;
        let result = listener_settings(&self.settings, &self.ips, self.selected_ip)
            .and_then(|runtime| Service::start(&runtime, &self.dir.join("nanodns.log")));
        match result {
            Ok(service) => {
                self.service = Some(service);
                self.update_tray();
                if !needs_lan_firewall(&self.settings) {
                    self.firewall_status = "Локальный режим · консоль недоступна".into();
                } else if self.qa.is_none()
                    && self.firewall_check.is_none()
                    && self.firewall_wait.is_none()
                {
                    let (sender, receiver) = mpsc::channel();
                    let port = self.settings.port;
                    thread::spawn(move || {
                        let _ = sender.send(platform::firewall_configured(port));
                    });
                    self.firewall_check = Some(receiver);
                    self.firewall_status = "Проверяем доступ из локальной сети…".into();
                }
            }
            Err(error) => {
                self.error = Some(error);
                self.update_tray();
            }
        }
    }
    fn stop(&mut self) {
        self.service = None;
        self.update_tray();
    }
    fn toggle(&mut self) {
        if self.service.is_some() {
            self.stop();
        } else {
            self.start();
        }
    }
    fn update_tray(&self) {
        let running = self.service.is_some();
        self.tray_toggle.set_text(i18n::tr(if running {
            "Остановить DNS"
        } else {
            "Запустить DNS"
        }));
        if let Some(tray) = &self.tray {
            let _ = tray.set_tooltip(Some(i18n::tr(if running {
                "nanoDNS — DNS работает"
            } else {
                "nanoDNS — DNS остановлен"
            })));
        }
    }
    fn save(&mut self) {
        if self.firewall_check.is_some() || self.firewall_wait.is_some() {
            self.notice = Some("Дождитесь завершения настройки брандмауэра.".into());
            return;
        }
        match self.editor.read().and_then(|settings| {
            settings
                .save(&self.dir.join("settings.json"))
                .map(|()| settings)
        }) {
            Ok(settings) => {
                let restart = self.service.is_some();
                self.stop();
                self.settings = settings;
                self.editor = Editor::new(&self.settings);
                self.error = None;
                self.notice = Some("Настройки сохранены".into());
                if restart {
                    self.start();
                }
            }
            Err(error) => self.error = Some(error),
        }
    }
    fn startup_control(&mut self, ui: &mut egui::Ui) {
        let old = self.startup;
        if switch_row(ui, &mut self.startup, "Запускать с Windows")
            .on_hover_text(i18n::tr(
                "Приложение запускается в трее. Этот переключатель применяется сразу.",
            ))
            .changed()
            && self.qa.is_none()
            && let Err(error) = platform::set_startup(self.startup)
        {
            self.startup = old;
            self.error = Some(error);
        }
    }
    fn chrome(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("window_titlebar")
            .exact_height(44.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let rect = ui.max_rect().shrink(6.0);
                let tools_width = if self.page == Page::Overview {
                    280.0
                } else {
                    448.0
                };
                let drag_rect = egui::Rect::from_min_max(
                    rect.min,
                    egui::pos2(rect.right() - tools_width, rect.bottom()),
                );
                let drag = ui.interact(
                    drag_rect,
                    ui.id().with("window_drag"),
                    egui::Sense::click_and_drag(),
                );
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                if drag.double_clicked_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                } else if drag.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                console_icon(
                    ui.painter(),
                    egui::pos2(24.0, drag_rect.center().y),
                    Page::Overview,
                    NAV_TEXT,
                    0.65,
                );
                ui.painter().text(
                    egui::pos2(43.0, drag_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "nanoDNS",
                    FontId::proportional(13.0),
                    NAV_TEXT,
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(rect)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if window_control(ui, WindowControl::Close).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if window_control(
                            ui,
                            if maximized {
                                WindowControl::Restore
                            } else {
                                WindowControl::Maximize
                            },
                        )
                        .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                        }
                        if window_control(ui, WindowControl::Minimize).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        ui.add_space(16.0);
                        let running = self.service.is_some();
                        if self.page != Page::Overview {
                            if button(
                                ui,
                                if running {
                                    "Остановить DNS"
                                } else {
                                    "Запустить DNS"
                                },
                                if running {
                                    ButtonStyle::Outlined
                                } else {
                                    ButtonStyle::Primary
                                },
                                Vec2::new(152.0, 30.0),
                            )
                            .clicked()
                            {
                                self.toggle();
                            }
                            ui.add_space(12.0);
                        }
                        ui.label(
                            RichText::new(i18n::tr(
                                if running && !needs_lan_firewall(&self.settings) {
                                    "DNS работает локально"
                                } else if running {
                                    "DNS работает"
                                } else {
                                    "DNS выключен"
                                },
                            ))
                            .size(12.0)
                            .color(if running { MINT } else { MUTED }),
                        )
                        .on_hover_text(format!(
                            "{} · UDP",
                            self.service.as_ref().map_or_else(
                                || format!("{}:{}", self.settings.bind, self.settings.port),
                                |service| service.listen_ipv4().to_string(),
                            )
                        ));
                        ui.add_space(7.0);
                        let (dot, _) =
                            ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                        ui.painter().circle_filled(
                            dot.center(),
                            3.0,
                            if running { MINT } else { MUTED },
                        );
                    },
                );
            });
        egui::TopBottomPanel::top("toolbar")
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::NONE)
                    .inner_margin(egui::Margin::symmetric(PAGE_INLINE, 8)),
            )
            .show(ctx, |ui| {
                ui.set_max_width(ui.available_width().min(CONTENT_WIDTH));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    for (page, title, width) in [
                        (Page::Overview, "Подключение", 154.0_f32),
                        (Page::Rules, "Правила", 126.0),
                        (Page::Settings, "Настройки", 142.0),
                        (Page::Journal, "Журнал", 126.0),
                    ] {
                        let title = i18n::tr(title);
                        let width = width.max(
                            ui.painter()
                                .layout_no_wrap(
                                    title.to_owned(),
                                    FontId::proportional(14.0),
                                    NAV_TEXT,
                                )
                                .size()
                                .x
                                + 60.0,
                        );
                        let selected = self.page == page;
                        // Selection belongs to the complete labeled tab, with
                        // generous space around an unframed icon.
                        let response = ui.add_sized(
                            [width, 44.0],
                            egui::Button::new(RichText::new(title).color(Color32::TRANSPARENT))
                                .selected(selected)
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::NONE)
                                .corner_radius(8),
                        );
                        let icon_rect = egui::Rect::from_center_size(
                            response.rect.left_center() + Vec2::new(20.0, 0.0),
                            Vec2::splat(32.0),
                        );
                        let hovered = response.hovered();
                        let t = ctx.animate_bool_with_time(
                            ui.id().with(title),
                            hovered,
                            ui.style().animation_time,
                        );
                        ui.painter().rect_filled(
                            response.rect,
                            8,
                            if selected {
                                Color32::from_black_alpha((13.0 + t * 9.0).round() as u8)
                            } else {
                                Color32::from_rgba_unmultiplied(
                                    68,
                                    74,
                                    86,
                                    (t * 72.0).round() as u8,
                                )
                            },
                        );
                        if selected || response.has_focus() {
                            ui.painter().rect_stroke(
                                response.rect,
                                8,
                                if response.has_focus() {
                                    Stroke::new(1.5_f32, TEXT)
                                } else {
                                    Stroke::new(1.0_f32, Color32::from_white_alpha(175))
                                },
                                egui::StrokeKind::Inside,
                            );
                        }
                        console_icon(ui.painter(), icon_rect.center(), page, NAV_TEXT, 0.75);
                        ui.painter().text(
                            response.rect.left_center() + Vec2::new(48.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            title,
                            FontId::proportional(14.0),
                            NAV_TEXT,
                        );
                        if response.clicked() {
                            self.page = page;
                            self.error = None;
                            self.notice = None;
                        }
                    }
                });
            });
    }

    fn dns_action(&mut self, ui: &mut egui::Ui) {
        let running = self.service.is_some();
        let response = button(
            ui,
            if running {
                "Остановить DNS"
            } else {
                "Запустить DNS"
            },
            if running {
                ButtonStyle::Outlined
            } else {
                ButtonStyle::Primary
            },
            Vec2::new(152.0, 36.0),
        );
        if self
            .qa
            .as_ref()
            .is_some_and(|qa| !qa.motion && qa.stage == 8)
        {
            response.request_focus();
        }
        if response.clicked() {
            self.toggle();
        }
    }

    fn overview(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "Подключение",
            "Компьютер и PlayStation должны быть в одной сети.",
        );
        let width = ui.available_width();
        let left_width = (width - COLUMN_GAP) * 0.55;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = COLUMN_GAP;
            ui.allocate_ui_with_layout(
                Vec2::new(left_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    self.connection_address(ui);
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(width - left_width - COLUMN_GAP, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    self.console_setup(ui);
                    ui.add_space(GROUP_GAP);
                    self.startup_control(ui);
                    self.connection_status(ui);
                },
            );
        });
    }

    fn connection_status(&mut self, ui: &mut egui::Ui) {
        // The title and connection notice already explain a loopback-only DNS.
        if self.service.is_some() && !needs_lan_firewall(&self.settings) {
            return;
        }
        if self.service.is_some()
            || self.firewall_check.is_some()
            || self.firewall_wait.is_some()
            || self.firewall_status.starts_with("Доступ не")
        {
            ui.add_space(FIELD_GAP);
            let status = if self.firewall_status == "Доступ разрешён · UDP · локальная сеть"
            {
                "Доступ из локальной сети разрешён"
            } else {
                &self.firewall_status
            };
            ui.label(RichText::new(i18n::tr(status)).size(13.0).color(MUTED))
                .on_hover_text(format!(
                    "{} {}",
                    i18n::tr("Брандмауэр Windows:"),
                    i18n::tr(&self.firewall_status)
                ));
        }
        if self.firewall_status.starts_with("Доступ не")
            && ui.link(i18n::tr("Повторить настройку")).clicked()
        {
            self.request_firewall();
        }
    }

    fn connection_address(&mut self, ui: &mut egui::Ui) {
        let bind = self.service.as_ref().map_or_else(
            || self.settings.bind.clone(),
            |service| service.listen_ipv4().ip().to_string(),
        );
        let selected = console_interface(&bind, &self.ips, self.selected_ip)
            .and_then(|index| self.ips.get(index).cloned());
        if let Some(selected) = &selected {
            text(ui, "Адрес DNS для консоли", 15.0, MUTED);
            ui.add_space(4.0);
            ui.label(
                RichText::new(&selected.0)
                    .monospace()
                    .size(28.0)
                    .color(TEXT),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if button(
                    ui,
                    "Копировать",
                    ButtonStyle::Primary,
                    Vec2::new(112.0, 36.0),
                )
                .clicked()
                {
                    ui.ctx().copy_text(selected.0.clone());
                    self.notice = Some("Адрес скопирован".into());
                }
                if self.ips.len() <= 1 || self.settings.bind != "0.0.0.0" {
                    ui.add(
                        egui::Label::new(RichText::new(&selected.1).size(13.0).color(MUTED))
                            .truncate(),
                    )
                    .on_hover_text(&selected.1);
                }
            });
            if self.ips.len() > 1 && self.settings.bind == "0.0.0.0" {
                ui.add_enabled_ui(self.service.is_none(), |ui| {
                    egui::ComboBox::from_id_salt("interface")
                        .selected_text(&selected.1)
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for (index, (address, name)) in self.ips.iter().enumerate() {
                                ui.selectable_value(
                                    &mut self.selected_ip,
                                    index,
                                    format!("{name} · {address}"),
                                );
                            }
                        })
                        .response
                        .on_hover_text(format!(
                            "{}\n{}",
                            selected.1,
                            i18n::tr("Выберите подключение к той же сети, что и консоль.")
                        ));
                })
                .response
                .on_disabled_hover_text(i18n::tr("Для смены подключения остановите DNS."));
            }
        } else if self.settings.bind == "0.0.0.0" {
            text(ui, "Нет подключения к локальной сети", 14.0, TEXT);
            text(
                ui,
                "Подключите компьютер к Wi-Fi или Ethernet.",
                13.0,
                MUTED,
            );
        }
        let warnings = console_warnings(
            &bind,
            self.settings.port,
            selected.as_ref().map(|(address, _)| address.as_str()),
        );
        let recovery = match warnings {
            [Some(_), Some(_)] if !needs_lan_firewall(&self.settings) => {
                Some("В «Настройках» укажите IPv4 0.0.0.0 и порт UDP 53.")
            }
            [Some(_), Some(_)] => {
                Some("В «Настройках» укажите адрес компьютера или 0.0.0.0 и порт UDP 53.")
            }
            [Some(warning), None] | [None, Some(warning)] => Some(warning),
            [None, None] => None,
        };
        if let Some(recovery) = recovery {
            if selected.is_some() {
                ui.add_space(FIELD_GAP);
            }
            egui::Frame::new()
                .fill(Color32::from_rgb(58, 35, 39))
                .corner_radius(5)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    text(ui, "Нужна настройка для PlayStation", 14.0, ERROR);
                    text(ui, recovery, 13.0, TEXT);
                    if ui.link(i18n::tr("Открыть настройки DNS")).clicked() {
                        self.page = Page::Settings;
                        self.error = None;
                    }
                });
        }
        ui.add_space(16.0);
        self.dns_action(ui);
    }

    fn console_setup(&mut self, ui: &mut egui::Ui) {
        text(ui, "На PlayStation", 16.0, TEXT);
        ui.add_space(FIELD_GAP);
        text(
            ui,
            "В настройках сети выберите ручной DNS и введите адрес компьютера в оба поля.",
            14.0,
            MUTED,
        );
        ui.add_space(FIELD_GAP);
        egui::CollapsingHeader::new(i18n::tr("Подробная настройка"))
            .id_salt("console_help")
            .open(self.qa.as_ref().is_some_and(|qa| !qa.motion && matches!(qa.stage, 10 | 14)).then_some(true))
            .show(ui, |ui| {
                text(ui, "В настройках сети PS4 / PS5 выберите своё подключение и ручной DNS.", 14.0, TEXT);
                ui.add_space(6.0);
                text(ui, "Укажите адрес компьютера в основном и дополнительном DNS. IP / DHCP и MTU оставьте автоматически, прокси — не использовать.", 14.0, MUTED);
                ui.add_space(6.0);
                text(ui, "Компьютер должен оставаться включённым. При смене его IP обновите оба поля DNS на консоли.", 13.0, MUTED);
                if ui.link(i18n::tr("Настройки сети Windows")).clicked() && let Err(error) = platform::open_network_settings() {
                    self.error = Some(error);
                }
            });
    }

    fn rules(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Правила DNS", "");
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = COLUMN_GAP;
            ui.columns(2, |columns| {
                let left = &mut columns[0];
                left.spacing_mut().item_spacing.x = 12.0;
                let rules_label = left.label(
                    RichText::new(i18n::tr("Домены и IPv4"))
                        .size(14.0)
                        .color(TEXT),
                );
                left.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("rules_editor")
                    .max_height(235.0)
                    .show(left, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.editor.rules)
                                .font(egui::TextStyle::Monospace)
                                .id(egui::Id::new("rule_masks"))
                                .desired_width(f32::INFINITY)
                                .desired_rows(11)
                                .margin(egui::Margin::same(9)),
                        )
                        .labelled_by(rules_label.id);
                    });
                let right = &mut columns[1];
                right.spacing_mut().item_spacing.x = 12.0;
                let exceptions_label =
                    right.label(RichText::new(i18n::tr("Исключения")).size(14.0).color(TEXT));
                right.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("exceptions_editor")
                    .max_height(235.0)
                    .show(right, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.editor.exceptions)
                                .font(egui::TextStyle::Monospace)
                                .id(egui::Id::new("exception_masks"))
                                .desired_width(f32::INFINITY)
                                .desired_rows(11)
                                .margin(egui::Margin::same(9)),
                        )
                        .labelled_by(exceptions_label.id);
                    });
            });
        });
        ui.add_space(SECTION_GAP);
        egui::CollapsingHeader::new(i18n::tr("Синтаксис правил")).id_salt("rule_help")
            .open(self.qa.as_ref().is_some_and(|qa| qa.stage == 11).then_some(true))
            .show(ui, |ui| {
            text(ui, "домен=IPv4 · * — любые символы, ? — один. Правила проверяются сверху вниз.", 13.0, MUTED);
            text(ui, "Исключения — по одному домену на строку; они имеют приоритет и не подменяют IP.", 13.0, MUTED);
        });
    }

    fn save_bar(&mut self, ui: &mut egui::Ui) {
        let changed = self.editor.changed(&self.settings);
        if !changed {
            // Reserve the same space without invisible focusable controls.
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), 36.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_height(36.0);
                    if let Some(notice) = &self.notice {
                        text(ui, notice, 13.0, MINT);
                    }
                },
            );
            return;
        }
        let busy = self.firewall_check.is_some() || self.firewall_wait.is_some();
        ui.horizontal(|ui| {
            ui.add_enabled_ui(changed && !busy, |ui| {
                let response = button(
                    ui,
                    "Сохранить",
                    ButtonStyle::Primary,
                    Vec2::new(120.0, 36.0),
                );
                if let Some(qa) = &mut self.qa
                    && qa.stage == 19
                    && !qa.action_injected
                {
                    qa.action_position = Some(response.rect.center());
                }
                if response.clicked() {
                    self.save();
                }
            });
            ui.add_enabled_ui(changed, |ui| {
                let response = button(
                    ui,
                    "Отменить",
                    ButtonStyle::Secondary,
                    Vec2::new(104.0, 36.0),
                );
                if let Some(qa) = &mut self.qa
                    && qa.stage == 16
                    && !qa.action_injected
                {
                    qa.action_position = Some(response.rect.center());
                }
                if response.clicked() {
                    self.editor = Editor::new(&self.settings);
                    self.error = None;
                }
            });
            if busy {
                text(ui, "Дождитесь настройки брандмауэра", 12.0, MUTED);
            } else if changed {
                text(
                    ui,
                    if self.service.is_some() {
                        "DNS перезапустится после сохранения"
                    } else {
                        "Изменения не сохранены"
                    },
                    12.0,
                    MUTED,
                );
            }
        });
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Настройки DNS", "");
        ui.scope(|ui| {
            // Floating scrollbars must not cover field borders or switches.
            ui.set_max_width((ui.available_width() - 12.0).clamp(0.0, FORM_WIDTH));
            setting_row(
                ui,
                "IPv4-адрес прослушивания",
                |ui, label| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.editor.settings.bind)
                            .desired_width(f32::INFINITY)
                            .margin(egui::Margin::symmetric(8, 6)),
                    )
                    .labelled_by(label)
                    .on_hover_text(i18n::tr("0.0.0.0 — подключение, выбранное на вкладке «Подключение». Внутренняя сеть виртуальных машин не используется."));
                },
            );
            setting_row(ui, "Внешние DNS-серверы", |ui, label| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.editor.upstreams)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(3)
                        .margin(egui::Margin::same(9)),
                )
                .labelled_by(label)
                .on_hover_text(i18n::tr("По одному IP на строку. Серверы опрашиваются сверху вниз."));
            });
            ui.add_space(FIELD_GAP);
            egui::CollapsingHeader::new(i18n::tr("Дополнительные параметры"))
                .id_salt("network_advanced")
                .open(
                    self.qa
                        .as_ref()
                        .is_some_and(|qa| matches!(qa.stage, 12 | 13))
                        .then_some(true),
                )
                .show(ui, |ui| {
                    setting_row(ui, "Порт UDP", |ui, label| {
                        ui.add_sized(
                            [80.0, 30.0],
                            egui::DragValue::new(&mut self.editor.settings.port).range(1..=65535),
                        )
                        .labelled_by(label)
                        .on_hover_text(i18n::tr("Для подключения PlayStation используйте порт 53."));
                    });
                    setting_row(ui, "IPv6-адрес", |ui, label| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.editor.settings.bind6)
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(8, 6)),
                        )
                        .labelled_by(label)
                        .on_hover_text(i18n::tr("off — не запускать IPv6-слушатель."));
                    });
                    setting_row(ui, "Тайм-аут, мс", |ui, label| {
                        ui.add_sized(
                            [80.0, 30.0],
                            egui::DragValue::new(&mut self.editor.settings.timeout_ms)
                                .range(100..=10000)
                                .speed(50),
                        )
                        .labelled_by(label)
                        .on_hover_text(i18n::tr("Общий тайм-аут одного DNS-запроса."));
                    });
                });
            ui.add_space(SECTION_GAP);
            text(ui, "Приложение", 16.0, TEXT);
            ui.add_space(FIELD_GAP);
            switch_row(
                ui,
                &mut self.editor.settings.start_dns,
                "Включать DNS при запуске",
            );
            switch_row(
                ui,
                &mut self.editor.settings.close_to_tray,
                "Сворачивать окно в трей при закрытии",
            );
            ui.add_space(FIELD_GAP);
            egui::CollapsingHeader::new(i18n::tr("Файлы и версия"))
                .id_salt("app_about")
                .open(
                    self.qa
                        .as_ref()
                        .is_some_and(|qa| matches!(qa.stage, 12 | 13))
                        .then_some(true),
                )
                .show(ui, |ui| {
                    if ui.link(i18n::tr("Открыть папку настроек и журнала")).clicked()
                        && let Err(error) = platform::open_folder(&self.dir)
                    {
                        self.error = Some(error);
                    }
                    ui.horizontal_wrapped(|ui| {
                        text(
                            ui,
                            format!("nanoDNS {} · GPL-3.0", env!("CARGO_PKG_VERSION")),
                            12.0,
                            MUTED,
                        );
                        if ui.link(i18n::tr("Исходный nanoDNS")).clicked()
                            && let Err(error) =
                                platform::open_url("https://github.com/drakmor/nanoDNS")
                        {
                            self.error = Some(error);
                        }
                    });
                });
        });
    }

    fn journal(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Журнал DNS", "");
        if let Some(service) = &self.service {
            text(
                ui,
                format!(
                    "Всего {} · Подмена {} · Внешний DNS {} · Ошибки {} · {} мин работы",
                    service.stats.total.load(Ordering::Relaxed),
                    service.stats.blocked.load(Ordering::Relaxed),
                    service.stats.forwarded.load(Ordering::Relaxed),
                    service.stats.failed.load(Ordering::Relaxed),
                    service.started.elapsed().as_secs() / 60
                ),
                13.0,
                MUTED,
            );
            ui.add_space(8.0);
        }
        if journal_search_row(ui, &mut self.search, !self.log.is_empty(), i18n::language()).inner {
            self.log.clear();
        }
        ui.add_space(SECTION_GAP);
        if self.log.is_empty() {
            text(ui, "Запросов пока нет.", 14.0, TEXT);
            text(
                ui,
                if self.service.is_some() {
                    "DNS работает. Укажите адрес компьютера в настройках DNS консоли."
                } else {
                    "Запустите DNS кнопкой вверху окна."
                },
                13.0,
                MUTED,
            );
            return;
        }
        let search = self.search.to_ascii_lowercase();
        let has_matches = self
            .log
            .iter()
            .any(|r| search.is_empty() || r.domain.contains(&search) || r.client.contains(&search));
        if !has_matches {
            text(ui, "Ничего не найдено.", 14.0, TEXT);
            if ui.link(i18n::tr("Сбросить поиск")).clicked() {
                self.search.clear();
            }
            return;
        }
        // Keep columns stable for sparse and long-content logs alike. The
        // domain gets the remaining width; full values remain in tooltips.
        let row_height = 32.0;
        let width = ui.available_width() - 12.0;
        let columns = [72.0, (width - 314.0).max(100.0), 44.0, 150.0];
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            for (title, width) in ["Время UTC", "Домен", "Тип", "Результат"]
                .into_iter()
                .zip(columns)
            {
                journal_cell(
                    ui,
                    width,
                    22.0,
                    RichText::new(i18n::tr(title)).size(12.0).color(MUTED),
                );
            }
        });
        let rows: Vec<_> = self
            .log
            .iter()
            .rev()
            .filter(|row| {
                search.is_empty() || row.domain.contains(&search) || row.client.contains(&search)
            })
            .collect();
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("query_log")
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(80.0))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded);
        if self.qa.as_ref().is_some_and(|qa| qa.stage == 18) {
            let spacing = ui.spacing().item_spacing.y;
            let content_height = rows.len() as f32 * (row_height + spacing) - spacing;
            scroll =
                scroll.vertical_scroll_offset((content_height - ui.available_height()).max(0.0));
        }
        scroll.show_rows(ui, row_height, rows.len(), |ui, range| {
                for (index, row) in rows.iter().enumerate().take(range.end).skip(range.start) {
                    let background = ui.painter().add(egui::Shape::Noop);
                    let response = ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 16.0;
                        journal_cell(ui, columns[0], row_height, RichText::new(&row.time).monospace().size(12.0).color(MUTED));
                        journal_cell(ui, columns[1], row_height, RichText::new(&row.domain).monospace().size(13.0).color(TEXT))
                        .on_hover_text(format!("{}\n{} {}", row.domain, i18n::tr("Клиент:"), row.client));
                        journal_cell(ui, columns[2], row_height, RichText::new(&row.kind).size(13.0).color(MUTED));
                        let (result, explanation) = match row.result.as_str() {
                            "NODATA" => ("Нет записи", "NODATA — для этого типа запроса запись не возвращается. Это ожидаемый ответ правила."),
                            "SERVFAIL" => ("Ошибка DNS", "SERVFAIL — DNS не смог обработать запрос."),
                            value => (value, value),
                        };
                        journal_cell(ui, columns[3], row_height, RichText::new(i18n::tr(result)).size(13.0).color(
                            if row.failed { ERROR } else if row.blocked { MINT } else { MUTED },
                        )).on_hover_text(i18n::tr(explanation));
                    });
                    let rect = egui::Rect::from_min_size(response.response.rect.min, Vec2::new(width, row_height));
                    let alpha = if ui.rect_contains_pointer(rect) { 14 } else if index % 2 == 0 { 6 } else { 0 };
                    ui.painter().set(background, egui::Shape::rect_filled(rect, 3, Color32::from_white_alpha(alpha)));
                }
            });
    }

    fn request_firewall(&mut self) {
        if self.qa.is_some() {
            return;
        }
        match platform::allow_firewall(&self.dir, self.settings.port) {
            Ok(()) => {
                self.firewall_wait = Some(Instant::now());
                self.firewall_status = "Подтвердите запрос администратора Windows".into();
            }
            Err(error) => {
                self.firewall_status = "Доступ не настроен".into();
                self.error = Some(error);
            }
        }
    }

    fn poll_firewall(&mut self, ctx: &egui::Context) {
        let previous_error = self.error.clone();
        let result = self.firewall_check.as_ref().and_then(|r| r.try_recv().ok());
        if let Some(result) = result {
            self.firewall_check = None;
            match result {
                Ok(true) => self.firewall_status = "Доступ разрешён · UDP · локальная сеть".into(),
                Ok(false) => self.request_firewall(),
                Err(error) => {
                    self.firewall_status = "Доступ не проверен".into();
                    self.error = Some(error);
                }
            }
        }
        if let Some(start) = self.firewall_wait {
            let path = self.dir.join("firewall-result.txt");
            if let Ok(result) = fs::read_to_string(&path) {
                self.firewall_wait = None;
                if result.trim_start_matches('\u{feff}').trim() == "OK" {
                    self.firewall_status = "Доступ разрешён · UDP · локальная сеть".into();
                    self.notice = Some("Брандмауэр настроен автоматически".into());
                } else {
                    self.firewall_status = "Доступ не настроен".into();
                    self.error = Some(result);
                }
            } else if start.elapsed() > Duration::from_secs(45) {
                self.firewall_wait = None;
                self.firewall_status = "Доступ не настроен".into();
                self.error = Some(
                    "Windows не подтвердила настройку порта. Нажмите «Повторить настройку».".into(),
                );
            }
        }
        if self.error.is_some() && self.error != previous_error {
            self.show_window(ctx);
        }
    }

    fn qa_start_and_query(&mut self) -> Result<(), String> {
        // QA uses loopback and a temporary profile. It never provisions the firewall or startup.
        let reservation = std::net::UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let address = reservation.local_addr().map_err(|e| e.to_string())?;
        drop(reservation);
        self.settings.bind = "127.0.0.1".into();
        self.settings.port = address.port();
        self.settings.bind6 = "off".into();
        self.start();
        if self.service.is_none() {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "QA: DNS did not start".into()));
        }
        thread::spawn(move || {
            if let Ok(socket) = std::net::UdpSocket::bind("127.0.0.1:0") {
                let _ = socket.set_read_timeout(Some(Duration::from_millis(300)));
                for (host, kind) in [
                    ("update.playstation.net", 1_u16),
                    ("download.playstation.com", 1),
                    ("cdn.psndl.net", 28),
                ] {
                    let mut packet = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
                    for label in host.split('.') {
                        let Ok(length) = u8::try_from(label.len()) else {
                            return;
                        };
                        packet.push(length);
                        packet.extend_from_slice(label.as_bytes());
                    }
                    packet.push(0);
                    packet.extend_from_slice(&kind.to_be_bytes());
                    packet.extend_from_slice(&1_u16.to_be_bytes());
                    if socket.send_to(&packet, address).is_ok() {
                        let mut buf = [0; 4096];
                        let _ = socket.recv_from(&mut buf);
                    }
                }
            }
        });
        Ok(())
    }

    fn qa_motion_frame(&mut self, ctx: &egui::Context) {
        let background_draws = self
            .background_renderer
            .as_ref()
            .map_or(0, background::Renderer::paint_count);
        let background_renderer = if self.background_renderer.is_some() {
            "gpu-per-pixel"
        } else {
            "mesh-fallback"
        };
        let Some(qa) = &mut self.qa else {
            return;
        };
        // Allow the real OS window to be activated before recording. Never
        // substitute synthetic focus for the production visibility policy.
        if qa.stage == 0
            && !ctx.input(|i| i.viewport().focused.unwrap_or(false))
            && qa.started.elapsed() < Duration::from_secs(20)
        {
            return;
        }
        if qa.stage >= 32 {
            let report = serde_json::json!({
                "native_captures": qa.stage,
                "wall_seconds": qa.started.elapsed().as_secs_f64(),
                "samples": qa.motion_samples,
                "firewall_mutated": false, "startup_mutated": false,
                "production_motion_clock": true,
                "forced_focus": false,
                "background_renderer": background_renderer,
                "background_draws": background_draws,
                "background_renderer_error": self.background_renderer_error.as_deref(),
            });
            if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
                let _ = fs::write(qa.dir.join("motion-report.json"), bytes);
            }
            self.stop();
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let screenshot = ctx.input(|i| {
            i.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = screenshot {
            let bytes: Vec<_> = image
                .pixels
                .iter()
                .flat_map(|pixel| pixel.to_array())
                .collect();
            if let (Ok(width), Ok(height)) =
                (u32::try_from(image.size[0]), u32::try_from(image.size[1]))
                && let Err(error) = image::save_buffer(
                    qa.dir.join(format!("frame-{:03}.png", qa.stage)),
                    &bytes,
                    width,
                    height,
                    image::ColorType::Rgba8,
                )
            {
                self.error = Some(error.to_string());
            }
            if let Some(sample) = qa.pending_motion_sample.take() {
                qa.motion_samples.push(sample);
            }
            qa.stage += 1;
            qa.waiting = false;
            qa.wait_until = Instant::now() + Duration::from_millis(250);
        }
        if !qa.waiting && Instant::now() >= qa.wait_until {
            let frame_nr = ctx.cumulative_frame_nr();
            qa.pending_motion_sample = Some(ctx.input(|i| {
                serde_json::json!({
                    "frame": qa.stage, "seconds": qa.started.elapsed().as_secs_f64(),
                    "egui_frame_nr": frame_nr,
                    "phase": self.background.phase,
                    "page": match self.page {
                        Page::Overview => "connection",
                        Page::Rules => "rules",
                        Page::Settings => "settings",
                        Page::Journal => "journal",
                    },
                    "viewport_focused": i.viewport().focused,
                    "input_focused": i.focused,
                    "minimized": i.viewport().minimized,
                    "hidden": self.hidden,
                    "background_renderer": background_renderer,
                    "background_draws": background_draws,
                })
            }));
            qa.waiting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }

    fn qa_frame(&mut self, ctx: &egui::Context) {
        if self.qa.as_ref().is_some_and(|qa| qa.motion) {
            self.qa_motion_frame(ctx);
            return;
        }
        const CAPTURES: [&str; 20] = [
            "overview",
            "rules",
            "settings",
            "journal",
            "settings-dirty",
            "bind-error",
            "journal-live",
            "overview-running",
            "focus",
            "search-empty",
            "setup-expanded",
            "rules-help-expanded",
            "settings-expanded",
            "settings-bottom",
            "setup-bottom",
            "rules-dirty",
            "rules-cancelled",
            "journal-long",
            "journal-bottom",
            "settings-saved",
        ];
        let Some(qa) = &self.qa else {
            return;
        };
        let stage = qa.stage;
        let dir = qa.dir.clone();
        if stage >= CAPTURES.len() {
            let Some(result) = qa
                .window_restore
                .as_ref()
                .and_then(|receiver| receiver.try_recv().ok())
            else {
                return;
            };
            let (restored, restore_error) = match result {
                Ok(hidden_before_restore) => (hidden_before_restore, None),
                Err(error) => (false, Some(error)),
            };
            let count = self
                .service
                .as_ref()
                .map(|s| s.stats.total.load(Ordering::Relaxed))
                .unwrap_or(0)
                .max(qa.queries_before_save);
            let (corner_preference, corner_error) = match self
                .main_window
                .map(|window| window.rounded_corner_preference())
            {
                Some(Ok(preference)) => (preference, None),
                Some(Err(error)) => (None, Some(error)),
                None => (None, None),
            };
            let report = serde_json::json!({"native_captures":CAPTURES.len(),"tray_created":self.tray.is_some(),"real_loopback_queries":count,"firewall_mutated":false,"startup_mutated":false,"native_hide_restore":restored,"native_hide_restore_error":restore_error,"dwm_round_corner_preference":corner_preference,"dwm_round_corner_error":corner_error,"cancel_via_egui_widget":qa.cancel_verified,"save_via_egui_widget":qa.save_verified,"synthetic_long_log_rows":300,"viewport_points":[ctx.content_rect().width(),ctx.content_rect().height()]});
            if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
                let _ = fs::write(dir.join("ui-state-report.json"), bytes);
            }
            self.stop();
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let screenshot = ctx.input(|i| {
            i.events.iter().find_map(|e| {
                if let egui::Event::Screenshot { image, .. } = e {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = screenshot {
            if let Some(filename) = CAPTURES.get(stage) {
                let bytes = image
                    .pixels
                    .iter()
                    .flat_map(|p| p.to_array())
                    .collect::<Vec<_>>();
                if let (Ok(width), Ok(height)) =
                    (u32::try_from(image.size[0]), u32::try_from(image.size[1]))
                    && let Err(error) = image::save_buffer(
                        dir.join(format!("{filename}.png")),
                        &bytes,
                        width,
                        height,
                        image::ColorType::Rgba8,
                    )
                {
                    self.error = Some(error.to_string());
                }
            }
            let next = stage + 1;
            if let Some(qa) = &mut self.qa {
                qa.stage = next;
                qa.waiting = false;
                qa.action_position = None;
                qa.action_injected = false;
                qa.wait_until = Instant::now() + Duration::from_millis(450);
            }
            match next {
                1 => self.page = Page::Rules,
                2 => self.page = Page::Settings,
                3 => self.page = Page::Journal,
                4 => {
                    self.page = Page::Settings;
                    self.editor.settings.timeout_ms = 2200;
                }
                5 => {
                    self.page = Page::Overview;
                    if let Ok(occupied) = std::net::UdpSocket::bind("127.0.0.1:0")
                        && let Ok(address) = occupied.local_addr()
                    {
                        let settings = Settings {
                            bind: "127.0.0.1".into(),
                            port: address.port(),
                            ..self.settings.clone()
                        };
                        self.error = Service::start(&settings, &self.dir.join("qa-busy.log")).err();
                    }
                }
                6 => {
                    self.page = Page::Journal;
                    self.error = None;
                    if let Err(error) = self.qa_start_and_query() {
                        self.error = Some(error);
                    }
                }
                7 | 8 => self.page = Page::Overview,
                9 => {
                    self.page = Page::Journal;
                    self.search = "no-match.invalid".into();
                }
                10 => self.page = Page::Overview,
                11 => self.page = Page::Rules,
                12 | 13 => self.page = Page::Settings,
                14 => self.page = Page::Overview,
                15 => {
                    self.page = Page::Rules;
                    self.editor = Editor::new(&self.settings);
                    self.editor.rules.push_str("\nqa.example=0.0.0.0");
                    self.error = None;
                    self.notice = None;
                }
                16 => {}
                17 => {
                    self.page = Page::Journal;
                    self.search.clear();
                    self.log.clear();
                    // Explicit QA data covers truncation, DNS errors and the
                    // final virtualized row; it never enters the disk journal.
                    for number in 0..300 {
                        self.log.push_back(QueryEvent {
                            time: format!("12:{:02}:{:02}", number / 60, number % 60),
                            domain: format!(
                                "{number:03}.{}.{}.{}.example",
                                "long-segment".repeat(3),
                                "console-download".repeat(3),
                                "service".repeat(6)
                            ),
                            kind: if number % 2 == 0 { "AAAA" } else { "A" }.into(),
                            result: if number % 7 == 0 {
                                "SERVFAIL"
                            } else {
                                "203.0.113.7"
                            }
                            .into(),
                            client: "192.0.2.10:53530".into(),
                            blocked: false,
                            failed: number % 7 == 0,
                        });
                    }
                }
                18 => {}
                19 => {
                    self.page = Page::Settings;
                    self.editor = Editor::new(&self.settings);
                    self.editor.settings.timeout_ms = 2300;
                    if let Some(qa) = &mut self.qa {
                        qa.queries_before_save = self
                            .service
                            .as_ref()
                            .map(|s| s.stats.total.load(Ordering::Relaxed))
                            .unwrap_or(0);
                    }
                }
                next if next == CAPTURES.len() => {
                    let (sender, receiver) = mpsc::channel();
                    if let Some(qa) = &mut self.qa {
                        qa.window_restore = Some(receiver);
                    }
                    if let Some(window) = self.main_window {
                        self.hide_window(ctx);
                        let ctx = ctx.clone();
                        thread::spawn(move || {
                            // Give the preceding frame time to apply Visible(false).
                            // This one-shot QA check has a strict two-second bound.
                            thread::sleep(Duration::from_millis(350));
                            let result = (|| {
                                let hidden = !window.visible()?;
                                window.restore()?;
                                let deadline = Instant::now() + Duration::from_secs(2);
                                while !window.visible()? {
                                    if Instant::now() >= deadline {
                                        return Err("Native window restore timed out".into());
                                    }
                                    thread::sleep(Duration::from_millis(20));
                                }
                                Ok(hidden)
                            })();
                            let _ = sender.send(result);
                            ctx.request_repaint();
                        });
                    } else {
                        let _ = sender.send(Err("Native window handle unavailable".into()));
                        ctx.request_repaint();
                    }
                }
                _ => {}
            }
        }
        let clean = !self.editor.changed(&self.settings) && self.error.is_none();
        if let Some(qa) = &mut self.qa
            && qa.action_injected
            && clean
        {
            if qa.stage == 16 {
                qa.cancel_verified = true;
            }
            if qa.stage == 19 {
                qa.save_verified = true;
            }
        }
        if let Some(qa) = &mut self.qa
            && !qa.waiting
            && qa.stage < CAPTURES.len()
            && Instant::now() >= qa.wait_until
            && (!matches!(qa.stage, 16 | 19) || (qa.action_injected && clean))
        {
            qa.waiting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }
}

impl eframe::App for App {
    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let (Some(renderer), Some(gl)) = (self.background_renderer.take(), gl)
            && let Err(error) = renderer.destroy(gl)
        {
            eprintln!("Background renderer cleanup: {error}");
        }
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        // QA verifies the widget focus treatment even when its native window is in the background.
        if self
            .qa
            .as_ref()
            .is_some_and(|qa| !qa.motion && matches!(qa.stage, 8 | 16 | 19))
        {
            input.focused = true;
        }
        if let Some(qa) = &mut self.qa
            && let Some(pos) = qa.action_position.take()
        {
            input.events.push(egui::Event::PointerMoved(pos));
            for pressed in [true, false] {
                input.events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                });
            }
            qa.action_injected = true;
        }
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut show_requested = false;
        while let Ok(action) = self.tray_rx.try_recv() {
            platform::lifecycle_trace(format_args!(
                "action={} running={} hidden={} quitting={}",
                match &action {
                    TrayAction::Show => "show",
                    TrayAction::Toggle => "toggle",
                    TrayAction::Quit => "quit",
                    TrayAction::RestoreError(_) => "restore-error",
                },
                self.service.is_some(),
                self.hidden,
                self.quitting
            ));
            match action {
                TrayAction::Show if !self.quitting => {
                    show_requested = true;
                    self.show_window(ctx);
                }
                TrayAction::Toggle if !self.quitting => {
                    self.toggle();
                    if self.error.is_some() {
                        self.show_window(ctx);
                    }
                }
                TrayAction::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                TrayAction::RestoreError(error) => self.error = Some(error),
                TrayAction::Show | TrayAction::Toggle => {}
            }
        }
        // Requests enter through the native Windows message queue, after any
        // earlier tray Quit. Each caller owns a separate acknowledgement event.
        let mut activations = Vec::new();
        for _ in 0..32 {
            let Some(activation) = self._instance.try_activation() else {
                break;
            };
            activations.push(activation);
        }
        if !activations.is_empty() && !self.quitting {
            show_requested = true;
            self.show_window(ctx);
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            if self.settings.close_to_tray && self.tray.is_some() && self.main_window.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                // Close can remain queued while Windows omits a hidden paint.
                // An explicit reopen in this pass takes priority over hiding
                // again; a real Quit/exit still wins above and below.
                if !show_requested {
                    self.hide_window(ctx);
                    activations.clear();
                }
            } else {
                self.quitting = true;
            }
        }
        if self.quitting {
            if let Err(error) = self._instance.begin_shutdown() {
                eprintln!("{error}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !self.hidden {
            for activation in activations {
                if let Err(error) = self._instance.acknowledge(activation) {
                    self.error = Some(error);
                }
            }
        }
        if let Some(service) = &self.service {
            while let Ok(event) = service.events.try_recv() {
                if self.log.len() >= 300 {
                    self.log.pop_front();
                }
                self.log.push_back(event);
            }
            if !service.healthy() {
                self.stop();
                self.error=Some("DNS остановился из-за ошибки сети. Проверьте подключение и запустите его снова.".into());
                self.show_window(ctx);
            }
        }
        self.poll_firewall(ctx);
        let active = !self.hidden && !ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if active
            && self.service.is_some()
            && self.interfaces_refreshed.elapsed() >= Duration::from_secs(30)
        {
            self.refresh_interfaces();
        }
        let motion_delay = background_delay(active, !platform::client_animations_enabled());
        let phase = if let Some(qa) = &self.qa
            && !qa.motion
        {
            // Stable native captures include two alternate light positions.
            match qa.stage {
                17 => std::f32::consts::FRAC_PI_4,
                18 => std::f32::consts::FRAC_PI_2,
                _ => 0.0,
            }
        } else {
            self.background.tick(Instant::now(), motion_delay.is_some())
        };
        console_background(ctx, phase, self.background_renderer.as_ref());
        self.chrome(ctx);
        if matches!(self.page, Page::Rules | Page::Settings) {
            egui::TopBottomPanel::bottom("apply_bar")
                .show_separator_line(false)
                .exact_height(60.0)
                .frame(
                    egui::Frame::new()
                        .fill(Color32::TRANSPARENT)
                        .inner_margin(egui::Margin::symmetric(PAGE_INLINE, 12)),
                )
                .show(ctx, |ui| {
                    ui.set_max_width(ui.available_width().min(CONTENT_WIDTH));
                    self.save_bar(ui);
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(egui::Margin::symmetric(PAGE_INLINE, 24)),
            )
            .show(ctx, |ui| {
                ui.set_max_width(ui.available_width().min(CONTENT_WIDTH));
                let scroll = egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    );
                let journal = self.page == Page::Journal;
                let mut content = |ui: &mut egui::Ui| {
                        if let Some(error) = &self.error {
                            egui::Frame::new()
                                .fill(Color32::from_rgb(58, 35, 39))
                                .corner_radius(5)
                                .inner_margin(10)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    let recovery = if error.contains("os error 10048") {
                                        Some((
                                            "Порт DNS занят другой программой",
                                            "Остановите другой DNS-сервис и нажмите «Запустить DNS» снова.",
                                        ))
                                    } else if error.contains("os error 10049") {
                                        Some((
                                            "Адрес прослушивания недоступен",
                                            "Проверьте IPv4 в «Настройках»: укажите адрес компьютера или 0.0.0.0. Затем запустите DNS снова.",
                                        ))
                                    } else {
                                        None
                                    };
                                    if let Some((title, instruction)) = recovery {
                                        text(ui, title, 14.0, ERROR);
                                        text(ui, instruction, 13.0, TEXT);
                                        egui::CollapsingHeader::new(i18n::tr("Технические подробности"))
                                            .id_salt("bind_error_details")
                                            .show(ui, |ui| text(ui, error, 12.0, MUTED));
                                    } else {
                                        text(ui, error, 13.0, ERROR);
                                    }
                                });
                            ui.add_space(12.0);
                        }
                        if !matches!(self.page, Page::Rules | Page::Settings)
                            && let Some(notice) = &self.notice
                        {
                            text(ui, notice, 13.0, MINT);
                            ui.add_space(9.0);
                        }
                        match self.page {
                            Page::Overview => self.overview(ui),
                            Page::Rules => self.rules(ui),
                            Page::Settings => self.settings(ui),
                            Page::Journal => self.journal(ui),
                        }
                        if self.page != Page::Journal {
                            ui.add_space(SECTION_GAP);
                        }
                        if self.qa.as_ref().is_some_and(|qa| !qa.motion && matches!(qa.stage, 13 | 14)) {
                            // Scroll to the actual laid-out content, so the saved QA offset is bounded.
                            ui.scroll_to_cursor(Some(egui::Align::Max));
                        }
                    };
                if journal {
                    // Journal owns its virtualized scroll area. Nesting it in
                    // the page scroller creates a second bar and shifts rows.
                    content(ui);
                } else {
                    scroll.show(ui, content);
                }
            });
        self.qa_frame(ctx);
        window_resize(ctx);
        if self.qa.as_ref().is_none_or(|qa| qa.motion)
            && let Some(delay) = motion_delay
        {
            ctx.request_repaint_after(delay);
        }
        // Hidden Windows redraw deadlines can leave eframe polling for a frame
        // that never arrives. Tray callbacks restore natively before waking us;
        // health, log and setup status catch up when the window is shown again.
        if let Some(delay) = repaint_delay(
            self.service.is_some(),
            active,
            self.qa.is_some() || self.firewall_check.is_some() || self.firewall_wait.is_some(),
        ) {
            ctx.request_repaint_after(delay);
        }
    }
}

pub fn run() -> Result<(), String> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let mut dir = platform::data_dir()?;
    let mut qa = None;
    let mut tray_start = false;
    let mut headless = false;
    let mut run_for = 1500_u64;
    let mut qa_small = false;
    let mut qa_motion = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].to_str() {
            Some("--data-dir") => {
                index += 1;
                dir = PathBuf::from(args.get(index).ok_or("Не указана папка настроек.")?);
            }
            Some("--qa") | Some("--qa-motion") => {
                qa_motion = args[index] == "--qa-motion";
                index += 1;
                qa = Some(PathBuf::from(
                    args.get(index).ok_or("Не указана папка снимков.")?,
                ));
            }
            Some("--tray") => tray_start = true,
            Some("--headless") => headless = true,
            Some("--qa-small") => qa_small = true,
            Some("--run-for-ms") => {
                index += 1;
                run_for = args
                    .get(index)
                    .and_then(|s| s.to_str())
                    .ok_or("Не указано время запуска")?
                    .parse()
                    .map_err(|_| "Неверное время запуска")?;
            }
            _ => return Err("Неизвестный параметр запуска.".into()),
        }
        index += 1;
    }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let instance = match instance::acquire(&dir, headless, tray_start)? {
        instance::Launch::Primary(instance) => instance,
        instance::Launch::Activated => return Ok(()),
    };
    let settings = Settings::load(&dir.join("settings.json"))?;
    if headless {
        let service = Service::start(&settings, &dir.join("nanodns.log"))?;
        thread::sleep(Duration::from_millis(run_for.min(60_000)));
        let stats = serde_json::json!({"total":service.stats.total.load(Ordering::Relaxed),"blocked":service.stats.blocked.load(Ordering::Relaxed),"forwarded":service.stats.forwarded.load(Ordering::Relaxed),"failed":service.stats.failed.load(Ordering::Relaxed)});
        let mut file = File::create(dir.join("smoke-stats.json")).map_err(|e| e.to_string())?;
        write!(file, "{stats}").map_err(|e| e.to_string())?;
        return Ok(());
    }
    if let Some(path) = &qa {
        fs::create_dir_all(path).map_err(|e| e.to_string())?;
    }
    let icon = image::load_from_memory(include_bytes!("../assets/nanodns.png"))
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let icon = egui::IconData {
        rgba: icon.into_raw(),
        width: 256,
        height: 256,
    };
    let hook = instance.hook();
    let options = eframe::NativeOptions {
        centered: true,
        event_loop_builder: Some(Box::new(move |builder| {
            builder.with_msg_hook(move |message| {
                // SAFETY: winit supplies a live, aligned Win32 MSG pointer only
                // for this callback; the hook reads it without retaining it.
                unsafe { hook.on_message(message) }
            });
        })),
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_resizable(true)
            .with_inner_size(if qa_small {
                [720.0, 620.0]
            } else {
                [900.0, 660.0]
            })
            .with_min_inner_size([720.0, 620.0])
            .with_icon(icon),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "nanoDNS Desktop",
        options,
        Box::new(move |cc| {
            App::new(cc, settings, dir, instance, tray_start, qa, qa_motion)
                .map(|app| Box::new(app) as Box<dyn eframe::App>)
                .map_err(Box::<dyn std::error::Error + Send + Sync>::from)
        }),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        console_interface, console_warnings, interface_rank, needs_lan_firewall, repaint_delay,
        resize_direction,
    };
    use crate::config::Settings;
    use eframe::egui;
    use std::time::Duration;

    #[test]
    fn background_stops_when_hidden_or_reduced_and_keeps_visible_motion() {
        for state in [(false, false), (false, true), (true, true)] {
            assert!(super::background_delay(state.0, state.1).is_none());
        }
        assert_eq!(
            super::background_delay(true, false),
            Some(Duration::from_millis(33))
        );
    }

    #[test]
    fn long_translated_journal_actions_fit_actual_widget_bounds() {
        for language in [crate::i18n::Language::Bg, crate::i18n::Language::El] {
            let ctx = egui::Context::default();
            super::theme(&ctx);
            for width in [720.0, 900.0] {
                for has_rows in [false, true] {
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 620.0),
                        )),
                        ..Default::default()
                    };
                    let _ = ctx.run(input, |ctx| {
                        egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(32)).show(ctx, |ui| {
                            let bounds = ui.max_rect();
                            let mut search = String::new();
                            let row = super::journal_search_row(ui, &mut search, has_rows, language);
                            assert!(bounds.expand(0.5).contains_rect(row.response.rect),
                                "{} journal action escaped content at width {width}: {:?} outside {:?}",
                                language.tag(), row.response.rect, bounds);
                        });
                    });
                }
            }
        }
    }

    #[test]
    fn background_resume_preserves_phase_without_catching_up_hidden_time() {
        let now = std::time::Instant::now();
        let mut motion = super::BackgroundMotion::default();
        assert_eq!(motion.tick(now, true), 0.0);
        let before_hide = motion.tick(now + Duration::from_millis(100), true);
        assert!(before_hide > 0.0);
        assert_eq!(
            motion.tick(now + Duration::from_secs(60), false),
            before_hide
        );
        assert_eq!(
            motion.tick(now + Duration::from_secs(3600), false),
            before_hide
        );
        assert_eq!(
            motion.tick(now + Duration::from_secs(7200), true),
            before_hide
        );
        assert!(
            motion.tick(
                now + Duration::from_secs(7200) + Duration::from_millis(100),
                true
            ) > before_hide
        );
    }

    #[test]
    fn app_palette_stays_dark_when_windows_reports_light_theme() {
        let ctx = egui::Context::default();
        let light_input = || egui::RawInput {
            system_theme: Some(egui::Theme::Light),
            ..Default::default()
        };
        let _ = ctx.run(light_input(), |_| {});
        super::theme(&ctx);
        let _ = ctx.run(light_input(), |ctx| {
            assert_eq!(ctx.theme(), egui::Theme::Dark);
            let style = ctx.style();
            assert!(style.visuals.dark_mode);
            assert_eq!(style.visuals.override_text_color, Some(super::TEXT));
            assert_eq!(style.visuals.weak_text_color, Some(super::MUTED));
            assert_eq!(
                style.visuals.widgets.noninteractive.bg_stroke.color,
                super::LINE
            );
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut checked = false;
                ui.checkbox(&mut checked, "Default checkbox");
                egui::CollapsingHeader::new("Default disclosure").show(ui, |_| {});
                let mut field = String::new();
                ui.add(egui::TextEdit::singleline(&mut field).hint_text("Placeholder"));
                super::switch_row(ui, &mut checked, "App switch");
            });
        });
    }

    #[test]
    fn stopped_ui_sleeps_until_input_but_running_dns_keeps_health_checks() {
        assert_eq!(repaint_delay(false, true, false), None);
        assert_eq!(repaint_delay(false, false, false), None);
        assert_eq!(
            repaint_delay(true, true, false),
            Some(Duration::from_secs(1))
        );
        assert_eq!(repaint_delay(true, false, false), None);
    }

    #[test]
    fn setup_and_qa_poll_only_while_visible() {
        for running in [false, true] {
            for active in [false, true] {
                assert_eq!(
                    repaint_delay(running, active, true),
                    active.then_some(Duration::from_millis(200))
                );
            }
        }
    }

    #[test]
    fn loopback_only_dns_skips_firewall_but_any_lan_listener_requires_it() {
        for (bind, bind6, expected) in [
            ("127.0.0.1", "off", false),
            ("127.0.0.2", "::1", false),
            ("127.0.0.1", "::ffff:127.0.0.1", false),
            ("0.0.0.0", "off", true),
            ("192.168.1.10", "off", true),
            ("127.0.0.1", "::", true),
            ("127.0.0.1", "fd00::10", true),
        ] {
            let settings = Settings {
                bind: bind.into(),
                bind6: bind6.into(),
                ..Settings::default()
            };
            assert_eq!(needs_lan_firewall(&settings), expected, "{bind} / {bind6}");
        }
    }

    #[test]
    fn automatic_listener_uses_selected_lan_and_preserves_saved_settings() {
        let settings = Settings {
            bind6: "::1".into(),
            ..Settings::default()
        };
        let original = settings.clone();
        let ips = vec![
            ("192.0.2.10".into(), "Wi-Fi".into()),
            ("198.51.100.20".into(), "vEthernet (External-Direct)".into()),
            ("172.31.128.1".into(), "vEthernet (Default Switch)".into()),
        ];
        let runtime = super::listener_settings(&settings, &ips, 1).unwrap();
        assert_eq!(runtime.bind, "198.51.100.20");
        assert_eq!(runtime.bind6, "::1", "IPv6 is an explicit separate setting");
        assert_eq!(
            settings, original,
            "runtime binding must not persist a DHCP address"
        );
        assert!(
            super::listener_settings(&settings, &ips, 2).is_err(),
            "automatic mode must not take over VM gateway DNS"
        );
        assert!(super::listener_settings(&settings, &[], 0).is_err());
        assert!(
            super::listener_settings(&settings, &ips, 9).is_err(),
            "a vanished selection must not silently switch networks"
        );
    }

    #[test]
    fn explicit_listener_does_not_require_or_follow_the_adapter_picker() {
        for bind in ["127.0.0.1", "192.0.2.30"] {
            let settings = Settings {
                bind: bind.into(),
                ..Settings::default()
            };
            assert_eq!(
                super::listener_settings(&settings, &[], 4).unwrap(),
                settings
            );
        }
    }

    #[test]
    fn specific_bind_uses_only_its_real_interface() {
        let ips = vec![
            ("192.168.1.10".into(), "Wi-Fi".into()),
            ("10.0.0.10".into(), "Ethernet".into()),
        ];
        assert_eq!(console_interface("0.0.0.0", &ips, 1), Some(1));
        assert_eq!(console_interface("10.0.0.10", &ips, 0), Some(1));
        assert_eq!(console_interface("127.0.0.1", &ips, 0), None);
        assert_eq!(console_interface("192.168.1.99", &ips, 0), None);
        assert_eq!(console_interface("0.0.0.0", &[], 0), None);
    }

    #[test]
    fn lan_addresses_rank_before_isolated_switches_and_vpn_without_hiding_any() {
        let mut ips = [
            ("192.0.2.1", "vEthernet (Default Switch)"),
            ("192.0.2.2", "vEthernet (External-Direct)"),
            ("192.0.2.3", "vEthernet (Hosted)"),
            ("198.51.100.1", "singbox_tun"),
            ("198.51.100.2", "Wi-Fi"),
            ("203.0.113.1", "Ethernet"),
            ("203.0.113.2", "vEthernet (WSL)"),
            ("203.0.113.3", "DockerNAT"),
        ];
        ips.sort_by_key(|(_, name)| interface_rank(name));
        assert_eq!(ips.len(), 8, "ranking keeps all addresses selectable");
        assert_eq!(ips[0].1, "Wi-Fi");
        assert_eq!(ips[1].1, "Ethernet");
        assert_eq!(ips[2].1, "vEthernet (External-Direct)");
        assert_eq!(ips[3].1, "singbox_tun");
        assert!(
            interface_rank("vEthernet (External-Direct)")
                < interface_rank("vEthernet (Default Switch)")
        );
        for tunnel in ["tun0", "TAP-Windows Adapter", "WireGuard", "Tailscale"] {
            assert!(
                interface_rank("Ethernet") < interface_rank(tunnel),
                "{tunnel}"
            );
        }
    }

    #[test]
    fn console_warnings_explain_loopback_missing_bind_and_nonstandard_port() {
        assert_eq!(
            console_warnings("0.0.0.0", 53, Some("192.168.1.10")),
            [None, None]
        );
        assert_eq!(
            console_warnings("192.168.1.10", 53, Some("192.168.1.10")),
            [None, None]
        );
        assert!(console_warnings("127.0.0.1", 53, None)[0].is_some());
        assert!(console_warnings("192.168.1.99", 53, None)[0].is_some());
        assert!(console_warnings("0.0.0.0", 5353, Some("192.168.1.10"))[1].is_some());
        assert!(
            console_warnings("127.0.0.1", 5353, None)
                .iter()
                .all(Option::is_some)
        );
    }

    #[test]
    fn frameless_resize_hits_edges_and_corners_without_capturing_content() {
        use egui::viewport::ResizeDirection;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(720.0, 620.0));
        for (point, direction) in [
            (egui::pos2(1.0, 1.0), ResizeDirection::NorthWest),
            (egui::pos2(719.0, 1.0), ResizeDirection::NorthEast),
            (egui::pos2(1.0, 619.0), ResizeDirection::SouthWest),
            (egui::pos2(719.0, 619.0), ResizeDirection::SouthEast),
            (egui::pos2(705.0, 605.0), ResizeDirection::SouthEast),
            (egui::pos2(1.0, 300.0), ResizeDirection::West),
            (egui::pos2(719.0, 300.0), ResizeDirection::East),
            (egui::pos2(360.0, 1.0), ResizeDirection::North),
            (egui::pos2(360.0, 619.0), ResizeDirection::South),
        ] {
            assert_eq!(resize_direction(rect, point), Some(direction));
        }
        assert_eq!(resize_direction(rect, egui::pos2(360.0, 310.0)), None);
        assert_eq!(resize_direction(rect, egui::pos2(-1.0, 310.0)), None);
    }
}
