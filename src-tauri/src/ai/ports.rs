//! Port ownership inspection and safe clearing for the two app-managed
//! llama-server engines (chat + embed).
//!
//! `LlamaEngine::stop_*` can only kill a `Child` handle the *current* process
//! owns. When Arnold is force-killed or crashes, its `llama-server.exe`
//! children survive, keep :8080/:8081 bound, and the next launch / Stop /
//! Restart fails with "port is already in use" even though nothing appears to
//! be running. This module finds the listener on a port and reaps it when it
//! is a llama-server, so the in-app Stop / Restart controls recover without
//! manual Task Manager work.
//!
//! Kill policy (user-approved): any process whose name is `llama-server*` may
//! be reaped automatically. Anything else is never killed implicitly — it is
//! reported by pid/name and requires the explicit "Clear port" action.

use serde::Serialize;
use std::process::Command;
use sysinfo::{Pid, System};

/// Who is listening on a TCP port, as far as Windows will tell us.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PortOwner {
    pub port: u16,
    pub pid: u32,
    pub name: String,
    pub exe: Option<String>,
    pub command: String,
    /// True when the executable name is `llama-server*` — the only kind Arnold
    /// reaps without asking.
    pub is_llama_server: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortRole {
    Chat,
    Embed,
}

impl PortRole {
    pub fn label(self) -> &'static str {
        match self {
            PortRole::Chat => "chat",
            PortRole::Embed => "embed",
        }
    }

    /// The setting a user changes if they would rather move the port than
    /// clear it.
    pub fn setting(self) -> &'static str {
        match self {
            PortRole::Chat => "llama_chat_port",
            PortRole::Embed => "llama_embed_port",
        }
    }

    fn other_label(self) -> &'static str {
        match self {
            PortRole::Chat => "embed",
            PortRole::Embed => "chat",
        }
    }
}

/// Outcome of trying to free a port for one of Arnold's engines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortClearance {
    /// Nothing is listening.
    Free,
    /// Held by the engine this caller is about to (re)start — leave it alone;
    /// `start_*` is idempotent.
    OwnEngine(PortOwner),
    /// A llama-server was killed to free the port.
    Reaped(PortOwner),
    /// Someone else holds it; Arnold will not kill it implicitly.
    Blocked { owner: PortOwner, reason: String },
}

/// Pure kill decision, separated from the OS calls so it can be table-tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortDecision {
    Free,
    Own,
    OtherEngine,
    Reap,
    Refuse,
}

impl PortClearance {
    /// One-line human summary for the UI, if anything notable happened.
    pub fn note(&self, role: PortRole) -> Option<String> {
        match self {
            PortClearance::Reaped(owner) => Some(format!(
                "freed port {} by stopping a stray llama-server (pid {})",
                owner.port, owner.pid
            )),
            PortClearance::Blocked { reason, .. } => Some(reason.clone()),
            PortClearance::Free | PortClearance::OwnEngine(_) => {
                let _ = role;
                None
            }
        }
    }
}

/// `llama-server.exe`, `llama-server`, `LLAMA-SERVER.EXE` -> true.
/// `llama-server-impl.dll`, `python.exe`, `my-llama-server.exe` -> false.
pub fn is_llama_server_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    let Some(stem) = lower.strip_suffix(".exe") else {
        return false;
    };
    stem.starts_with("llama-server")
}

/// Find the pid owning a LISTENING socket for `port` in `netstat -ano` output.
/// UDP rows (no state column) and TIME_WAIT rows are ignored.
pub fn parse_netstat_listener(output: &str, port: u16) -> Option<u32> {
    for line in output.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 {
            continue;
        }
        let proto = cols[0];
        if !proto.eq_ignore_ascii_case("tcp") {
            continue;
        }
        let state = cols[3];
        if !state.eq_ignore_ascii_case("LISTENING") {
            continue;
        }
        let Some(local_port) = cols[1]
            .rsplit_once(':')
            .and_then(|(_, p)| p.parse::<u16>().ok())
        else {
            continue;
        };
        if local_port != port {
            continue;
        }
        if let Some(pid) = cols[4].parse::<u32>().ok() {
            return Some(pid);
        }
    }
    None
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Describe a pid using sysinfo. Unresolvable processes are reported with an
/// empty name and `is_llama_server = false` — the safe default (never kill what
/// we could not identify).
pub fn describe_pid(sys: &System, port: u16, pid: u32) -> PortOwner {
    let proc = sys.process(Pid::from_u32(pid));
    let name = proc.map(|p| p.name().to_string()).unwrap_or_default();
    let exe = proc
        .and_then(|p| p.exe())
        .map(|p| p.to_string_lossy().into_owned());
    let command = proc
        .map(|p| truncate(&p.cmd().join(" "), 400))
        .unwrap_or_default();
    let exe_name = exe
        .as_deref()
        .and_then(|e| e.rsplit(['\\', '/']).next())
        .unwrap_or("");
    let is_llama_server = is_llama_server_name(&name) || is_llama_server_name(exe_name);
    PortOwner {
        port,
        pid,
        name,
        exe,
        command,
        is_llama_server,
    }
}

/// Pid of the process LISTENING on `port`, via `netstat` (Windows builtin, no
/// extra dependency, works without elevation for same-session sockets).
pub fn listener_pid(port: u16) -> Option<u32> {
    let output = Command::new("netstat")
        .args(["-ano", "-p", "TCP"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_netstat_listener(&String::from_utf8_lossy(&output.stdout), port)
}

pub fn owner_of_blocking(port: u16) -> Option<PortOwner> {
    let pid = listener_pid(port)?;
    let sys = System::new_all();
    Some(describe_pid(&sys, port, pid))
}

pub fn classify(
    owner: Option<&PortOwner>,
    own_pid: Option<u32>,
    other_pid: Option<u32>,
) -> PortDecision {
    match owner {
        None => PortDecision::Free,
        Some(o) if Some(o.pid) == own_pid => PortDecision::Own,
        Some(o) if Some(o.pid) == other_pid => PortDecision::OtherEngine,
        Some(o) if o.is_llama_server => PortDecision::Reap,
        Some(_) => PortDecision::Refuse,
    }
}

fn describe_owner(owner: &PortOwner) -> String {
    if owner.name.trim().is_empty() {
        format!("an unidentified process (pid {})", owner.pid)
    } else {
        format!("{} (pid {})", owner.name, owner.pid)
    }
}

pub fn blocked_reason(role: PortRole, owner: &PortOwner) -> String {
    format!(
        "Port {} is held by {}, which is not a llama-server. Arnold will not stop it automatically - use \"Clear port\" in System to force it, or change {} in Settings.",
        owner.port,
        describe_owner(owner),
        role.setting()
    )
}

pub fn duplicate_port_reason(role: PortRole, owner: &PortOwner) -> String {
    format!(
        "Port {} is held by Arnold's own {} engine (pid {}). llama_chat_port and llama_embed_port must differ - change one in Settings > LLM Engine.",
        owner.port,
        role.other_label(),
        owner.pid
    )
}

/// Terminate one pid: sysinfo (TerminateProcess) first, `taskkill /T /F` as a
/// fallback for processes sysinfo cannot signal.
pub fn kill_pid(pid: u32) -> Result<(), String> {
    let sys = System::new_all();
    if let Some(proc) = sys.process(Pid::from_u32(pid)) {
        if proc.kill() {
            return Ok(());
        }
    }
    let output = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output()
        .map_err(|e| format!("could not run taskkill: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

/// Wait until the socket is released (TerminateProcess is asynchronous with
/// respect to the kernel tearing down the listener).
fn wait_until_free(port: u16, pid: u32) -> bool {
    for _ in 0..20 {
        match listener_pid(port) {
            None => return true,
            Some(other) if other != pid => return true,
            Some(_) => std::thread::sleep(std::time::Duration::from_millis(150)),
        }
    }
    false
}

fn failed_reason(role: PortRole, owner: &PortOwner, detail: &str) -> String {
    format!(
        "Port {} is held by {} and could not be freed: {}. Stop it manually, or change {} in Settings.",
        owner.port,
        describe_owner(owner),
        detail,
        role.setting()
    )
}

fn unknown_owner(port: u16) -> PortOwner {
    PortOwner {
        port,
        pid: 0,
        name: String::new(),
        exe: None,
        command: String::new(),
        is_llama_server: false,
    }
}

pub fn clear_port_blocking(
    role: PortRole,
    port: u16,
    own_pid: Option<u32>,
    other_pid: Option<u32>,
) -> PortClearance {
    let owner = match owner_of_blocking(port) {
        Some(o) => o,
        None => return PortClearance::Free,
    };
    match classify(Some(&owner), own_pid, other_pid) {
        PortDecision::Free => PortClearance::Free,
        PortDecision::Own => PortClearance::OwnEngine(owner),
        PortDecision::OtherEngine => PortClearance::Blocked {
            reason: duplicate_port_reason(role, &owner),
            owner,
        },
        PortDecision::Refuse => PortClearance::Blocked {
            reason: blocked_reason(role, &owner),
            owner,
        },
        PortDecision::Reap => match kill_pid(owner.pid) {
            Ok(()) if wait_until_free(port, owner.pid) => PortClearance::Reaped(owner),
            Ok(()) => PortClearance::Blocked {
                reason: failed_reason(role, &owner, "it exited but the port is still bound"),
                owner,
            },
            Err(e) => PortClearance::Blocked {
                reason: failed_reason(role, &owner, &e),
                owner,
            },
        },
    }
}

/// Explicit, user-confirmed kill of whatever holds the port — including
/// non-llama processes. Still refuses Arnold's own live engine.
pub fn force_clear_blocking(
    role: PortRole,
    port: u16,
    own_pid: Option<u32>,
) -> Result<Option<PortOwner>, String> {
    let Some(owner) = owner_of_blocking(port) else {
        return Ok(None);
    };
    if Some(owner.pid) == own_pid {
        return Err(format!(
            "Port {} is held by Arnold's own {} engine (pid {}) - use Stop instead.",
            port,
            role.label(),
            owner.pid
        ));
    }
    kill_pid(owner.pid)?;
    if !wait_until_free(port, owner.pid) {
        return Err(failed_reason(
            role,
            &owner,
            "it exited but the port is still bound",
        ));
    }
    Ok(Some(owner))
}

pub async fn owner_of(port: u16) -> Option<PortOwner> {
    tokio::task::spawn_blocking(move || owner_of_blocking(port))
        .await
        .ok()
        .flatten()
}

pub async fn clear_port(
    role: PortRole,
    port: u16,
    own_pid: Option<u32>,
    other_pid: Option<u32>,
) -> PortClearance {
    tokio::task::spawn_blocking(move || clear_port_blocking(role, port, own_pid, other_pid))
        .await
        .unwrap_or_else(|e| PortClearance::Blocked {
            reason: format!("Port {} could not be inspected: {}", port, e),
            owner: unknown_owner(port),
        })
}

pub async fn force_clear(
    role: PortRole,
    port: u16,
    own_pid: Option<u32>,
) -> Result<Option<PortOwner>, String> {
    tokio::task::spawn_blocking(move || force_clear_blocking(role, port, own_pid))
        .await
        .map_err(|e| format!("Port {} could not be cleared: {}", port, e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Active Connections\r\n\r\n  Proto  Local Address          Foreign Address        State           PID\r\n  TCP    127.0.0.1:8080         0.0.0.0:0              LISTENING       13008\r\n  TCP    127.0.0.1:8080         127.0.0.1:1833         TIME_WAIT       0\r\n  TCP    127.0.0.1:80810        0.0.0.0:0              LISTENING       999\r\n  TCP    [::]:8081              [::]:0                 LISTENING       6272\r\n  UDP    0.0.0.0:8080           *:*                                    4242\r\n";

    fn owner(pid: u32, name: &str, is_llama: bool) -> PortOwner {
        PortOwner {
            port: 8080,
            pid,
            name: name.to_string(),
            exe: Some(format!("C:\\tools\\{}", name)),
            command: String::new(),
            is_llama_server: is_llama,
        }
    }

    #[test]
    fn parse_netstat_finds_only_the_listening_row_for_the_port() {
        assert_eq!(parse_netstat_listener(SAMPLE, 8080), Some(13008));
        // IPv6 listener rows use the same TCP proto column.
        assert_eq!(parse_netstat_listener(SAMPLE, 8081), Some(6272));
        // 8080 must not match 80810, and unused ports yield nothing.
        assert_eq!(parse_netstat_listener(SAMPLE, 808), None);
        assert_eq!(parse_netstat_listener(SAMPLE, 8082), None);
        assert_eq!(parse_netstat_listener("", 8080), None);
        // LF-only output parses the same way.
        assert_eq!(
            parse_netstat_listener(
                "  TCP    127.0.0.1:8080    0.0.0.0:0    LISTENING    42\n",
                8080
            ),
            Some(42)
        );
    }

    #[test]
    fn llama_server_name_matching_is_narrow() {
        assert!(is_llama_server_name("llama-server.exe"));
        assert!(is_llama_server_name("LLAMA-SERVER.EXE"));
        assert!(is_llama_server_name("llama-server-impl.exe"));
        assert!(!is_llama_server_name("llama-server"));
        assert!(!is_llama_server_name("llama-server-impl.dll"));
        assert!(!is_llama_server_name("python.exe"));
        assert!(!is_llama_server_name("my-llama-server.exe"));
        assert!(!is_llama_server_name(""));
    }

    #[test]
    fn classify_never_reaps_a_foreign_process() {
        let llama = owner(13008, "llama-server.exe", true);
        let python = owner(4242, "python.exe", false);

        assert_eq!(classify(None, None, None), PortDecision::Free);
        assert_eq!(
            classify(Some(&llama), Some(13008), None),
            PortDecision::Own,
            "the caller's own engine must not be killed"
        );
        assert_eq!(
            classify(Some(&llama), None, Some(13008)),
            PortDecision::OtherEngine,
            "the sibling engine means a duplicate-port config"
        );
        assert_eq!(classify(Some(&llama), None, None), PortDecision::Reap);
        assert_eq!(classify(Some(&python), None, None), PortDecision::Refuse);
    }

    #[test]
    fn blocked_reasons_name_the_process_and_the_setting_to_change() {
        let python = owner(4242, "python.exe", false);
        let foreign = blocked_reason(PortRole::Chat, &python);
        assert!(foreign.contains("python.exe (pid 4242)"), "{}", foreign);
        assert!(foreign.contains("llama_chat_port"), "{}", foreign);
        assert!(foreign.contains("Clear port"), "{}", foreign);

        let dup = duplicate_port_reason(PortRole::Chat, &owner(6272, "llama-server.exe", true));
        assert!(dup.contains("Arnold's own embed engine"), "{}", dup);
        assert!(
            dup.contains("llama_chat_port and llama_embed_port"),
            "{}",
            dup
        );
    }

    #[test]
    fn port_owner_serializes_snake_case_wire_keys() {
        let v = serde_json::to_value(owner(13008, "llama-server.exe", true)).unwrap();
        assert_eq!(v["port"], 8080);
        assert_eq!(v["pid"], 13008);
        assert_eq!(v["is_llama_server"], true);
        assert!(v.get("isLlamaServer").is_none());
    }
}
