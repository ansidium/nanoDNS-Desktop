use glob::Pattern;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Rule {
    pub mask: String,
    pub address: Ipv4Addr,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub bind: String,
    pub bind6: String,
    pub port: u16,
    pub upstreams: Vec<String>,
    pub timeout_ms: u64,
    pub rules: Vec<Rule>,
    pub exceptions: Vec<String>,
    pub start_dns: bool,
    pub close_to_tray: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let masks = [
            "*.playstation.com",
            "*.playstation.com.*",
            "playstation.com",
            "*.playstation.net",
            "*.playstation.net.*",
            "*.psndl.net",
            "playstation.net",
            "psndl.net",
        ];
        Self {
            bind: "0.0.0.0".into(),
            bind6: "off".into(),
            port: 53,
            upstreams: vec!["1.1.1.1".into(), "8.8.8.8".into(), "77.88.8.8".into()],
            timeout_ms: 1500,
            rules: masks
                .into_iter()
                .map(|mask| Rule {
                    mask: mask.into(),
                    address: Ipv4Addr::UNSPECIFIED,
                })
                .collect(),
            exceptions: [
                "feature.api.playstation.com",
                "*.stun.playstation.net",
                "stun.*.playstation.net",
                "ena.net.playstation.net",
                "post.net.playstation.net",
                "gst.prod.dl.playstation.net",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            start_dns: true,
            close_to_tray: true,
        }
    }
}

pub fn parse_upstream(value: &str) -> Result<SocketAddr, String> {
    if let Ok(ip) = value.trim().parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, 53));
    }
    let address: SocketAddr = value
        .trim()
        .parse()
        .map_err(|_| format!("Неверный DNS-сервер: {value}. Укажите IP или IP:порт."))?;
    if address.port() == 0 {
        return Err("Порт DNS-сервера должен быть от 1 до 65535.".into());
    }
    Ok(address)
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        self.bind
            .parse::<Ipv4Addr>()
            .map_err(|_| "Укажите правильный IPv4-адрес прослушивания.".to_string())?;
        if self.bind6 != "off" {
            self.bind6
                .parse::<std::net::Ipv6Addr>()
                .map_err(|_| "Укажите IPv6-адрес или off.".to_string())?;
        }
        if self.port == 0 {
            return Err("Порт должен быть от 1 до 65535.".into());
        }
        if !(100..=10_000).contains(&self.timeout_ms) {
            return Err("Тайм-аут должен быть от 100 до 10000 мс.".into());
        }
        if self.upstreams.is_empty() || self.upstreams.len() > 8 {
            return Err("Укажите от 1 до 8 DNS-серверов.".into());
        }
        let local_ips: Vec<IpAddr> = if_addrs::get_if_addrs()
            .unwrap_or_default()
            .into_iter()
            .map(|a| a.ip())
            .collect();
        for upstream in &self.upstreams {
            let address = parse_upstream(upstream)?;
            // An IPv4-mapped IPv6 address reaches the same IPv4 listener. Normalize
            // before checking for wildcard addresses and forwarding loops.
            let upstream_ip = match address.ip() {
                IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or(IpAddr::V6(ip), IpAddr::V4),
                ip => ip,
            };
            if upstream_ip.is_unspecified() || upstream_ip.is_multicast() {
                return Err(
                    "DNS-сервер должен иметь адрес устройства, а не 0.0.0.0 или multicast.".into(),
                );
            }
            if address.port() == self.port
                && (upstream_ip.is_loopback() || local_ips.contains(&upstream_ip))
            {
                return Err(
                    "DNS-сервер указывает на этот компьютер и создаст цикл. Укажите внешний DNS."
                        .into(),
                );
            }
        }
        if self.rules.len() > 128 || self.exceptions.len() > 128 {
            return Err("Максимум 128 правил и 128 исключений.".into());
        }
        for mask in self
            .rules
            .iter()
            .map(|r| &r.mask)
            .chain(self.exceptions.iter())
        {
            if mask.is_empty()
                || mask.len() > 253
                || !mask.is_ascii()
                || mask.chars().any(char::is_whitespace)
            {
                return Err(format!(
                    "Неверная маска домена: {mask}. Для IDN используйте punycode."
                ));
            }
            Pattern::new(mask).map_err(|e| format!("Неверная маска {mask}: {e}"))?;
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
        if metadata.len() > 256 * 1024 {
            return Err("Файл настроек слишком большой.".into());
        }
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        let settings: Self = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Не удалось прочитать настройки: {e}. Исходный файл сохранён."))?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let parent = path.parent().ok_or("Нет папки настроек")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        temp.write_all(&bytes)
            .and_then(|()| temp.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        temp.persist(path)
            .map_err(|e| format!("Не удалось сохранить настройки: {e}"))?;
        Ok(())
    }

    pub fn rule_text(&self) -> String {
        self.rules
            .iter()
            .map(|r| format!("{}={}", r.mask, r.address))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn parse_rules(text: &str) -> Result<Vec<Rule>, String> {
    let mut result = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let (mask, ip) = line
            .split_once('=')
            .ok_or_else(|| format!("Правило {}: используйте домен=IP.", index + 1))?;
        result.push(Rule {
            mask: mask.trim().trim_end_matches('.').to_ascii_lowercase(),
            address: ip
                .trim()
                .parse()
                .map_err(|_| format!("Правило {}: неверный IPv4-адрес.", index + 1))?,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_settings_roundtrip_and_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        let mut settings = Settings::default();
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        settings.timeout_ms = 2200;
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
    }
    #[test]
    fn invalid_config_is_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        fs::write(&path, b"broken").unwrap();
        assert!(Settings::load(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken");
    }
    #[test]
    fn rejects_invalid_inputs_and_dns_loop() {
        assert!(parse_rules("wrong").is_err());
        assert!(parse_rules("x=300.1.1.1").is_err());
        let settings = Settings {
            upstreams: vec!["127.0.0.1".into()],
            ..Settings::default()
        };
        assert!(settings.validate().is_err());
        assert!(
            Settings {
                exceptions: vec!["[".into()],
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Settings {
                timeout_ms: 0,
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn ipv4_mapped_upstreams_cannot_bypass_address_or_loop_validation() {
        for upstream in [
            "[::ffff:127.0.0.1]:53",
            "[::ffff:0.0.0.0]:53",
            "[::ffff:224.0.0.1]:53",
        ] {
            let settings = Settings {
                upstreams: vec![upstream.into()],
                ..Settings::default()
            };
            assert!(settings.validate().is_err(), "{upstream} must be rejected");
        }
        let settings = Settings {
            upstreams: vec!["[::ffff:192.0.2.1]:53".into()],
            ..Settings::default()
        };
        assert!(
            settings.validate().is_ok(),
            "a remote mapped IPv4 address remains valid"
        );
    }
}
