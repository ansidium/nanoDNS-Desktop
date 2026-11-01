//! Display-only catalogs. Protocol data, settings, and captured user values stay unchanged.
use std::{borrow::Cow, collections::HashMap, sync::OnceLock};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    En,
    Ru,
    De,
    Fr,
    Es,
    It,
    PtBr,
    PtPt,
    Nl,
    Pl,
    Uk,
    Cs,
    Sk,
    Tr,
    El,
    Hu,
    Ro,
    Bg,
    Hr,
    Da,
    Sv,
    Nb,
    Fi,
    Id,
    Vi,
    ZhHans,
    ZhHant,
    Ja,
    Ko,
    Ms,
}

impl Language {
    pub const ALL: [Self; 30] = [
        Self::En,
        Self::Ru,
        Self::De,
        Self::Fr,
        Self::Es,
        Self::It,
        Self::PtBr,
        Self::PtPt,
        Self::Nl,
        Self::Pl,
        Self::Uk,
        Self::Cs,
        Self::Sk,
        Self::Tr,
        Self::El,
        Self::Hu,
        Self::Ro,
        Self::Bg,
        Self::Hr,
        Self::Da,
        Self::Sv,
        Self::Nb,
        Self::Fi,
        Self::Id,
        Self::Vi,
        Self::ZhHans,
        Self::ZhHant,
        Self::Ja,
        Self::Ko,
        Self::Ms,
    ];

    pub const fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ru => "ru",
            Self::De => "de",
            Self::Fr => "fr",
            Self::Es => "es",
            Self::It => "it",
            Self::PtBr => "pt-BR",
            Self::PtPt => "pt-PT",
            Self::Nl => "nl",
            Self::Pl => "pl",
            Self::Uk => "uk",
            Self::Cs => "cs",
            Self::Sk => "sk",
            Self::Tr => "tr",
            Self::El => "el",
            Self::Hu => "hu",
            Self::Ro => "ro",
            Self::Bg => "bg",
            Self::Hr => "hr",
            Self::Da => "da",
            Self::Sv => "sv",
            Self::Nb => "nb",
            Self::Fi => "fi",
            Self::Id => "id",
            Self::Vi => "vi",
            Self::ZhHans => "zh-Hans",
            Self::ZhHant => "zh-Hant",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Ms => "ms",
        }
    }

    /// Accept standard language tags, Windows-style underscores, and POSIX suffixes.
    pub fn from_tag(value: &str) -> Option<Self> {
        let tag = value.trim().split(['.', '@']).next()?.replace('_', "-");
        let mut parts = tag.split('-');
        let primary = parts.next()?;
        if primary.eq_ignore_ascii_case("zh") {
            // A script subtag takes precedence over a regional fallback.
            let traditional = parts.clone().any(|part| part.eq_ignore_ascii_case("Hant"));
            let simplified = parts.clone().any(|part| part.eq_ignore_ascii_case("Hans"));
            let traditional_region = parts.any(|part| {
                ["TW", "HK", "MO"]
                    .iter()
                    .any(|region| part.eq_ignore_ascii_case(region))
            });
            return Some(if traditional || (!simplified && traditional_region) {
                Self::ZhHant
            } else {
                Self::ZhHans
            });
        }
        if primary.eq_ignore_ascii_case("pt") {
            return Some(
                if parts
                    .next()
                    .is_some_and(|part| part.eq_ignore_ascii_case("BR"))
                {
                    Self::PtBr
                } else {
                    Self::PtPt
                },
            );
        }
        if primary.eq_ignore_ascii_case("no") {
            return Some(Self::Nb);
        }
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.tag().eq_ignore_ascii_case(primary))
    }

    fn json(self) -> &'static str {
        match self {
            Self::En => include_str!("../locales/en.json"),
            Self::Ru => include_str!("../locales/ru.json"),
            Self::De => include_str!("../locales/de.json"),
            Self::Fr => include_str!("../locales/fr.json"),
            Self::Es => include_str!("../locales/es.json"),
            Self::It => include_str!("../locales/it.json"),
            Self::PtBr => include_str!("../locales/pt-BR.json"),
            Self::PtPt => include_str!("../locales/pt-PT.json"),
            Self::Nl => include_str!("../locales/nl.json"),
            Self::Pl => include_str!("../locales/pl.json"),
            Self::Uk => include_str!("../locales/uk.json"),
            Self::Cs => include_str!("../locales/cs.json"),
            Self::Sk => include_str!("../locales/sk.json"),
            Self::Tr => include_str!("../locales/tr.json"),
            Self::El => include_str!("../locales/el.json"),
            Self::Hu => include_str!("../locales/hu.json"),
            Self::Ro => include_str!("../locales/ro.json"),
            Self::Bg => include_str!("../locales/bg.json"),
            Self::Hr => include_str!("../locales/hr.json"),
            Self::Da => include_str!("../locales/da.json"),
            Self::Sv => include_str!("../locales/sv.json"),
            Self::Nb => include_str!("../locales/nb.json"),
            Self::Fi => include_str!("../locales/fi.json"),
            Self::Id => include_str!("../locales/id.json"),
            Self::Vi => include_str!("../locales/vi.json"),
            Self::ZhHans => include_str!("../locales/zh-Hans.json"),
            Self::ZhHant => include_str!("../locales/zh-Hant.json"),
            Self::Ja => include_str!("../locales/ja.json"),
            Self::Ko => include_str!("../locales/ko.json"),
            Self::Ms => include_str!("../locales/ms.json"),
        }
    }
    pub const fn is_cjk(self) -> bool {
        matches!(self, Self::ZhHans | Self::ZhHant | Self::Ja | Self::Ko)
    }
}

static LANGUAGE: OnceLock<Language> = OnceLock::new();
type Catalog = HashMap<String, String>;
// Only the selected language is parsed by normal tr/message calls. Explicit
// translate calls can select another language without leaking owned strings.
static CATALOGS: [OnceLock<Option<Catalog>>; Language::ALL.len()] =
    [const { OnceLock::new() }; Language::ALL.len()];

fn choose_language(windows_language: u16, override_value: Option<&str>) -> Language {
    if let Some(language) = override_value.and_then(Language::from_tag) {
        return language;
    }
    match windows_language & 0x03ff {
        0x0019 => Language::Ru,
        0x0007 => Language::De,
        0x000c => Language::Fr,
        0x000a => Language::Es,
        0x0010 => Language::It,
        0x0016 if windows_language >> 10 == 1 => Language::PtBr,
        0x0016 => Language::PtPt,
        0x0013 => Language::Nl,
        0x0015 => Language::Pl,
        0x0022 => Language::Uk,
        0x0005 => Language::Cs,
        0x001b => Language::Sk,
        0x001f => Language::Tr,
        0x0008 => Language::El,
        0x000e => Language::Hu,
        0x0018 => Language::Ro,
        0x0002 => Language::Bg,
        // Serbian and Bosnian share Croatian's primary ID. Only Croatian
        // sublanguages select this catalog; unsupported languages use English.
        0x001a if matches!(windows_language >> 10, 1 | 4) => Language::Hr,
        0x0006 => Language::Da,
        0x001d => Language::Sv,
        0x0014 => Language::Nb,
        0x000b => Language::Fi,
        0x0021 => Language::Id,
        0x002a => Language::Vi,
        0x0004 if matches!(windows_language, 0x0404 | 0x0c04 | 0x1404 | 0x7c04) => Language::ZhHant,
        0x0004 => Language::ZhHans,
        0x0011 => Language::Ja,
        0x0012 => Language::Ko,
        0x003e => Language::Ms,
        _ => Language::En,
    }
}

#[cfg(windows)]
fn windows_ui_language() -> u16 {
    // SAFETY: this read-only Windows API has no pointer arguments or prerequisites.
    unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() }
}

#[cfg(not(windows))]
fn windows_ui_language() -> u16 {
    0x0409
}

pub fn language() -> Language {
    *LANGUAGE.get_or_init(|| {
        // A process-local QA override lets screenshots exercise each language
        // without changing Windows settings or the persisted application profile.
        let override_value = std::env::var("NANODNS_LANGUAGE").ok();
        choose_language(windows_ui_language(), override_value.as_deref())
    })
}

fn lookup(text: &str) -> Option<&'static str> {
    CATALOG
        .iter()
        .find_map(|&(source, english)| (source == text || english == text).then_some(english))
}

fn catalog(language: Language) -> Option<&'static Catalog> {
    let slot = Language::ALL
        .iter()
        .position(|candidate| *candidate == language)?;
    CATALOGS
        .get(slot)?
        .get_or_init(|| {
            // Embedded assets are checked by unit tests and check-locales.py.
            // A broken catalog must keep the app usable through English fallback.
            serde_json::from_str(language.json()).ok()
        })
        .as_ref()
}

fn localized(language: Language, english: &'static str) -> &'static str {
    if language == Language::En {
        return english;
    }
    catalog(language)
        .and_then(|values| values.get(english))
        .filter(|value| !value.trim().is_empty())
        .map_or(english, String::as_str)
}

pub fn translate(language: Language, text: &str) -> &str {
    lookup(text).map_or(text, |english| localized(language, english))
}

/// Translate a fixed label without allocating; unknown values remain verbatim.
pub fn tr(text: &str) -> &str {
    translate(language(), text)
}

/// Translate a complete display message, preserving captured paths/IPs/domains.
pub fn message(text: &str) -> Cow<'_, str> {
    message_for(language(), text, 0)
}

fn message_for(language: Language, text: &str, depth: u8) -> Cow<'_, str> {
    if depth >= 4 {
        return Cow::Borrowed(text);
    }
    if let Some(english) = lookup(text) {
        return Cow::Borrowed(localized(language, english));
    }
    for &(source, english, translate_values) in TEMPLATES {
        let target = localized(language, english);
        for source in [source, english] {
            if let Some(output) =
                render_template(language, text, source, target, translate_values, depth)
            {
                return Cow::Owned(output);
            }
        }
    }
    Cow::Borrowed(text)
}

fn render_template(
    language: Language,
    text: &str,
    source: &str,
    target: &str,
    translate_values: bool,
    depth: u8,
) -> Option<String> {
    let mut source_parts = source.split("{}").peekable();
    let mut rest = text.strip_prefix(source_parts.next()?)?;
    let mut captures = [""; 8];
    let mut count = 0;
    while let Some(literal) = source_parts.next() {
        let capture = captures.get_mut(count)?;
        if source_parts.peek().is_none() {
            // A final literal belongs at the end of the complete message.
            // Paths, domains, and IP addresses may contain that literal too.
            *capture = rest.strip_suffix(literal)?;
            rest = "";
        } else {
            let end = rest.find(literal)?;
            *capture = rest.get(..end)?;
            rest = rest.get(end..)?.strip_prefix(literal)?;
        }
        count += 1;
    }
    if !rest.is_empty() {
        return None;
    }
    let mut target_parts = target.split("{}");
    let mut output = String::with_capacity(text.len());
    output.push_str(target_parts.next()?);
    for capture in captures.get(..count)? {
        if translate_values {
            output.push_str(&message_for(language, capture, depth.saturating_add(1)));
        } else {
            output.push_str(capture);
        }
        output.push_str(target_parts.next()?);
    }
    target_parts.next().is_none().then_some(output)
}

const CATALOG: &[(&str, &str)] = &[
    ("Свернуть окно", "Minimize window"),
    ("Развернуть окно", "Maximize window"),
    ("Восстановить размер окна", "Restore window size"),
    ("Закрыть окно", "Close window"),
    ("Открыть nanoDNS", "Open nanoDNS"),
    ("Запустить DNS", "Start DNS"),
    ("Остановить DNS", "Stop DNS"),
    ("Выйти", "Exit"),
    ("nanoDNS — DNS работает", "nanoDNS — DNS running"),
    ("nanoDNS — DNS остановлен", "nanoDNS — DNS stopped"),
    ("DNS работает", "DNS running"),
    ("DNS работает локально", "DNS running locally"),
    ("DNS выключен", "DNS stopped"),
    ("Подключение", "Connection"),
    ("Правила", "Rules"),
    ("Настройки", "Settings"),
    ("Журнал", "Log"),
    (
        "Компьютер и PlayStation должны быть в одной сети.",
        "The computer and PlayStation must be on the same network.",
    ),
    ("Адрес DNS для консоли", "DNS address for the console"),
    ("Копировать", "Copy"),
    ("Адрес скопирован", "Address copied"),
    (
        "Нет подключения к локальной сети",
        "No local network connection",
    ),
    (
        "Подключите компьютер к Wi-Fi или Ethernet.",
        "Connect the computer to Wi-Fi or Ethernet.",
    ),
    (
        "Нет подключения к локальной сети. Подключите компьютер к Wi-Fi или Ethernet.",
        "No local network connection. Connect the computer to Wi-Fi or Ethernet.",
    ),
    (
        "Выберите подключение к сети консоли, а не внутренний коммутатор виртуальных машин.",
        "Select the console's network connection, rather than an internal virtual machine switch.",
    ),
    (
        "Подключение, на котором работал DNS, недоступно. Выберите сеть консоли и запустите DNS снова.",
        "The connection used by DNS is unavailable. Select the console's network and start DNS again.",
    ),
    (
        "Выберите подключение к той же сети, что и консоль.",
        "Select a connection on the same network as the console.",
    ),
    (
        "Для смены подключения остановите DNS.",
        "Stop DNS before changing the connection.",
    ),
    ("На PlayStation", "On PlayStation"),
    (
        "В настройках сети выберите ручной DNS и введите адрес компьютера в оба поля.",
        "In network settings, choose manual DNS and enter the computer's address in both fields.",
    ),
    ("Подробная настройка", "Detailed setup"),
    (
        "В настройках сети PS4 / PS5 выберите своё подключение и ручной DNS.",
        "In PS4 / PS5 network settings, select your connection and manual DNS.",
    ),
    (
        "Укажите адрес компьютера в основном и дополнительном DNS. IP / DHCP и MTU оставьте автоматически, прокси — не использовать.",
        "Enter the computer's address for primary and secondary DNS. Leave IP / DHCP and MTU automatic, and do not use a proxy.",
    ),
    (
        "Компьютер должен оставаться включённым. При смене его IP обновите оба поля DNS на консоли.",
        "Keep the computer switched on. If its IP changes, update both DNS fields on the console.",
    ),
    ("Настройки сети Windows", "Windows network settings"),
    ("Запускать с Windows", "Start with Windows"),
    (
        "Приложение запускается в трее. Этот переключатель применяется сразу.",
        "The app starts in the system tray. This switch takes effect immediately.",
    ),
    (
        "Локальный режим · консоль недоступна",
        "Local mode · console unavailable",
    ),
    (
        "Проверяем доступ из локальной сети…",
        "Checking local network access…",
    ),
    ("Проверяется при запуске DNS", "Checked when DNS starts"),
    (
        "Доступ из локальной сети разрешён",
        "Local network access allowed",
    ),
    (
        "Доступ разрешён · UDP · локальная сеть",
        "Access allowed · UDP · local network",
    ),
    ("Доступ не проверен", "Access not checked"),
    ("Доступ не настроен", "Access not configured"),
    ("Повторить настройку", "Retry setup"),
    (
        "Подтвердите запрос администратора Windows",
        "Approve the Windows administrator prompt",
    ),
    (
        "Дождитесь завершения настройки брандмауэра.",
        "Wait for firewall setup to finish.",
    ),
    (
        "Брандмауэр настроен автоматически",
        "Firewall configured automatically",
    ),
    (
        "Windows не подтвердила настройку порта. Нажмите «Повторить настройку».",
        "Windows did not confirm port setup. Select \"Retry setup\".",
    ),
    (
        "Нужна настройка для PlayStation",
        "PlayStation setup required",
    ),
    ("Открыть настройки DNS", "Open DNS settings"),
    (
        "В «Настройках» укажите IPv4 0.0.0.0 и порт UDP 53.",
        "In Settings, set IPv4 to 0.0.0.0 and the UDP port to 53.",
    ),
    (
        "В «Настройках» укажите адрес компьютера или 0.0.0.0 и порт UDP 53.",
        "In Settings, enter the computer's address or 0.0.0.0 and UDP port 53.",
    ),
    (
        "Консоль не может подключиться: DNS слушает только этот компьютер. В «Настройках» укажите IPv4 0.0.0.0.",
        "The console cannot connect: DNS is listening only on this computer. In Settings, set IPv4 to 0.0.0.0.",
    ),
    (
        "Консоль не может подключиться: адрес прослушивания отсутствует в локальной сети. Проверьте IPv4 в «Настройках».",
        "The console cannot connect: the listening address is not on the local network. Check IPv4 in Settings.",
    ),
    (
        "Для подключения PlayStation нужен порт UDP 53. Измените порт в «Настройках».",
        "PlayStation requires UDP port 53. Change the port in Settings.",
    ),
    ("Правила DNS", "DNS rules"),
    ("Домены и IPv4", "Domains and IPv4"),
    ("Исключения", "Exceptions"),
    ("Синтаксис правил", "Rule syntax"),
    (
        "домен=IPv4 · * — любые символы, ? — один. Правила проверяются сверху вниз.",
        "domain=IPv4 · * matches any characters; ? matches one. Rules are checked from top to bottom.",
    ),
    (
        "Исключения — по одному домену на строку; они имеют приоритет и не подменяют IP.",
        "Enter one exception domain per line. Exceptions take priority and do not override the IP.",
    ),
    ("Сохранить", "Save"),
    ("Отменить", "Cancel"),
    ("Настройки сохранены", "Settings saved"),
    ("Дождитесь настройки брандмауэра", "Wait for firewall setup"),
    (
        "DNS перезапустится после сохранения",
        "DNS will restart after saving",
    ),
    ("Изменения не сохранены", "Unsaved changes"),
    ("Настройки DNS", "DNS settings"),
    ("IPv4-адрес прослушивания", "Listening IPv4 address"),
    (
        "0.0.0.0 — подключение, выбранное на вкладке «Подключение». Внутренняя сеть виртуальных машин не используется.",
        "0.0.0.0 uses the connection selected on the Connection tab. The internal virtual machine network is not used.",
    ),
    ("Внешние DNS-серверы", "Upstream DNS servers"),
    (
        "По одному IP на строку. Серверы опрашиваются сверху вниз.",
        "One IP per line. Servers are tried from top to bottom.",
    ),
    ("Дополнительные параметры", "Advanced settings"),
    ("Порт UDP", "UDP port"),
    (
        "Для подключения PlayStation используйте порт 53.",
        "Use port 53 to connect PlayStation.",
    ),
    ("IPv6-адрес", "IPv6 address"),
    (
        "off — не запускать IPv6-слушатель.",
        "off disables the IPv6 listener.",
    ),
    ("Тайм-аут, мс", "Timeout, ms"),
    (
        "Общий тайм-аут одного DNS-запроса.",
        "Total timeout for one DNS query.",
    ),
    ("Приложение", "Application"),
    ("Брандмауэр Windows:", "Windows Firewall:"),
    ("Клиент:", "Client:"),
    ("Включать DNS при запуске", "Start DNS when the app opens"),
    (
        "Сворачивать окно в трей при закрытии",
        "Close to system tray",
    ),
    ("Файлы и версия", "Files and version"),
    (
        "Открыть папку настроек и журнала",
        "Open settings and log folder",
    ),
    ("Исходный nanoDNS", "Original nanoDNS"),
    ("Журнал DNS", "DNS log"),
    ("Поиск", "Search"),
    ("Домен или IP клиента", "Domain or client IP"),
    (
        "Фильтр журнала по домену или IP клиента",
        "Filter the log by domain or client IP",
    ),
    ("Очистить журнал", "Clear log"),
    (
        "Очистить список в этом окне. Файл журнала сохраняется.",
        "Clear the list in this window. The log file is kept.",
    ),
    ("Запросов пока нет.", "No queries yet."),
    (
        "DNS работает. Укажите адрес компьютера в настройках DNS консоли.",
        "DNS is running. Enter the computer's address in the console's DNS settings.",
    ),
    (
        "Запустите DNS кнопкой вверху окна.",
        "Start DNS using the button at the top of the window.",
    ),
    ("Ничего не найдено.", "No matches found."),
    ("Сбросить поиск", "Clear search"),
    ("Время UTC", "Time UTC"),
    ("Домен", "Domain"),
    ("Тип", "Type"),
    ("Результат", "Result"),
    ("Нет записи", "No record"),
    ("Ошибка DNS", "DNS error"),
    (
        "NODATA — для этого типа запроса запись не возвращается. Это ожидаемый ответ правила.",
        "NODATA means no record is returned for this query type. This is the rule's expected response.",
    ),
    (
        "SERVFAIL — DNS не смог обработать запрос.",
        "SERVFAIL means DNS could not process the query.",
    ),
    (
        "DNS остановился из-за ошибки сети. Проверьте подключение и запустите его снова.",
        "DNS stopped because of a network error. Check the connection and start it again.",
    ),
    (
        "Порт DNS занят другой программой",
        "Another program is using the DNS port",
    ),
    (
        "Остановите другой DNS-сервис и нажмите «Запустить DNS» снова.",
        "Stop the other DNS service, then select \"Start DNS\" again.",
    ),
    (
        "Адрес прослушивания недоступен",
        "Listening address unavailable",
    ),
    (
        "Проверьте IPv4 в «Настройках»: укажите адрес компьютера или 0.0.0.0. Затем запустите DNS снова.",
        "Check IPv4 in Settings: enter the computer's address or 0.0.0.0, then start DNS again.",
    ),
    ("Технические подробности", "Technical details"),
    (
        "Файл настроек слишком большой.",
        "The settings file is too large.",
    ),
    (
        "Укажите правильный IPv4-адрес прослушивания.",
        "Enter a valid listening IPv4 address.",
    ),
    (
        "Укажите IPv6-адрес или off.",
        "Enter an IPv6 address or off.",
    ),
    (
        "Порт должен быть от 1 до 65535.",
        "The port must be between 1 and 65535.",
    ),
    (
        "Тайм-аут должен быть от 100 до 10000 мс.",
        "The timeout must be between 100 and 10000 ms.",
    ),
    (
        "Укажите от 1 до 8 DNS-серверов.",
        "Enter between 1 and 8 DNS servers.",
    ),
    (
        "DNS-сервер должен иметь адрес устройства, а не 0.0.0.0 или multicast.",
        "The DNS server must have a device address, rather than 0.0.0.0 or a multicast address.",
    ),
    (
        "DNS-сервер указывает на этот компьютер и создаст цикл. Укажите внешний DNS.",
        "The DNS server points to this computer and would create a loop. Enter an upstream DNS server.",
    ),
    (
        "Максимум 128 правил и 128 исключений.",
        "A maximum of 128 rules and 128 exceptions is allowed.",
    ),
    (
        "Порт DNS-сервера должен быть от 1 до 65535.",
        "The DNS server port must be between 1 and 65535.",
    ),
    ("Нет папки настроек", "The settings folder is missing"),
    (
        "Не указана папка настроек.",
        "The settings folder was not specified.",
    ),
    (
        "Не указана папка снимков.",
        "The screenshot folder was not specified.",
    ),
    (
        "Не указано время запуска",
        "The run duration was not specified",
    ),
    ("Неверное время запуска", "Invalid run duration"),
    ("Неизвестный параметр запуска.", "Unknown launch argument."),
    (
        "Не удалось определить IPv4-адрес DNS.",
        "Could not determine the DNS IPv4 address.",
    ),
    (
        "nanoDNS уже запущен для этой папки настроек.",
        "nanoDNS is already running with this settings folder.",
    ),
    ("nanoDNS завершает работу.", "nanoDNS is shutting down."),
    (
        "Обработчик окна nanoDNS ещё не готов.",
        "The nanoDNS window handler is not ready yet.",
    ),
    (
        "Окно nanoDNS недоступно для повторного запуска.",
        "The nanoDNS window is unavailable for reopening.",
    ),
    (
        "Не удалось подключить окно nanoDNS.",
        "Could not attach the nanoDNS window.",
    ),
    (
        "Не удалось подключить обработчик окна.",
        "Could not attach the window handler.",
    ),
    (
        "Обработчик окна уже подключён.",
        "The window handler is already attached.",
    ),
    (
        "Не удалось отключить обработчик окна.",
        "Could not detach the window handler.",
    ),
    (
        "Окно nanoDNS больше недоступно.",
        "The nanoDNS window is no longer available.",
    ),
    (
        "Не удалось прочитать состояние окна nanoDNS.",
        "Could not read the nanoDNS window state.",
    ),
    (
        "Windows не смогла восстановить окно nanoDNS.",
        "Windows could not restore the nanoDNS window.",
    ),
    (
        "Windows не смогла обновить окно nanoDNS.",
        "Windows could not refresh the nanoDNS window.",
    ),
    (
        "Windows не предоставила дескриптор окна. Сворачивание в трей недоступно.",
        "Windows did not provide a window handle. Minimizing to the system tray is unavailable.",
    ),
    (
        "Некорректный атрибут оформления окна.",
        "Invalid window appearance attribute.",
    ),
    (
        "Некорректный размер атрибута оформления окна.",
        "Invalid window appearance attribute size.",
    ),
    (
        "Windows не предоставила папку LOCALAPPDATA.",
        "Windows did not provide the LOCALAPPDATA folder.",
    ),
    (
        "Путь приложения содержит некорректный Unicode.",
        "The application path contains invalid Unicode.",
    ),
    ("Некорректный путь приложения.", "Invalid application path."),
    ("Некорректный путь журнала.", "Invalid log path."),
    ("Не удалось открыть браузер.", "Could not open the browser."),
    (
        "Доступ не изменён: запрос Windows отменён или не удалось открыть настройки.",
        "Access was not changed: the Windows prompt was canceled or settings could not be opened.",
    ),
    (
        "Не удалось проверить брандмауэр Windows.",
        "Could not check Windows Firewall.",
    ),
    ("Не удалось запустить nanoDNS.", "Could not start nanoDNS."),
    (
        "Работающий экземпляр не подтвердил открытие окна. Если это предыдущая версия, выберите «Выйти» в её трее и запустите новую версию.",
        "The running instance did not confirm that its window opened. If it is an older version, select \"Exit\" in its system tray menu and start the new version.",
    ),
];

// Whole-message templates, never free substring replacements. The boolean is
// true only for nested application errors/status; data values stay verbatim.
const TEMPLATES: &[(&str, &str, bool)] = &[
    (
        "Всего {} · Подмена {} · Внешний DNS {} · Ошибки {} · {} мин работы",
        "Total {} · Overrides {} · Upstream DNS {} · Errors {} · {} min running",
        false,
    ),
    ("{}\nКлиент: {}", "{}\nClient: {}", false),
    (
        "{}\nВыберите подключение к той же сети, что и консоль.",
        "{}\nSelect a connection on the same network as the console.",
        false,
    ),
    ("Брандмауэр Windows: {}", "Windows Firewall: {}", true),
    (
        "Не удалось запустить nanoDNS.\n\n{}",
        "Could not start nanoDNS.\n\n{}",
        true,
    ),
    (
        "nanoDNS уже запущен для этой папки настроек.\n{}",
        "nanoDNS is already running with this settings folder.\n{}",
        true,
    ),
    (
        "Не удалось запустить nanoDNS: {}",
        "Could not start nanoDNS: {}",
        true,
    ),
    (
        "Не удалось прочитать настройки: {}. Исходный файл сохранён.",
        "Could not read settings: {}. The original file was preserved.",
        false,
    ),
    (
        "Не удалось сохранить настройки: {}",
        "Could not save settings: {}",
        false,
    ),
    (
        "Неверный DNS-сервер: {}. Укажите IP или IP:порт.",
        "Invalid DNS server: {}. Enter an IP address or IP:port.",
        false,
    ),
    (
        "Неверная маска домена: {}. Для IDN используйте punycode.",
        "Invalid domain pattern: {}. Use punycode for IDNs.",
        false,
    ),
    ("Неверная маска {}: {}", "Invalid pattern {}: {}", false),
    (
        "Правило {}: используйте домен=IP.",
        "Rule {}: use domain=IP.",
        false,
    ),
    (
        "Правило {}: неверный IPv4-адрес.",
        "Rule {}: invalid IPv4 address.",
        false,
    ),
    (
        "Не удалось открыть {}. Порт может быть занят другой программой или адрес недоступен.\n{}",
        "Could not open {}. Another program may be using the port, or the address may be unavailable.\n{}",
        false,
    ),
    (
        "Не удалось определить адрес DNS: {}",
        "Could not determine the DNS address: {}",
        false,
    ),
    (
        "Не удалось открыть журнал: {}",
        "Could not open the log: {}",
        false,
    ),
    (
        "Не удалось перейти в конец журнала: {}",
        "Could not seek to the end of the log: {}",
        false,
    ),
    (
        "Не удалось запустить DNS: {}",
        "Could not start DNS: {}",
        false,
    ),
    (
        "Не удалось обновить данные запуска: {}",
        "Could not update launch data: {}",
        false,
    ),
    (
        "Не удалось зарегистрировать открытие окна: {}",
        "Could not register window activation: {}",
        false,
    ),
    (
        "Не удалось удалить данные запуска: {}",
        "Could not remove launch data: {}",
        false,
    ),
    (
        "Не удалось открыть блокировку настроек: {}",
        "Could not open the settings lock: {}",
        false,
    ),
    (
        "Не удалось заблокировать настройки: {}",
        "Could not lock settings: {}",
        false,
    ),
    (
        "Не удалось открыть окно nanoDNS: {}",
        "Could not open the nanoDNS window: {}",
        false,
    ),
    (
        "Не удалось получить окно nanoDNS. Сворачивание в трей недоступно: {}",
        "Could not obtain the nanoDNS window. Minimizing to the system tray is unavailable: {}",
        false,
    ),
    (
        "DWM не применил скругление окна: HRESULT {}.",
        "DWM did not apply rounded corners: HRESULT {}.",
        false,
    ),
    (
        "Не удалось проверить скругление окна: HRESULT {}.",
        "Could not check rounded corners: HRESULT {}.",
        false,
    ),
    (
        "Нет доступа к автозапуску: {}",
        "Could not access Windows startup settings: {}",
        false,
    ),
    (
        "Трей недоступен: {}",
        "The system tray is unavailable: {}",
        false,
    ),
];

#[cfg(test)]
mod tests {
    use super::{
        CATALOG, Catalog, Language, TEMPLATES, choose_language, localized, message_for, translate,
    };
    use std::{borrow::Cow, collections::HashSet};

    #[test]
    fn os_ui_language_and_qa_override_are_deterministic() {
        for (windows, language) in [
            (0x0409, Language::En),
            (0x0419, Language::Ru),
            (0x0819, Language::Ru),
            (0x0407, Language::De),
            (0x0c0c, Language::Fr),
            (0x080a, Language::Es),
            (0x0410, Language::It),
            (0x0416, Language::PtBr),
            (0x0816, Language::PtPt),
            (0x0413, Language::Nl),
            (0x0415, Language::Pl),
            (0x0422, Language::Uk),
            (0x0405, Language::Cs),
            (0x041b, Language::Sk),
            (0x041f, Language::Tr),
            (0x0408, Language::El),
            (0x040e, Language::Hu),
            (0x0418, Language::Ro),
            (0x0402, Language::Bg),
            (0x041a, Language::Hr),
            (0x101a, Language::Hr),
            (0x0406, Language::Da),
            (0x041d, Language::Sv),
            (0x0414, Language::Nb),
            (0x0814, Language::Nb),
            (0x040b, Language::Fi),
            (0x0421, Language::Id),
            (0x042a, Language::Vi),
            (0x0804, Language::ZhHans),
            (0x1004, Language::ZhHans),
            (0x0404, Language::ZhHant),
            (0x0c04, Language::ZhHant),
            (0x1404, Language::ZhHant),
            (0x7c04, Language::ZhHant),
            (0x0411, Language::Ja),
            (0x0412, Language::Ko),
            (0x043e, Language::Ms),
            (0x083e, Language::Ms),
        ] {
            assert_eq!(
                choose_language(windows, None),
                language,
                "Windows language {windows:04x}"
            );
        }
        for unsupported in [0, 0x0401, 0x040d, 0x081a, 0x141a, 0x044c, 0x043a] {
            assert_eq!(choose_language(unsupported, None), Language::En);
        }
        assert_eq!(choose_language(0x0419, Some("EN")), Language::En);
        assert_eq!(choose_language(0x0409, Some(" ru ")), Language::Ru);
        assert_eq!(choose_language(0x0419, Some("invalid")), Language::Ru);
        assert_eq!(choose_language(0x0419, Some("ar")), Language::Ru);
        assert_eq!(choose_language(0x0409, Some("PT_br")), Language::PtBr);
        assert_eq!(choose_language(0x0409, Some("pt-PT")), Language::PtPt);
        assert_eq!(choose_language(0x0409, Some("pt")), Language::PtPt);
        assert_eq!(choose_language(0x0409, Some("pt-AO")), Language::PtPt);
        assert_eq!(choose_language(0x0409, Some("fr_CA.UTF-8")), Language::Fr);
        assert_eq!(choose_language(0x0409, Some("no-NO")), Language::Nb);
        assert_eq!(choose_language(0x0409, Some("vi_VN")), Language::Vi);
        for (tag, expected) in [
            ("zh", Language::ZhHans),
            ("zh_CN", Language::ZhHans),
            ("zh-sg", Language::ZhHans),
            ("ZH_hans", Language::ZhHans),
            ("zh-Hans-TW", Language::ZhHans),
            ("zh_TW.UTF-8", Language::ZhHant),
            ("zh-HK", Language::ZhHant),
            ("zh-MO", Language::ZhHant),
            ("zh-Hant-CN", Language::ZhHant),
            ("ja_JP", Language::Ja),
            ("ko-KR", Language::Ko),
            ("ms", Language::Ms),
            ("ms-MY", Language::Ms),
            ("MS_bn", Language::Ms),
            (" ms_MY.UTF-8 ", Language::Ms),
        ] {
            assert_eq!(choose_language(0x0409, Some(tag)), expected, "tag: {tag}");
        }
        for language in Language::ALL {
            assert_eq!(Language::from_tag(language.tag()), Some(language));
            assert_eq!(
                language.is_cjk(),
                matches!(language.tag(), "zh-Hans" | "zh-Hant" | "ja" | "ko")
            );
        }
    }

    #[test]
    fn complete_labels_use_computer_and_unknown_values_stay_verbatim() {
        assert_eq!(translate(Language::Ru, "Подключение"), "Подключение");
        assert_eq!(translate(Language::En, "Подключение"), "Connection");
        assert_eq!(
            translate(
                Language::En,
                "Компьютер и PlayStation должны быть в одной сети."
            ),
            "The computer and PlayStation must be on the same network."
        );
        for value in [
            "Wi-Fi Дом",
            "192.168.31.78",
            "*.playstation.net",
            r"C:\Настройки\nanoDNS.exe",
            // NFD in a user value must not be normalized while catalogs use NFC.
            "Wi-Fi cafe\u{0301} / Δίκτυο / Đường dẫn",
            "[fe80::1%12]:53",
        ] {
            for language in Language::ALL {
                let translated = translate(language, value);
                assert_eq!(translated, value);
                assert!(
                    std::ptr::eq(translated, value),
                    "unknown values must stay borrowed"
                );
                assert!(
                    matches!(message_for(language, value, 0), Cow::Borrowed(text) if text == value)
                );
            }
        }
    }

    #[test]
    fn dynamic_messages_preserve_values_and_translate_nested_errors() {
        let message =
            "Не удалось запустить nanoDNS.\n\nnanoDNS уже запущен для этой папки настроек.";
        assert_eq!(
            message_for(Language::En, message, 0),
            "Could not start nanoDNS.\n\nnanoDNS is already running with this settings folder."
        );
        assert_eq!(
            message_for(
                Language::En,
                "nanoDNS уже запущен для этой папки настроек.\nРаботающий экземпляр не подтвердил открытие окна. Если это предыдущая версия, выберите «Выйти» в её трее и запустите новую версию.",
                0
            ),
            "nanoDNS is already running with this settings folder.\nThe running instance did not confirm that its window opened. If it is an older version, select \"Exit\" in its system tray menu and start the new version."
        );
        assert_eq!(
            message_for(Language::En, "Правило 7: неверный IPv4-адрес.", 0),
            "Rule 7: invalid IPv4 address."
        );
        assert_eq!(
            message_for(
                Language::En,
                "Всего 12 · Подмена 3 · Внешний DNS 8 · Ошибки 1 · 2 мин работы",
                0
            ),
            "Total 12 · Overrides 3 · Upstream DNS 8 · Errors 1 · 2 min running"
        );
        assert_eq!(
            message_for(
                Language::En,
                "Wi-Fi Дом\nВыберите подключение к той же сети, что и консоль.",
                0
            ),
            "Wi-Fi Дом\nSelect a connection on the same network as the console."
        );
        assert_eq!(
            message_for(
                Language::En,
                r"Не удалось сохранить настройки: C:\Настройки\settings.json",
                0
            ),
            r"Could not save settings: C:\Настройки\settings.json"
        );
    }

    #[test]
    fn catalog_keys_are_unique_and_templates_keep_placeholder_counts() {
        let mut keys = std::collections::HashSet::new();
        for &(source, english) in CATALOG {
            assert!(keys.insert(source), "duplicate catalog key: {source}");
            assert!(
                !english
                    .chars()
                    .any(|character| ('\u{0400}'..='\u{04ff}').contains(&character))
            );
            assert!(!english.to_lowercase().contains("laptop"));
        }
        for &(source, english, _) in TEMPLATES {
            assert_eq!(source.matches("{}").count(), english.matches("{}").count());
            assert!((1..=8).contains(&source.matches("{}").count()));
        }
    }

    #[test]
    fn every_embedded_catalog_covers_exactly_the_complete_source_keyset()
    -> Result<(), serde_json::Error> {
        let expected: HashSet<_> = CATALOG
            .iter()
            .map(|&(_, english)| english)
            .chain(TEMPLATES.iter().map(|&(_, english, _)| english))
            .collect();
        assert_eq!(expected.len(), 184);
        assert_eq!(Language::ALL.len(), 30);
        let mut tags = HashSet::new();
        for language in Language::ALL {
            assert!(tags.insert(language.tag()), "duplicate language tag");
            let catalog: Catalog = serde_json::from_str(language.json())?;
            assert_eq!(catalog.len(), expected.len(), "{} coverage", language.tag());
            assert_eq!(
                catalog.keys().map(String::as_str).collect::<HashSet<_>>(),
                expected,
                "{} source keys",
                language.tag()
            );
            for (key, value) in &catalog {
                assert!(
                    !value.trim().is_empty(),
                    "{} empty value: {key}",
                    language.tag()
                );
                assert_eq!(
                    key.matches("{}").count(),
                    value.matches("{}").count(),
                    "{} placeholders: {key}",
                    language.tag()
                );
                assert_eq!(
                    key.matches('\n').count(),
                    value.matches('\n').count(),
                    "{} line breaks: {key}",
                    language.tag()
                );
                if language == Language::En {
                    assert_eq!(key, value, "English keys must be identity translations");
                }
            }
        }
        Ok(())
    }

    #[test]
    fn every_language_translates_nested_errors_and_preserves_dynamic_data() {
        let data = "C:\\Настрои\u{0306}ки\\Δίκτυο\\đường dẫn.json / [fe80::1%4]:53 / *.example.net";
        for language in Language::ALL {
            assert_eq!(
                translate(language, "Подключение"),
                localized(language, "Connection")
            );
            assert_eq!(
                translate(language, "Connection"),
                localized(language, "Connection")
            );
            let input = format!("Не удалось сохранить настройки: {data}");
            let expected =
                localized(language, "Could not save settings: {}").replacen("{}", data, 1);
            assert_eq!(
                message_for(language, &input, 0),
                expected,
                "{} data",
                language.tag()
            );

            let input = "Не удалось запустить nanoDNS.\n\nНе удалось сохранить настройки: "
                .to_owned()
                + data;
            let inner = localized(language, "Could not save settings: {}").replacen("{}", data, 1);
            let expected =
                localized(language, "Could not start nanoDNS.\n\n{}").replacen("{}", &inner, 1);
            assert_eq!(
                message_for(language, &input, 0),
                expected,
                "{} nested error",
                language.tag()
            );

            let input = "Брандмауэр Windows: Доступ не настроен";
            let expected = localized(language, "Windows Firewall: {}").replacen(
                "{}",
                localized(language, "Access not configured"),
                1,
            );
            assert_eq!(
                message_for(language, input, 0),
                expected,
                "{} nested status",
                language.tag()
            );
        }
    }

    #[test]
    fn every_template_preserves_capture_order_in_every_language() {
        for language in Language::ALL {
            for &(source, english, _) in TEMPLATES {
                for source in [source, english] {
                    let mut input = source.to_owned();
                    let mut expected = localized(language, english).to_owned();
                    for index in 0..source.matches("{}").count() {
                        let value = format!("VALUE-{index}-Δίκτυο-Đường-192.0.2.1");
                        input = input.replacen("{}", &value, 1);
                        expected = expected.replacen("{}", &value, 1);
                    }
                    assert_eq!(
                        message_for(language, &input, 0),
                        expected,
                        "{} template: {source}",
                        language.tag()
                    );
                }
            }
        }
    }

    #[test]
    fn final_template_literal_inside_captured_data_stays_verbatim() {
        for language in Language::ALL {
            let value = "C:\\file.name\\192.0.2.1.json";
            let input = format!("DWM не применил скругление окна: HRESULT {value}.");
            let expected = localized(language, "DWM did not apply rounded corners: HRESULT {}.")
                .replacen("{}", value, 1);
            assert_eq!(message_for(language, &input, 0), expected);
            assert!(
                matches!(
                    message_for(language, &(input + " suffix"), 0),
                    Cow::Borrowed(_)
                ),
                "a template with a final literal must consume the whole message"
            );
        }
    }

    #[test]
    fn whole_message_matching_does_not_translate_partial_or_recursive_user_values() {
        for language in Language::ALL {
            for input in [
                "prefix Не удалось сохранить настройки: C:\\file.json",
                "unknown: Подключение / Connection",
                "not an application error / Не удалось сохранить настройки: value",
            ] {
                assert!(
                    matches!(message_for(language, input, 0), Cow::Borrowed(text) if text == input)
                );
            }
            let input = "Не удалось сохранить настройки: Подключение";
            let expected =
                localized(language, "Could not save settings: {}").replacen("{}", "Подключение", 1);
            assert_eq!(
                message_for(language, input, 0),
                expected,
                "data captures must not recurse"
            );
            assert_eq!(
                message_for(language, "Подключение", 4),
                "Подключение",
                "nested translation is bounded"
            );
        }
    }
}
