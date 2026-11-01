use crate::config::{Settings, parse_upstream};
use glob::{MatchOptions, Pattern};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Seek, SeekFrom, Write},
    net::{IpAddr, SocketAddr, SocketAddrV4, UdpSocket},
    os::windows::io::AsRawSocket,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::Networking::WinSock::{
    FD_SET, SOCKET_ERROR, TIMEVAL, WSAGetLastError, select,
};

const MAX_PACKET: usize = 4096;

#[derive(Clone, Debug)]
pub struct QueryEvent {
    pub time: String,
    pub domain: String,
    pub kind: String,
    pub result: String,
    pub client: String,
    pub blocked: bool,
    pub failed: bool,
}

#[derive(Default)]
pub struct Stats {
    pub total: AtomicU64,
    pub blocked: AtomicU64,
    pub forwarded: AtomicU64,
    pub failed: AtomicU64,
}

#[derive(Debug, PartialEq)]
struct Question {
    id: u16,
    flags: u16,
    count: u16,
    name: String,
    qtype: u16,
    class: u16,
    canonical_question: Vec<u8>,
}

fn word(packet: &[u8], offset: usize) -> Result<u16, &'static str> {
    let bytes: [u8; 2] = packet
        .get(offset..offset + 2)
        .ok_or("short DNS packet")?
        .try_into()
        .map_err(|_| "short DNS word")?;
    Ok(u16::from_be_bytes(bytes))
}

fn name(packet: &[u8], start: usize) -> Result<(String, usize), &'static str> {
    let mut cursor = start;
    let mut end = None;
    let mut labels = Vec::new();
    let mut visited = Vec::new();
    let mut name_length = 0;
    for _ in 0..128 {
        if visited.contains(&cursor) {
            return Err("DNS compression cycle");
        }
        visited.push(cursor);
        let length = *packet.get(cursor).ok_or("short DNS name")?;
        if length & 0xc0 == 0xc0 {
            let pointer = usize::from(word(packet, cursor)? & 0x3fff);
            if pointer < 12 || pointer >= packet.len() {
                return Err("invalid DNS pointer");
            }
            end.get_or_insert(cursor + 2);
            cursor = pointer;
            continue;
        }
        if length & 0xc0 != 0 {
            return Err("invalid DNS label");
        }
        cursor += 1;
        if length == 0 {
            return Ok((labels.join("."), end.unwrap_or(cursor)));
        }
        let length = usize::from(length);
        let label = packet
            .get(cursor..cursor + length)
            .ok_or("truncated DNS label")?;
        if !label.is_ascii() || label.iter().any(|b| !(33..=126).contains(b) || *b == b'.') {
            return Err("invalid DNS label bytes");
        }
        name_length += length + usize::from(!labels.is_empty());
        if name_length > 253 {
            return Err("DNS name too long");
        }
        labels.push(
            std::str::from_utf8(label)
                .map_err(|_| "invalid DNS name")?
                .to_owned(),
        );
        cursor += length;
    }
    Err("too many DNS compression jumps")
}

fn question(packet: &[u8]) -> Result<Question, &'static str> {
    if packet.len() < 12 || packet.len() > MAX_PACKET {
        return Err("invalid DNS packet size");
    }
    let count = word(packet, 4)?;
    if count == 0 {
        return Err("no DNS question");
    }
    let (name, cursor) = name(packet, 12)?;
    let qtype = word(packet, cursor)?;
    let class = word(packet, cursor + 2)?;
    let mut canonical_question = Vec::new();
    if !name.is_empty() {
        for label in name.split('.') {
            canonical_question.push(u8::try_from(label.len()).map_err(|_| "DNS label too long")?);
            canonical_question.extend_from_slice(label.as_bytes());
        }
    }
    canonical_question.push(0);
    canonical_question.extend_from_slice(&qtype.to_be_bytes());
    canonical_question.extend_from_slice(&class.to_be_bytes());
    Ok(Question {
        id: word(packet, 0)?,
        flags: word(packet, 2)?,
        count,
        name: name.to_ascii_lowercase(),
        qtype,
        class,
        canonical_question,
    })
}

fn response(
    _packet: &[u8],
    q: &Question,
    rcode: u16,
    address: Option<std::net::Ipv4Addr>,
) -> Vec<u8> {
    // Re-encode the question so compression pointers never refer to omitted request records.
    // Its original label case is retained for clients that validate DNS case randomization.
    let mut result = Vec::with_capacity(q.canonical_question.len() + 28);
    result.extend_from_slice(&q.id.to_be_bytes());
    result.extend_from_slice(&(0x8080 | (q.flags & 0x7900) | rcode).to_be_bytes());
    result.extend_from_slice(&1_u16.to_be_bytes());
    result.extend_from_slice(&u16::from(address.is_some()).to_be_bytes());
    result.extend_from_slice(&[0; 4]);
    result.extend_from_slice(&q.canonical_question);
    if let Some(address) = address {
        result.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1]);
        result.extend_from_slice(&60_u32.to_be_bytes());
        result.extend_from_slice(&4_u16.to_be_bytes());
        result.extend_from_slice(&address.octets());
    }
    result
}

struct Policy {
    rules: Vec<(Pattern, std::net::Ipv4Addr)>,
    exceptions: Vec<Pattern>,
    upstreams: Vec<SocketAddr>,
    timeout: Duration,
}

impl Policy {
    fn new(settings: &Settings) -> Result<Self, String> {
        let rules = settings
            .rules
            .iter()
            .map(|r| {
                Pattern::new(&r.mask.to_ascii_lowercase())
                    .map(|p| (p, r.address))
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<_, _>>()?;
        let exceptions = settings
            .exceptions
            .iter()
            .map(|r| Pattern::new(&r.to_ascii_lowercase()).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        let upstreams = settings
            .upstreams
            .iter()
            .map(|r| parse_upstream(r))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            rules,
            exceptions,
            upstreams,
            timeout: Duration::from_millis(settings.timeout_ms),
        })
    }

    fn override_for(&self, name: &str) -> Option<std::net::Ipv4Addr> {
        let options = MatchOptions {
            case_sensitive: false,
            require_literal_separator: false,
            require_literal_leading_dot: false,
        };
        if self
            .exceptions
            .iter()
            .any(|p| p.matches_with(name, options))
        {
            return None;
        }
        self.rules
            .iter()
            .find(|(p, _)| p.matches_with(name, options))
            .map(|(_, ip)| *ip)
    }
}

/// Wait without repeatedly cancelling a blocking Winsock receive. Each socket
/// has one reader and remains owned by that worker through readiness + recv.
fn readable(socket: &UdpSocket, timeout: Duration) -> io::Result<bool> {
    let mut reads = FD_SET {
        fd_count: 1,
        fd_array: [0; 64],
    };
    reads.fd_array[0] = usize::try_from(socket.as_raw_socket()).map_err(io::Error::other)?;
    let timeout = TIMEVAL {
        tv_sec: i32::try_from(timeout.as_secs()).map_err(io::Error::other)?,
        tv_usec: i32::try_from(timeout.subsec_micros()).map_err(io::Error::other)?,
    };
    // SAFETY: std initialized Winsock when binding this live, borrowed socket.
    // The initialized FD_SET contains one valid handle within its capacity;
    // select writes only this stack set, borrows TIMEVAL, and retains neither.
    // No other reader or Drop can consume/close this worker-owned socket here.
    let ready = unsafe {
        select(
            0, // Winsock ignores nfds.
            &mut reads,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &timeout,
        )
    };
    if ready == SOCKET_ERROR {
        // SAFETY: Reads only this thread's Winsock error immediately after select.
        return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }));
    }
    Ok(ready > 0)
}

fn forward(
    packet: &[u8],
    q: &Question,
    policy: &Policy,
    stop: &AtomicBool,
) -> io::Result<(Vec<u8>, SocketAddr)> {
    let deadline = Instant::now() + policy.timeout;
    let mut last_error = io::Error::new(io::ErrorKind::TimedOut, "DNS upstream timeout");
    for (index, address) in policy.upstreams.iter().enumerate() {
        if stop.load(Ordering::Relaxed) || Instant::now() >= deadline {
            break;
        }
        // Reserve part of the shared budget for each remaining upstream. Otherwise a
        // silent first server consumes the whole timeout and backups are never tried.
        let now = Instant::now();
        let remaining = deadline.saturating_duration_since(now);
        let attempts = u32::try_from(policy.upstreams.len() - index).map_err(io::Error::other)?;
        let attempt_deadline = now + remaining / attempts;
        let local: SocketAddr = if address.is_ipv4() {
            "0.0.0.0:0".parse().map_err(io::Error::other)?
        } else {
            "[::]:0".parse().map_err(io::Error::other)?
        };
        let attempt = || -> io::Result<Vec<u8>> {
            let socket = UdpSocket::bind(local)?;
            socket.connect(address)?;
            socket.set_write_timeout(Some(Duration::from_millis(100)))?;
            socket.send(packet)?;
            let mut buf = [0_u8; MAX_PACKET];
            while !stop.load(Ordering::Relaxed) {
                let remaining = attempt_deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                if !readable(&socket, remaining.min(Duration::from_millis(100)))? {
                    continue;
                }
                match socket.recv(&mut buf) {
                    Ok(length) => {
                        let candidate = &buf[..length];
                        if let Ok(answer) = question(candidate)
                            && answer.id == q.id
                            && answer.flags & 0x8000 != 0
                            && answer.flags & 0x7800 == q.flags & 0x7800
                            && answer.count == q.count
                            && answer.name == q.name
                            && answer.qtype == q.qtype
                            && answer.class == q.class
                        {
                            return Ok(candidate.to_vec());
                        }
                        // Ignore unsolicited/mismatched responses within the same bounded budget.
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) => {}
                    Err(error) => return Err(error),
                }
            }
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "DNS upstream timeout",
            ))
        };
        match attempt() {
            Ok(bytes) => return Ok((bytes, *address)),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

struct Log {
    file: Mutex<File>,
    events: SyncSender<QueryEvent>,
}

impl Log {
    fn push(&self, event: QueryEvent) {
        if let Ok(mut file) = self.file.lock() {
            let _ = (|| -> io::Result<()> {
                // Truncating leaves the cursor unchanged: reset both before
                // writing so rotation neither grows forever nor leaves a gap.
                if file.metadata()?.len() > 2 * 1024 * 1024 {
                    file.set_len(0)?;
                    file.rewind()?;
                }
                writeln!(
                    file,
                    "{}\t{}\t{}\t{}\t{}",
                    event.time, event.client, event.domain, event.kind, event.result
                )
            })();
        }
        let _ = self.events.try_send(event);
    }
}

pub struct Service {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    listen_ipv4: SocketAddrV4,
    pub stats: Arc<Stats>,
    pub events: Receiver<QueryEvent>,
    pub started: Instant,
}

impl Service {
    pub fn start(settings: &Settings, log_path: &Path) -> Result<Self, String> {
        settings.validate()?;
        let policy = Arc::new(Policy::new(settings)?);
        let ipv4: IpAddr = settings
            .bind
            .parse()
            .map_err(|e: std::net::AddrParseError| e.to_string())?;
        let addresses = if settings.bind6 == "off" {
            vec![SocketAddr::new(ipv4, settings.port)]
        } else {
            vec![
                SocketAddr::new(ipv4, settings.port),
                SocketAddr::new(
                    settings
                        .bind6
                        .parse()
                        .map_err(|e: std::net::AddrParseError| e.to_string())?,
                    settings.port,
                ),
            ]
        };
        let mut sockets = Vec::new();
        let mut listen_ipv4 = None;
        for address in addresses {
            let socket = UdpSocket::bind(address).map_err(|e| format!("Не удалось открыть {address}. Порт может быть занят другой программой или адрес недоступен.\n{e}"))?;
            if let SocketAddr::V4(address) = socket
                .local_addr()
                .map_err(|e| format!("Не удалось определить адрес DNS: {e}"))?
            {
                listen_ipv4 = Some(address);
            }
            socket.set_read_timeout(None).map_err(|e| e.to_string())?;
            socket
                .set_write_timeout(Some(Duration::from_millis(100)))
                .map_err(|e| e.to_string())?;
            sockets.push(socket);
        }
        let listen_ipv4 =
            listen_ipv4.ok_or_else(|| "Не удалось определить IPv4-адрес DNS.".to_string())?;
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // Windows append-only handles cannot truncate. The profile lock gives
        // this service one writer; its two listeners serialize via Log::file.
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(log_path)
            .map_err(|e| format!("Не удалось открыть журнал: {e}"))?;
        file.seek(SeekFrom::End(0))
            .map_err(|e| format!("Не удалось перейти в конец журнала: {e}"))?;
        let (sender, events) = mpsc::sync_channel(512);
        let log = Arc::new(Log {
            file: Mutex::new(file),
            events: sender,
        });
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Stats::default());
        let mut service = Self {
            stop,
            threads: Vec::new(),
            listen_ipv4,
            stats,
            events,
            started: Instant::now(),
        };
        for socket in sockets {
            let stop = Arc::clone(&service.stop);
            let stats = Arc::clone(&service.stats);
            let policy = Arc::clone(&policy);
            let log = Arc::clone(&log);
            let worker = thread::Builder::new()
                .name("nanoDNS UDP".into())
                .spawn(move || {
                    // The extra byte detects truncation on platforms that return a
                    // shortened datagram instead of reporting WSAEMSGSIZE.
                    let mut buf = [0_u8; MAX_PACKET + 1];
                    while !stop.load(Ordering::Relaxed) {
                        match readable(&socket, Duration::from_millis(150)) {
                            Ok(true) => {}
                            Ok(false) => continue,
                            Err(_) => {
                                stats.failed.fetch_add(1, Ordering::Relaxed);
                                break;
                            }
                        }
                        let (length, client) = match socket.recv_from(&mut buf) {
                            Ok(result) => result,
                            // Windows discards an oversized UDP datagram with 10040.
                            // Malformed network input must not stop the DNS listener.
                            Err(error) if error.raw_os_error() == Some(10040) => continue,
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    io::ErrorKind::WouldBlock
                                        | io::ErrorKind::TimedOut
                                        | io::ErrorKind::ConnectionReset
                                ) =>
                            {
                                continue;
                            }
                            Err(_) => {
                                stats.failed.fetch_add(1, Ordering::Relaxed);
                                break;
                            }
                        };
                        if length > MAX_PACKET {
                            continue;
                        }
                        let packet = &buf[..length];
                        let q = match question(packet) {
                            Ok(q) if q.flags & 0x8000 == 0 => q,
                            _ => continue,
                        };
                        stats.total.fetch_add(1, Ordering::Relaxed);
                        let mut blocked = false;
                        let mut failed = false;
                        let (answer, result) = if q.count == 1
                            && q.class == 1
                            && q.flags & 0x7800 == 0
                            && let Some(ip) = policy.override_for(&q.name)
                        {
                            blocked = true;
                            stats.blocked.fetch_add(1, Ordering::Relaxed);
                            if matches!(q.qtype, 1 | 255) {
                                (response(packet, &q, 0, Some(ip)), ip.to_string())
                            } else {
                                (response(packet, &q, 0, None), "NODATA".into())
                            }
                        } else {
                            match forward(packet, &q, &policy, &stop) {
                                Ok((bytes, upstream)) => {
                                    stats.forwarded.fetch_add(1, Ordering::Relaxed);
                                    (bytes, upstream.to_string())
                                }
                                Err(_) => {
                                    failed = true;
                                    stats.failed.fetch_add(1, Ordering::Relaxed);
                                    (response(packet, &q, 2, None), "SERVFAIL".into())
                                }
                            }
                        };
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        if socket.send_to(&answer, client).is_err() {
                            stats.failed.fetch_add(1, Ordering::Relaxed);
                        }
                        let seconds = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                            % 86400;
                        let event = QueryEvent {
                            time: format!(
                                "{:02}:{:02}:{:02}",
                                seconds / 3600,
                                seconds / 60 % 60,
                                seconds % 60
                            ),
                            domain: q.name,
                            kind: match q.qtype {
                                1 => "A".into(),
                                28 => "AAAA".into(),
                                255 => "ANY".into(),
                                t => t.to_string(),
                            },
                            result,
                            client: client.ip().to_string(),
                            blocked,
                            failed,
                        };
                        log.push(event);
                    }
                })
                .map_err(|e| format!("Не удалось запустить DNS: {e}"))?;
            service.threads.push(worker);
        }
        Ok(service)
    }

    pub fn listen_ipv4(&self) -> SocketAddrV4 {
        self.listen_ipv4
    }

    pub fn healthy(&self) -> bool {
        self.threads.iter().all(|t| !t.is_finished())
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn query(domain: &str, qtype: u16) -> Vec<u8> {
        let mut bytes = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in domain.split('.') {
            bytes.push(u8::try_from(label.len()).unwrap());
            bytes.extend(label.as_bytes());
        }
        bytes.push(0);
        bytes.extend(qtype.to_be_bytes());
        bytes.extend(1_u16.to_be_bytes());
        bytes
    }
    #[test]
    fn default_rules_and_exceptions_preserve_upstream() {
        let policy = Policy::new(&Settings::default()).unwrap();
        for host in [
            "playstation.com",
            "UPDATE.PLAYSTATION.NET",
            "a.playstation.com.cn",
            "a.psndl.net",
        ] {
            assert_eq!(
                policy.override_for(host),
                Some(std::net::Ipv4Addr::UNSPECIFIED),
                "{host}"
            );
        }
        for host in [
            "example.com",
            "feature.api.playstation.com",
            "x.stun.playstation.net",
            "stun.x.playstation.net",
            "post.net.playstation.net",
        ] {
            assert_eq!(policy.override_for(host), None, "{host}");
        }
    }
    #[test]
    fn wildcard_classes_and_order() {
        let settings = Settings {
            rules: crate::config::parse_rules(
                "api??.[ab]lab=10.0.0.1\nx[!0-9].lab=10.0.0.2\n*.lab=10.0.0.3",
            )
            .unwrap(),
            exceptions: vec![],
            ..Settings::default()
        };
        let policy = Policy::new(&settings).unwrap();
        assert_eq!(
            policy.override_for("api12.alab").unwrap().octets(),
            [10, 0, 0, 1]
        );
        assert_eq!(
            policy.override_for("xz.lab").unwrap().octets(),
            [10, 0, 0, 2]
        );
        assert_eq!(
            policy.override_for("x1.lab").unwrap().octets(),
            [10, 0, 0, 3]
        );
    }
    #[test]
    fn a_any_aaaa_and_servfail_wire_headers() {
        for qtype in [1, 255, 28] {
            let request = query("update.playstation.net", qtype);
            let q = question(&request).unwrap();
            let answer = response(
                &request,
                &q,
                0,
                if qtype == 28 {
                    None
                } else {
                    Some(std::net::Ipv4Addr::UNSPECIFIED)
                },
            );
            assert_eq!(word(&answer, 0).unwrap(), 0x1234);
            assert_eq!(word(&answer, 2).unwrap(), 0x8180);
            assert_eq!(word(&answer, 6).unwrap(), u16::from(qtype != 28));
            assert_eq!(question(&answer).unwrap().name, q.name);
            if qtype != 28 {
                assert_eq!(&answer[answer.len() - 4..], &[0; 4]);
            }
            assert_eq!(word(&response(&request, &q, 2, None), 2).unwrap() & 15, 2);
        }
    }
    #[test]
    fn compressed_question_remains_valid_in_local_reply() {
        let original = query("UPDATE.PlayStation.NET", 1);
        let mut compressed = original[..12].to_vec();
        compressed.extend_from_slice(&[0xc0, 18, 0, 1, 0, 1]);
        compressed.extend_from_slice(&original[12..original.len() - 4]);
        let q = question(&compressed).unwrap();
        let answer = response(&compressed, &q, 0, Some(std::net::Ipv4Addr::UNSPECIFIED));
        assert_eq!(question(&answer).unwrap().name, "update.playstation.net");
        assert_eq!(
            &answer[12..12 + q.canonical_question.len()],
            &original[12..]
        );
    }
    #[test]
    fn rejects_truncated_compression_cycles_and_invalid_labels() {
        let packet = query("a.test", 1);
        for length in 0..packet.len() {
            assert!(question(&packet[..length]).is_err());
        }
        let mut cycle = vec![0; 18];
        cycle[5] = 1;
        cycle[12] = 0xc0;
        cycle[13] = 12;
        assert!(question(&cycle).is_err());
        let mut invalid = packet;
        invalid[12] = 64;
        assert!(question(&invalid).is_err());
        // Deterministic arbitrary packets exercise parser bounds and its no-panic contract.
        let mut seed = 5_u64;
        for length in 0..4200 {
            let bytes = (0..length % 512)
                .map(|_| {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    seed.to_le_bytes()[7]
                })
                .collect::<Vec<_>>();
            let _ = question(&bytes);
        }
    }
    #[test]
    fn log_rotation_releases_old_bytes_and_restart_keeps_the_new_tail() {
        let temp = tempfile::tempdir().unwrap();
        let log_path = temp.path().join("test.log");
        fs::write(&log_path, vec![b'x'; 2 * 1024 * 1024 + 1]).unwrap();
        let mut first_tail = Vec::new();
        for domain in ["first.playstation.net", "second.playstation.net"] {
            let reservation = UdpSocket::bind("127.0.0.1:0").unwrap();
            let address = reservation.local_addr().unwrap();
            drop(reservation);
            let settings = Settings {
                bind: "127.0.0.1".into(),
                port: address.port(),
                ..Settings::default()
            };
            let service = Service::start(&settings, &log_path).unwrap();
            let client = UdpSocket::bind("127.0.0.1:0").unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            client.send_to(&query(domain, 1), address).unwrap();
            let mut response = [0; 4096];
            client.recv_from(&mut response).unwrap();
            drop(service); // Includes the log write after send_to.
            let contents = fs::read(&log_path).unwrap();
            assert!(
                contents.len() < 1024,
                "rotation must bound the real disk file"
            );
            assert!(!contents.contains(&0), "rewind must not leave a sparse gap");
            assert!(
                contents.starts_with(&first_tail),
                "restart must preserve the existing tail"
            );
            assert!(String::from_utf8_lossy(&contents).contains(domain));
            first_tail = contents;
        }
    }

    #[test]
    fn real_udp_forward_override_and_cleanup() {
        let upstream = UdpSocket::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        upstream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let worker = thread::spawn(move || {
            let mut buf = [0; 4096];
            let (n, peer) = upstream.recv_from(&mut buf).unwrap();
            let q = question(&buf[..n]).unwrap();
            let answer = response(
                &buf[..n],
                &q,
                0,
                Some(std::net::Ipv4Addr::new(203, 0, 113, 7)),
            );
            upstream.send_to(&answer, peer).unwrap();
        });
        let reservation = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let settings = Settings {
            bind: "127.0.0.1".into(),
            port: address.port(),
            upstreams: vec![upstream_addr.to_string()],
            ..Settings::default()
        };
        let temp = tempfile::tempdir().unwrap();
        let service = Service::start(&settings, &temp.path().join("test.log")).unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut buf = [0; 4096];
        client
            .send_to(&query("x.playstation.net", 1), address)
            .unwrap();
        let (n, _) = client.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[n - 4..n], &[0; 4]);
        client
            .send_to(&query("feature.api.playstation.com", 1), address)
            .unwrap();
        let (n, _) = client.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[n - 4..n], &[203, 0, 113, 7]);
        worker.join().unwrap();
        assert_eq!(service.stats.total.load(Ordering::Relaxed), 2);
        assert_eq!(service.stats.blocked.load(Ordering::Relaxed), 1);
        drop(service);
        assert!(UdpSocket::bind(address).is_ok(), "stop releases listener");
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn specific_ipv4_and_wildcard_coexist_in_both_startup_orders() -> io::Result<()> {
        for wildcard_first in [true, false] {
            let order = if wildcard_first {
                "wildcard first"
            } else {
                "specific first"
            };
            // Retain the chosen high port when the wildcard starts first. In
            // the opposite order, release this reservation before Service binds.
            // Every socket keeps the default options: no SO_REUSEADDR is set.
            let reservation = UdpSocket::bind("0.0.0.0:0")?;
            let port = reservation.local_addr()?.port();
            let wildcard = if wildcard_first {
                Some(reservation)
            } else {
                drop(reservation);
                None
            };
            let address = SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, port);
            let settings = Settings {
                bind: address.ip().to_string(),
                port,
                ..Settings::default()
            };
            let temp = tempfile::tempdir()?;
            let service = Service::start(&settings, &temp.path().join("test.log"))
                .map_err(io::Error::other)?;
            assert_eq!(service.listen_ipv4(), address, "{order}: actual listener");
            let wildcard = match wildcard {
                Some(socket) => socket,
                None => UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, port))?,
            };
            wildcard.set_read_timeout(Some(Duration::from_secs(2)))?;
            let client = UdpSocket::bind("127.0.0.1:0")?;
            client.set_read_timeout(Some(Duration::from_secs(2)))?;
            let mut buf = [0_u8; MAX_PACKET];

            for qtype in [1, 28] {
                client.send_to(&query("update.playstation.net", qtype), address)?;
                let (length, peer) = client.recv_from(&mut buf)?;
                assert_eq!(peer, SocketAddr::V4(address), "{order}: DNS reply source");
                assert!(length >= 12, "{order}: complete DNS header");
                if qtype == 1 {
                    assert_eq!(&buf[length - 4..length], &[0; 4], "{order}: A override");
                } else {
                    assert_eq!(&buf[6..8], &[0, 0], "{order}: AAAA NODATA");
                }
            }

            let untouched = SocketAddrV4::new(std::net::Ipv4Addr::new(127, 0, 0, 2), port);
            client.send_to(b"other-address", untouched)?;
            let (length, peer) = wildcard.recv_from(&mut buf)?;
            assert_eq!(peer, client.local_addr()?, "{order}: wildcard client");
            assert_eq!(&buf[..length], b"other-address", "{order}: other address");
            assert_eq!(
                service.stats.total.load(Ordering::Relaxed),
                2,
                "{order}: specific traffic only"
            );

            drop(service);
            client.send_to(b"specific-released", address)?;
            let (length, _) = wildcard.recv_from(&mut buf)?;
            assert_eq!(
                &buf[..length],
                b"specific-released",
                "{order}: stop releases specific address"
            );
        }
        Ok(())
    }

    #[test]
    fn busy_port_and_timeout_are_bounded() {
        let occupied = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = occupied.local_addr().unwrap();
        let settings = Settings {
            bind: "127.0.0.1".into(),
            port: address.port(),
            ..Settings::default()
        };
        let temp = tempfile::tempdir().unwrap();
        assert!(Service::start(&settings, &temp.path().join("test.log")).is_err());
        let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
        let silent_address = silent.local_addr().unwrap();
        let policy = Policy::new(&Settings {
            upstreams: vec![silent_address.to_string()],
            timeout_ms: 100,
            ..Settings::default()
        })
        .unwrap();
        let packet = query("example.com", 1);
        let q = question(&packet).unwrap();
        let start = Instant::now();
        assert!(forward(&packet, &q, &policy, &AtomicBool::new(false)).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn oversized_datagrams_do_not_stop_or_answer_from_a_truncated_question() {
        let reservation = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let settings = Settings {
            bind: "127.0.0.1".into(),
            port: address.port(),
            ..Settings::default()
        };
        let temp = tempfile::tempdir().unwrap();
        let service = Service::start(&settings, &temp.path().join("test.log")).unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        for length in [MAX_PACKET + 1, MAX_PACKET * 2] {
            let mut oversized = query("update.playstation.net", 1);
            oversized[..2].copy_from_slice(&0xdead_u16.to_be_bytes());
            oversized.resize(length, 0);
            client.send_to(&oversized, address).unwrap();
        }
        client
            .send_to(&query("update.playstation.net", 1), address)
            .unwrap();
        let mut buf = [0_u8; MAX_PACKET];
        let (length, _) = client.recv_from(&mut buf).unwrap();
        assert_eq!(word(&buf[..length], 0).unwrap(), 0x1234);
        assert_eq!(service.stats.total.load(Ordering::Relaxed), 1);
        assert!(
            service.healthy(),
            "oversized input must not kill the listener"
        );
    }

    #[test]
    fn silent_first_upstream_does_not_consume_the_backup_budget() {
        let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
        let backup = UdpSocket::bind("127.0.0.1:0").unwrap();
        backup
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let backup_address = backup.local_addr().unwrap();
        let policy = Policy::new(&Settings {
            upstreams: vec![
                silent.local_addr().unwrap().to_string(),
                backup_address.to_string(),
            ],
            timeout_ms: 600,
            ..Settings::default()
        })
        .unwrap();
        let worker = thread::spawn(move || {
            let mut buf = [0_u8; MAX_PACKET];
            let (length, peer) = backup.recv_from(&mut buf).unwrap();
            let request = &buf[..length];
            let answer = response(request, &question(request).unwrap(), 0, None);
            backup.send_to(&answer, peer).unwrap();
        });
        let packet = query("example.com", 1);
        let q = question(&packet).unwrap();
        let (_, via) = forward(&packet, &q, &policy, &AtomicBool::new(false)).unwrap();
        assert_eq!(via, backup_address);
        worker.join().unwrap();
    }

    #[test]
    fn non_query_opcode_is_forwarded_and_mismatched_response_is_ignored() {
        let upstream = UdpSocket::bind("127.0.0.1:0").unwrap();
        upstream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let reservation = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let settings = Settings {
            bind: "127.0.0.1".into(),
            port: address.port(),
            upstreams: vec![upstream.local_addr().unwrap().to_string()],
            ..Settings::default()
        };
        let worker = thread::spawn(move || {
            let mut buf = [0_u8; MAX_PACKET];
            let (length, peer) = upstream.recv_from(&mut buf).unwrap();
            let request = &buf[..length];
            let q = question(request).unwrap();
            let answer = response(request, &q, 0, None);
            let mut mismatched = answer.clone();
            mismatched[2..4].copy_from_slice(&0x8180_u16.to_be_bytes());
            upstream.send_to(&mismatched, peer).unwrap();
            upstream.send_to(&answer, peer).unwrap();
        });
        let temp = tempfile::tempdir().unwrap();
        let _service = Service::start(&settings, &temp.path().join("test.log")).unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = query("update.playstation.net", 1);
        request[2..4].copy_from_slice(&0x1100_u16.to_be_bytes());
        client.send_to(&request, address).unwrap();
        let mut buf = [0_u8; MAX_PACKET];
        let (length, _) = client.recv_from(&mut buf).unwrap();
        assert_eq!(word(&buf[..length], 2).unwrap(), 0x9180);
        assert_eq!(word(&buf[..length], 6).unwrap(), 0);
        worker.join().unwrap();
    }

    #[test]
    fn stopping_cancels_a_silent_upstream_and_releases_the_listener() {
        let upstream = UdpSocket::bind("127.0.0.1:0").unwrap();
        upstream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let reservation = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let settings = Settings {
            bind: "127.0.0.1".into(),
            port: address.port(),
            timeout_ms: 10_000,
            upstreams: vec![upstream.local_addr().unwrap().to_string()],
            ..Settings::default()
        };
        let temp = tempfile::tempdir().unwrap();
        let service = Service::start(&settings, &temp.path().join("test.log")).unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client.send_to(&query("example.com", 1), address).unwrap();
        let mut buf = [0_u8; MAX_PACKET];
        upstream.recv_from(&mut buf).unwrap();
        let started = Instant::now();
        drop(service);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "stop must cancel the upstream wait"
        );
        assert!(
            UdpSocket::bind(address).is_ok(),
            "stop must release the listener"
        );
    }
}
