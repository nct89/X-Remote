//! Identity this client reports to the UltraViewer server at login.
//!
//! The old build hardcoded a machine id, host name (`HOST-1`), user (`dev`), and
//! hardware id (`000000000000`, a dev-machine MAC). Every install then claimed to
//! be the same machine. Values now come from this machine; the machine id and
//! hardware id are salted hashes of the OS machine id, so the real MAC and
//! machine-id are not sent.

use md5::{Digest, Md5};

/// Fields declared in the login message (command 3) and the session login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    /// 32 uppercase hex chars, same shape as the original machine id.
    pub machine_id: String,
    /// 12 uppercase hex chars, same shape as a MAC address.
    pub hw_id: String,
    pub host_name: String,
    pub user_name: String,
    /// Boot-disk id `model|serial|firmware` (login field 10). The UltraViewer
    /// server **pins the ID to this string** (measured 2026-11-06 via a MITM proxy:
    /// a new serial looks like a new machine; a new MAC or machine id keeps the ID).
    /// It must stay stable for the machine and change only on reinstall. Use the
    /// real disk when it can be read; otherwise a stable string derived from the
    /// machine id.
    pub disk_id: String,
}

impl Ident {
    /// Identity of this machine.
    pub fn current() -> Ident {
        let host_name = host_name();
        let user = user_name();
        let seed = os_machine_id().unwrap_or_else(|| format!("{host_name}/{user}"));
        let mut id = Ident::from_seed(&seed, &host_name, &user);
        // Prefer a real disk id; if it cannot be read, keep the one derived from the machine id.
        if let Some(disk) = real_disk() {
            id.disk_id = disk;
        }
        id
    }

    /// Build an identity from a stable machine seed (split out so tests can call it).
    pub fn from_seed(seed: &str, host_name: &str, user_name: &str) -> Ident {
        let machine_id = hex_upper(&Md5::digest(format!("x-remote/may/{seed}").as_bytes()));
        let hw_id = hex_upper(&Md5::digest(format!("x-remote/phan-cung/{seed}").as_bytes()))[..12].to_string();
        let serial = hex_upper(&Md5::digest(format!("x-remote/o-cung/{seed}").as_bytes()))[..16].to_string();
        Ident {
            machine_id,
            hw_id,
            host_name: clean_token(host_name, "X-REMOTE"),
            user_name: clean_token(user_name, "user"),
            disk_id: format!("X-REMOTE DISK|{serial}|1.0"),
        }
    }
}

/// Clean one part of field 10 (model/serial/firmware): drop `:` and `|` (the
/// separators) and control chars; keep at most 40 chars.
fn clean_disk(s: &str) -> String {
    s.trim()
        .chars()
        .filter(|c| !c.is_control() && *c != ':' && *c != '|')
        .take(40)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Join a cleaned `model|serial|firmware`. Skip when both model and serial are empty.
fn join_disk(model: &str, serial: &str, fw: &str) -> Option<String> {
    let (m, s) = (clean_disk(model), clean_disk(serial));
    if m.is_empty() && s.is_empty() {
        return None;
    }
    let fw = clean_disk(fw);
    Some(format!("{}|{}|{}", if m.is_empty() { "DISK" } else { &m }, s, if fw.is_empty() { "1.0" } else { &fw }))
}

/// Base block device name (e.g. `sda` from `/dev/sda3`, `nvme0n1` from `/dev/nvme0n1p2`).
#[cfg(target_os = "linux")]
fn base_block_dev(dev: &str) -> String {
    let name = dev.rsplit('/').next().unwrap_or(dev);
    if (name.starts_with("nvme") || name.starts_with("mmcblk") || name.starts_with("loop"))
        && name.contains('p')
    {
        if let Some((base, part)) = name.rsplit_once('p') {
            if !base.is_empty() && !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()) {
                return base.to_string();
            }
        }
    }
    let trimmed = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if trimmed.is_empty() {
        name.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Block device backing `/` in `/proc/mounts` (e.g. `sda` or `nvme0n1`).
#[cfg(target_os = "linux")]
fn root_block_dev() -> Option<String> {
    let mounts = std::fs::read_to_string("/proc/mounts").ok()?;
    for line in mounts.lines() {
        let mut it = line.split_whitespace();
        let dev = it.next()?;
        let mnt = it.next()?;
        if mnt == "/" && dev.starts_with("/dev/") {
            return Some(base_block_dev(dev));
        }
    }
    None
}

/// Read the real boot-disk id for this OS (best-effort; no root required).
fn real_disk() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        // `/dev/disk/by-id/` holds links like `ata-VBOX_HARDDISK_VB97e2c89a-...`
        // and `nvme-<model>_<serial>`. Prefer the disk backing `/` and internal
        // non-USB drives so plugging in an external USB disk never shifts the ID.
        if let Ok(rd) = std::fs::read_dir("/dev/disk/by-id") {
            let root_dev = root_block_dev();
            let mut raw_entries: Vec<(String, String)> = Vec::new();
            let mut usb_devs: std::collections::HashSet<String> = std::collections::HashSet::new();
            for entry in rd.flatten() {
                let Ok(name) = entry.file_name().into_string() else { continue };
                if name.contains("-part") {
                    continue;
                }
                if !(name.starts_with("ata-")
                    || name.starts_with("nvme-")
                    || name.starts_with("scsi-")
                    || name.starts_with("usb-"))
                {
                    continue;
                }
                let target = std::fs::read_link(entry.path())
                    .ok()
                    .and_then(|p| p.file_name().and_then(|f| f.to_str()).map(str::to_string))
                    .unwrap_or_default();
                if name.starts_with("usb-") && !target.is_empty() {
                    usb_devs.insert(target.clone());
                }
                raw_entries.push((name, target));
            }
            let mut ranked: Vec<(u8, String)> = raw_entries
                .into_iter()
                .map(|(name, target)| {
                    let is_usb_name = name.starts_with("usb-");
                    let is_usb_dev = !target.is_empty() && usb_devs.contains(&target);
                    let is_root = root_dev.as_deref().is_some_and(|r| r == target);
                    let prio = if is_root && !is_usb_name {
                        0
                    } else if !is_usb_name && !is_usb_dev {
                        1
                    } else if !is_usb_name {
                        2
                    } else {
                        3
                    };
                    (prio, name)
                })
                .collect();
            ranked.sort();
            if let Some((_, n)) = ranked.into_iter().next() {
                // `ata-MODEL_SERIAL` → model|serial.
                let rest = n.splitn(2, '-').nth(1).unwrap_or(&n);
                let (model, serial) = rest.rsplit_once('_').unwrap_or((rest, ""));
                return join_disk(&model.replace('_', " "), serial, "1.0");
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        // ioreg: IOBlockStorageDevice has "Device Characteristics" with Product Name/Serial.
        let out = std::process::Command::new("ioreg")
            .args(["-r", "-c", "IOBlockStorageDevice", "-d", "1"])
            .output()
            .ok()?;
        let txt = String::from_utf8_lossy(&out.stdout);
        // ioreg prints `"Device Characteristics"` as a dict on ONE line:
        // `"Product Name"="APPLE SSD…","Serial Number"="…",…`. Find the key, then
        // the quoted string right after `=`.
        let field = |key: &str| -> String {
            let pat = format!("\"{key}\"");
            let Some(i) = txt.find(&pat) else { return String::new() };
            let after = txt[i + pat.len()..].trim_start_matches([' ', '=']);
            after
                .strip_prefix('"')
                .and_then(|r| r.find('"').map(|e| r[..e].trim().to_string()))
                .unwrap_or_default()
        };
        join_disk(&field("Product Name"), &field("Serial Number"), &field("Product Revision Level"))
    }
    #[cfg(target_os = "windows")]
    {
        // PowerShell: physical disk 0 (the boot disk is usually 0).
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "$d = Get-CimInstance Win32_DiskDrive | Sort-Object Index | Select-Object -First 1; \"$($d.Model)|$($d.SerialNumber)|$($d.FirmwareRevision)\"",
            ])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        let p: Vec<&str> = s.trim().split('|').collect();
        if p.len() == 3 {
            join_disk(p[0], p[1], p[2])
        } else {
            None
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

fn hex_upper(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

/// Drop protocol separators (`:`, `|`) and control chars. Empty becomes `fallback`.
fn clean_token(s: &str, fallback: &str) -> String {
    let s: String = s
        .trim()
        .chars()
        .filter(|c| !c.is_control() && *c != ':' && *c != '|')
        .take(32)
        .collect();
    if s.is_empty() { fallback.to_string() } else { s }
}

fn read_text(p: &str) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn env_var(k: &str) -> Option<String> {
    std::env::var(k).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn host_name() -> String {
    env_var("COMPUTERNAME")
        .or_else(|| read_text("/etc/hostname"))
        .or_else(|| read_text("/proc/sys/kernel/hostname"))
        .or_else(|| env_var("HOSTNAME"))
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_default()
}

fn user_name() -> String {
    env_var("USER").or_else(|| env_var("USERNAME")).or_else(|| env_var("LOGNAME")).unwrap_or_default()
}

/// Stable OS machine id (Linux: machine-id; macOS: IOPlatformUUID).
fn os_machine_id() -> Option<String> {
    if let Some(s) = read_text("/etc/machine-id").or_else(|| read_text("/var/lib/dbus/machine-id")) {
        return Some(s);
    }
    if cfg!(target_os = "macos") {
        let out = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .ok()?;
        let txt = String::from_utf8_lossy(&out.stdout);
        let line = txt.lines().find(|l| l.contains("IOPlatformUUID"))?;
        return line.split('"').nth(3).map(str::to_string);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_format() {
        let a = Ident::from_seed("abc", "may-a", "an");
        assert_eq!(a.machine_id.len(), 32);
        assert!(a.machine_id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));
        assert_eq!(a.hw_id.len(), 12);
        assert_eq!(a, Ident::from_seed("abc", "may-a", "an"), "same machine, same id");
        assert_ne!(a.machine_id, Ident::from_seed("xyz", "may-a", "an").machine_id);
    }

    #[test]
    fn no_hardcoded_identity() {
        let d = Ident::current();
        assert_ne!(d.machine_id, "3E5FC2E19C6337EC046AF62721452A11");
        assert_ne!(d.hw_id, "000000000000");
        assert!(!d.host_name.is_empty() && !d.user_name.is_empty());
    }

    #[test]
    fn strips_protocol_separators() {
        let d = Ident::from_seed("g", " a:b|c\n ", "");
        assert_eq!(d.host_name, "abc");
        assert_eq!(d.user_name, "user");
    }
}
