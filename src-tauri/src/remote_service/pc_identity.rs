//! The shack PC, as the Remote service is told about it when approving a station may start a trial:
//! one trial per PC, beside one per mailbox and one per callsign. Two identifiers, because each fails
//! differently:
//!
//! - `hw`, the SMBIOS system UUID (on Windows read from the firmware's own table), which survives a
//!   reinstall and changes with the motherboard or a new virtual machine. Many boards ship a
//!   placeholder (all zero, all F, or the well-known `03000200-0400-0500-0006-000700080009`); that is
//!   no identifier, and every such board would look like one PC, so it is sent as null.
//! - `os`, the operating system installation's id (Windows `MachineGuid`, Linux `/etc/machine-id`),
//!   which a reinstall changes and a cloned disk keeps.
//!
//! The raw values never leave this computer. Each is sent as `SHA-256("nexus-pc:" + kind + ":" +
//! value)` in lowercase hex, with the value trimmed and lower-cased first, and the service keys that
//! again before it stores anything. Either value can be changed by an administrator: this is a speed
//! bump, not a wall. A value that cannot be read is null, and so is every value on a platform with no
//! read here (macOS today).

use serde_json::{json, Value};

/// What the approval carries as `pc`: both hashes, either of them null.
pub(super) fn pc() -> Value {
    json!({ "hw": hw().map(|value| hash("hw", &value)), "os": os().map(|value| hash("os", &value)) })
}

/// The hash the service is sent for one identifier.
fn hash(kind: &str, value: &str) -> String {
    let digest = ring::digest::digest(
        &ring::digest::SHA256,
        format!("nexus-pc:{kind}:{value}").as_bytes(),
    );
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// An identifier in the one form it is hashed in, or `None` when there is nothing to send.
fn canonical(value: &str) -> Option<String> {
    let value = value
        .trim()
        .trim_end_matches('\0')
        .trim()
        .to_ascii_lowercase();
    (!value.is_empty()).then_some(value)
}

/// System UUIDs that boards ship as placeholders, in the form `smbios_uuid` writes them.
const PLACEHOLDER_UUIDS: [&str; 3] = [
    "00000000-0000-0000-0000-000000000000",
    "ffffffff-ffff-ffff-ffff-ffffffffffff",
    "03000200-0400-0500-0006-000700080009",
];

/// The system UUID in a raw SMBIOS table, as Windows returns the `RSMB` firmware table: an 8-byte
/// header (calling method, major and minor version, DMI revision, table length), then the
/// structures. The UUID is bytes 8..24 of the System Information structure (type 1, at least 0x19
/// bytes long); from SMBIOS 2.6 its first three fields are little-endian, which is how Windows
/// itself prints it. `None` for no such structure, a table that does not parse, or a placeholder.
fn smbios_uuid(raw: &[u8]) -> Option<String> {
    let (major, minor) = (*raw.get(1)?, *raw.get(2)?);
    let length = u32::from_le_bytes(raw.get(4..8)?.try_into().ok()?) as usize;
    let table = raw.get(8..8usize.checked_add(length)?)?;
    let mut at = 0usize;
    while at + 4 <= table.len() {
        let (kind, size) = (table[at], usize::from(table[at + 1]));
        if size < 4 || at + size > table.len() {
            return None;
        }
        if kind == 1 && size >= 0x19 {
            let b = &table[at + 8..at + 24];
            let order: [usize; 16] = if (major, minor) >= (2, 6) {
                [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15]
            } else {
                [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
            };
            let hex: String = order.iter().map(|&i| format!("{:02x}", b[i])).collect();
            let uuid = format!(
                "{}-{}-{}-{}-{}",
                &hex[0..8],
                &hex[8..12],
                &hex[12..16],
                &hex[16..20],
                &hex[20..32]
            );
            return (!PLACEHOLDER_UUIDS.contains(&uuid.as_str())).then_some(uuid);
        }
        if kind == 127 {
            return None;
        }
        // Past the formatted area, then past its strings, which end with two NULs.
        let strings = at + size;
        let end = table
            .get(strings..)?
            .windows(2)
            .position(|pair| pair == [0, 0])?;
        at = strings + end + 2;
    }
    None
}

#[cfg(windows)]
fn hw() -> Option<String> {
    use windows_sys::Win32::System::SystemInformation::{GetSystemFirmwareTable, RSMB};
    // SAFETY: a null buffer of size 0 asks only for the size the table needs.
    let size = unsafe { GetSystemFirmwareTable(RSMB, 0, std::ptr::null_mut(), 0) };
    if size == 0 || size > 1 << 20 {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    // SAFETY: `buffer` is `size` bytes long, and the call writes at most that many.
    let read = unsafe { GetSystemFirmwareTable(RSMB, 0, buffer.as_mut_ptr(), size) };
    if read == 0 || read > size {
        return None;
    }
    buffer.truncate(read as usize);
    smbios_uuid(&buffer)
}

#[cfg(not(windows))]
fn hw() -> Option<String> {
    // Linux keeps the system UUID readable by root only, and Nexus does not run as root.
    let _ = smbios_uuid;
    None
}

#[cfg(windows)]
fn os() -> Option<String> {
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY,
    };
    let key: Vec<u16> = "SOFTWARE\\Microsoft\\Cryptography\0"
        .encode_utf16()
        .collect();
    let name: Vec<u16> = "MachineGuid\0".encode_utf16().collect();
    let mut buffer = [0u16; 64];
    let mut bytes = std::mem::size_of_val(&buffer) as u32;
    // SAFETY: both names are NUL-terminated, and `bytes` is the size of `buffer` in bytes, which the
    // call writes no more than (it fails rather than overruns).
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != 0 {
        return None;
    }
    let units = (bytes as usize / 2).min(buffer.len());
    canonical(&String::from_utf16_lossy(&buffer[..units]))
}

#[cfg(target_os = "linux")]
fn os() -> Option<String> {
    std::fs::read_to_string("/etc/machine-id")
        .ok()
        .and_then(|id| canonical(&id))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn os() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An RSMB buffer holding a BIOS structure (type 0, with strings), then a System Information
    /// structure carrying `uuid` as raw bytes, then the end-of-table structure.
    fn table(major: u8, minor: u8, uuid: [u8; 16]) -> Vec<u8> {
        let mut structures = vec![0u8, 0x18, 0x00, 0x00];
        structures.extend([1u8; 0x14]);
        structures.extend(b"Vendor\0Version\0\0");
        let mut system = vec![1u8, 0x1b, 0x01, 0x00, 1, 2, 3, 4];
        system.extend(uuid);
        system.extend([6, 0, 0]);
        structures.extend(system);
        structures.extend(b"Maker\0\0");
        structures.extend([127u8, 4, 0xff, 0xff, 0, 0]);
        let mut raw = vec![0u8, major, minor, 0];
        raw.extend((structures.len() as u32).to_le_bytes());
        raw.extend(structures);
        raw
    }
    const UUID: [u8; 16] = [
        0x44, 0x45, 0x4c, 0x4c, 0x52, 0x00, 0x10, 0x34, 0x80, 0x48, 0xb4, 0xc0, 0x4f, 0x35, 0x31,
        0x32,
    ];

    #[test]
    fn the_system_uuid_is_read_from_the_firmware_table_in_the_order_windows_prints_it() {
        assert_eq!(
            smbios_uuid(&table(3, 4, UUID)).as_deref(),
            Some("4c4c4544-0052-3410-8048-b4c04f353132")
        );
        // Before SMBIOS 2.6 the bytes are in the order they are stored.
        assert_eq!(
            smbios_uuid(&table(2, 5, UUID)).as_deref(),
            Some("44454c4c-5200-1034-8048-b4c04f353132")
        );
    }

    #[test]
    fn a_placeholder_uuid_or_a_table_that_does_not_parse_is_no_identifier() {
        assert_eq!(smbios_uuid(&table(3, 4, [0; 16])), None);
        assert_eq!(smbios_uuid(&table(3, 4, [0xff; 16])), None);
        let placeholder = [0, 2, 0, 3, 0, 4, 0, 5, 0, 6, 0, 7, 0, 8, 0, 9];
        assert_eq!(smbios_uuid(&table(3, 4, placeholder)), None);
        // The control: one byte off the placeholder is a real UUID.
        let mut near = placeholder;
        near[15] = 0x0a;
        assert_eq!(
            smbios_uuid(&table(3, 4, near)).as_deref(),
            Some("03000200-0400-0500-0006-00070008000a")
        );
        let whole = table(3, 4, UUID);
        for cut in [0, 7, 12, whole.len() - 8] {
            assert_eq!(smbios_uuid(&whole[..cut]), None, "cut at {cut}");
        }
        let mut short = whole.clone();
        short[8 + 0x18 + 0x10 + 1] = 2; // the System Information structure claims a size below 4
        assert_eq!(smbios_uuid(&short), None);
    }

    #[test]
    fn each_identifier_is_sent_as_the_sha256_of_its_kind_and_canonical_value_never_the_value() {
        let guid = "6E1F2A3B-0C4D-4E5F-8A9B-0C1D2E3F4A5B";
        let sent = hash("os", &canonical(&format!(" {guid}\0")).unwrap());
        // SHA-256("nexus-pc:os:6e1f2a3b-0c4d-4e5f-8a9b-0c1d2e3f4a5b"), computed independently (sha256sum).
        assert_eq!(
            sent,
            "daf4bc5078174d7a72ede8d19eaf4f8006938310a43a9c69095a09504df937a9"
        );
        assert!(!sent.contains("6e1f2a3b"));
        assert_ne!(
            hash("hw", "x"),
            hash("os", "x"),
            "the kind is part of what is hashed"
        );
        assert_eq!(canonical(" \0"), None);
        let value = pc();
        for kind in ["hw", "os"] {
            let field = &value[kind];
            assert!(
                field.is_null()
                    || field.as_str().is_some_and(|h| h.len() == 64
                        && h.bytes()
                            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())),
                "{kind}: {field}"
            );
        }
    }
}
