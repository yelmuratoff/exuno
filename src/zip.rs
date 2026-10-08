//! ZIP archives, the container of a `.skill` package: stored and deflated
//! entries read, and written deterministically. Zip64, encryption, symbolic
//! links, and other compression methods are refused rather than half-read.

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_DIRECTORY: u32 = 0x0605_4b50;
const END_LEN: usize = 22;
const CENTRAL_LEN: usize = 46;
const LOCAL_LEN: usize = 30;
const MAX_COMMENT: usize = 0xFFFF;

const STORED: u16 = 0;
const DEFLATED: u16 = 8;
const ENCRYPTED: u16 = 1;
const UTF8_NAMES: u16 = 1 << 11;
const VERSION: u16 = 20;
const UNIX_HOST: u16 = 3;
const DOS_EPOCH_DATE: u16 = (1 << 5) | 1;

const FILE_TYPE: u32 = 0o170_000;
const DIRECTORY: u32 = 0o040_000;
const SYMLINK: u32 = 0o120_000;
const REGULAR: u32 = 0o100_000;

/// Bytes the entries of one archive may expand to.
pub const MAX_EXPANDED: usize = 256 << 20;

/// One regular file of an archive, at a relative `/`-separated path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub data: Vec<u8>,
    pub executable: bool,
}

/// The regular files of `bytes`, in central-directory order; directory
/// entries are dropped. Fails on a path that would leave the extraction
/// root, a checksum mismatch, or an archive that expands past
/// [`MAX_EXPANDED`].
pub fn read(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let end = find_end(bytes).ok_or("not a ZIP archive")?;
    let count = u16_at(bytes, end + 10).ok_or(TRUNCATED)?;
    let offset = u32_at(bytes, end + 16).ok_or(TRUNCATED)?;
    if count == u16::MAX || offset == u32::MAX {
        return Err(ZIP64.to_string());
    }
    let mut at = offset as usize;
    let mut entries = Vec::new();
    let mut expanded = 0usize;
    for _ in 0..count {
        let (header, next) = central_header(bytes, at)?;
        at = next;
        let Some(entry) = header.entry(bytes, MAX_EXPANDED - expanded)? else {
            continue;
        };
        expanded += entry.data.len();
        entries.push(entry);
    }
    Ok(entries)
}

/// `entries` as a ZIP archive: names flagged UTF-8, Unix modes recorded,
/// timestamps fixed at the DOS epoch so equal input gives equal bytes.
pub fn write(entries: &[Entry]) -> Result<Vec<u8>, String> {
    let count = u16::try_from(entries.len()).map_err(|_| "too many entries for ZIP".to_string())?;
    let mut out = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let deflated = miniz_oxide::deflate::compress_to_vec(&entry.data, 6);
        let (method, body) = if deflated.len() < entry.data.len() {
            (DEFLATED, deflated.as_slice())
        } else {
            (STORED, entry.data.as_slice())
        };
        let name = entry.path.as_bytes();
        let fields = [
            u32::try_from(body.len()),
            u32::try_from(entry.data.len()),
            u32::try_from(out.len()),
        ];
        let [Ok(stored_len), Ok(data_len), Ok(offset)] = fields else {
            return Err(format!("{}: too large for ZIP", entry.path));
        };
        let name_len =
            u16::try_from(name.len()).map_err(|_| format!("{}: name too long", entry.path))?;
        let crc = crc32(&entry.data);
        let shared = |buf: &mut Vec<u8>| {
            put16(buf, VERSION);
            put16(buf, UTF8_NAMES);
            put16(buf, method);
            put16(buf, 0);
            put16(buf, DOS_EPOCH_DATE);
            put32(buf, crc);
            put32(buf, stored_len);
            put32(buf, data_len);
            put16(buf, name_len);
            put16(buf, 0);
        };
        put32(&mut out, LOCAL_HEADER);
        shared(&mut out);
        out.extend_from_slice(name);
        out.extend_from_slice(body);

        let mode = REGULAR | if entry.executable { 0o755 } else { 0o644 };
        put32(&mut central, CENTRAL_HEADER);
        put16(&mut central, (UNIX_HOST << 8) | VERSION);
        shared(&mut central);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put32(&mut central, mode << 16);
        put32(&mut central, offset);
        central.extend_from_slice(name);
    }
    let fields = [u32::try_from(central.len()), u32::try_from(out.len())];
    let [Ok(central_len), Ok(central_offset)] = fields else {
        return Err("archive too large for ZIP".to_string());
    };
    out.extend_from_slice(&central);
    put32(&mut out, END_OF_DIRECTORY);
    put16(&mut out, 0);
    put16(&mut out, 0);
    put16(&mut out, count);
    put16(&mut out, count);
    put32(&mut out, central_len);
    put32(&mut out, central_offset);
    put16(&mut out, 0);
    Ok(out)
}

const TRUNCATED: &str = "truncated ZIP archive";
const ZIP64: &str = "ZIP64 archives are not supported";

struct Header<'a> {
    name: &'a [u8],
    host: u16,
    flags: u16,
    method: u16,
    crc: u32,
    stored_len: u32,
    data_len: u32,
    mode: u32,
    local_offset: u32,
}

fn central_header(bytes: &[u8], at: usize) -> Result<(Header<'_>, usize), String> {
    let fixed = bytes.get(at..at + CENTRAL_LEN).ok_or(TRUNCATED)?;
    if u32_at(fixed, 0) != Some(CENTRAL_HEADER) {
        return Err("corrupt ZIP central directory".to_string());
    }
    let field16 = |offset| usize::from(u16_at(fixed, offset).unwrap_or(0));
    let name_end = at + CENTRAL_LEN + field16(28);
    let name = bytes.get(at + CENTRAL_LEN..name_end).ok_or(TRUNCATED)?;
    let header = Header {
        name,
        host: u16_at(fixed, 4).unwrap_or(0) >> 8,
        flags: u16_at(fixed, 8).unwrap_or(0),
        method: u16_at(fixed, 10).unwrap_or(0),
        crc: u32_at(fixed, 16).unwrap_or(0),
        stored_len: u32_at(fixed, 20).unwrap_or(0),
        data_len: u32_at(fixed, 24).unwrap_or(0),
        mode: u32_at(fixed, 38).unwrap_or(0) >> 16,
        local_offset: u32_at(fixed, 42).unwrap_or(0),
    };
    Ok((header, name_end + field16(30) + field16(32)))
}

impl Header<'_> {
    /// The regular file this record describes, `None` for a directory.
    fn entry(&self, bytes: &[u8], budget: usize) -> Result<Option<Entry>, String> {
        let raw_name =
            std::str::from_utf8(self.name).map_err(|_| "entry name is not UTF-8".to_string())?;
        let mode = if self.host == UNIX_HOST { self.mode } else { 0 };
        if raw_name.ends_with('/') || mode & FILE_TYPE == DIRECTORY {
            return Ok(None);
        }
        let path = safe_path(raw_name)?;
        let fail = |reason: &str| Err(format!("{path}: {reason}"));
        if mode & FILE_TYPE == SYMLINK {
            return fail("symbolic links are not supported");
        }
        if self.flags & ENCRYPTED != 0 {
            return fail("encrypted entries are not supported");
        }
        if [self.stored_len, self.data_len, self.local_offset].contains(&u32::MAX) {
            return Err(ZIP64.to_string());
        }
        let data_len = self.data_len as usize;
        if data_len > budget {
            return fail("archive expands past the size limit");
        }
        let body = self.body(bytes).ok_or(TRUNCATED)?;
        let data = match self.method {
            STORED => body.to_vec(),
            DEFLATED => miniz_oxide::inflate::decompress_to_vec_with_limit(body, data_len)
                .map_err(|_| format!("{path}: corrupt deflate stream"))?,
            other => return fail(&format!("compression method {other} is not supported")),
        };
        if data.len() != data_len || crc32(&data) != self.crc {
            return fail("checksum mismatch");
        }
        Ok(Some(Entry {
            path,
            data,
            executable: mode & 0o111 != 0,
        }))
    }

    /// The stored bytes, past the local header whose name and extra field
    /// may differ in length from the central record's.
    fn body<'b>(&self, bytes: &'b [u8]) -> Option<&'b [u8]> {
        let at = self.local_offset as usize;
        if u32_at(bytes, at)? != LOCAL_HEADER {
            return None;
        }
        let name_len = usize::from(u16_at(bytes, at + 26)?);
        let extra_len = usize::from(u16_at(bytes, at + 28)?);
        let start = at + LOCAL_LEN + name_len + extra_len;
        bytes.get(start..start + self.stored_len as usize)
    }
}

/// `name` as a relative path that stays below the extraction root: `\`
/// read as `/`, empty and `.` segments dropped, and `..`, a leading `/`,
/// or a drive letter refused.
fn safe_path(name: &str) -> Result<String, String> {
    let unified = name.replace('\\', "/");
    let segments: Vec<&str> = unified
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    let escapes = unified.starts_with('/')
        || segments.is_empty()
        || segments
            .iter()
            .any(|segment| *segment == ".." || segment.contains(':'));
    if escapes {
        return Err(format!("{name}: path escapes the archive root"));
    }
    Ok(segments.join("/"))
}

fn find_end(bytes: &[u8]) -> Option<usize> {
    let last = bytes.len().checked_sub(END_LEN)?;
    let first = last.saturating_sub(MAX_COMMENT);
    (first..=last)
        .rev()
        .find(|&at| u32_at(bytes, at) == Some(END_OF_DIRECTORY))
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let pair = bytes.get(at..at + 2)?;
    Some(u16::from_le_bytes([pair[0], pair[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let quad = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
}

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut crc = index as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                0xEDB8_8320 ^ (crc >> 1)
            };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
};

fn crc32(data: &[u8]) -> u32 {
    !data.iter().fold(!0u32, |crc, &byte| {
        CRC_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, data: &[u8], executable: bool) -> Entry {
        Entry {
            path: path.to_string(),
            data: data.to_vec(),
            executable,
        }
    }

    /// An archive of one stored entry named `name`, as another tool might
    /// have written it.
    fn archive_named(name: &str) -> Vec<u8> {
        let mut bytes = write(&[entry("placeholder", b"x", false)]).unwrap();
        let wanted = name.as_bytes();
        assert_eq!(wanted.len(), "placeholder".len());
        for _ in 0..2 {
            let at = bytes
                .windows(wanted.len())
                .position(|window| window == b"placeholder")
                .unwrap();
            bytes[at..at + wanted.len()].copy_from_slice(wanted);
        }
        bytes
    }

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn reads_an_info_zip_archive() {
        let bytes = include_bytes!("../tests/fixtures/info-zip-demo.zip");
        let entries = read(bytes).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["demo/run.sh", "demo/SKILL.md"]);
        assert_eq!(entries[0].data, b"#!/bin/sh\n");
        assert!(entries[0].executable);
        assert!(
            String::from_utf8_lossy(&entries[1].data)
                .starts_with("---\nname: demo\ndescription: Demo skill.\n---\n")
        );
        assert!(!entries[1].executable);
    }

    #[test]
    fn written_entries_read_back_unchanged() {
        let entries = [
            entry("a/SKILL.md", "повтор ".repeat(200).as_bytes(), false),
            entry("a/scripts/run.sh", b"#!/bin/sh\n", true),
            entry("a/empty", b"", false),
        ];
        assert_eq!(read(&write(&entries).unwrap()).unwrap(), entries);
    }

    #[test]
    fn writing_is_deterministic() {
        let entries = [entry("x/y.md", b"same", false)];
        assert_eq!(write(&entries).unwrap(), write(&entries).unwrap());
    }

    #[test]
    fn compressible_data_is_deflated_and_incompressible_data_stored() {
        let bytes = write(&[entry("big", &[b'a'; 4096], false)]).unwrap();
        assert!(bytes.len() < 4096);
        assert_eq!(u16_at(&bytes, 8), Some(DEFLATED));
        let bytes = write(&[entry("tiny", b"ab", false)]).unwrap();
        assert_eq!(u16_at(&bytes, 8), Some(STORED));
    }

    #[test]
    fn a_trailing_comment_does_not_hide_the_directory() {
        let mut bytes = write(&[entry("f", b"data", false)]).unwrap();
        let len = bytes.len();
        bytes[len - 2..].copy_from_slice(&7u16.to_le_bytes());
        bytes.extend_from_slice(b"comment");
        assert_eq!(read(&bytes).unwrap()[0].data, b"data");
    }

    #[test]
    fn paths_that_leave_the_root_are_refused() {
        for name in ["../escape.x", "/abs/olute", "c:/windows", "a/../../bb"] {
            let padded = format!("{name:x<11}");
            let error = read(&archive_named(&padded[..11])).unwrap_err();
            assert!(
                error.contains("escapes the archive root"),
                "{name}: {error}"
            );
        }
    }

    #[test]
    fn backslashes_and_dot_segments_are_normalized() {
        assert_eq!(safe_path("a\\b/./c").unwrap(), "a/b/c");
        assert_eq!(safe_path("./a//b").unwrap(), "a/b");
    }

    #[test]
    fn a_corrupted_entry_fails_its_checksum() {
        let mut bytes = write(&[entry("f", b"data", false)]).unwrap();
        let at = bytes.windows(4).position(|w| w == b"data").unwrap();
        bytes[at] = b'D';
        assert!(read(&bytes).unwrap_err().contains("checksum mismatch"));
    }

    #[test]
    fn encrypted_and_symlinked_entries_are_refused() {
        let mut bytes = write(&[entry("f", b"data", false)]).unwrap();
        let central = bytes
            .windows(4)
            .rposition(|w| w == CENTRAL_HEADER.to_le_bytes())
            .unwrap();
        bytes[central + 8] |= ENCRYPTED as u8;
        assert!(read(&bytes).unwrap_err().contains("encrypted"));

        let mut bytes = write(&[entry("f", b"data", false)]).unwrap();
        let central = bytes
            .windows(4)
            .rposition(|w| w == CENTRAL_HEADER.to_le_bytes())
            .unwrap();
        bytes[central + 38..central + 42].copy_from_slice(&((SYMLINK | 0o777) << 16).to_le_bytes());
        assert!(read(&bytes).unwrap_err().contains("symbolic links"));
    }

    #[test]
    fn an_entry_past_the_size_limit_is_refused_before_inflating() {
        let bytes = write(&[entry("f", b"data", false)]).unwrap();
        let central = bytes
            .windows(4)
            .rposition(|w| w == CENTRAL_HEADER.to_le_bytes())
            .unwrap();
        let (header, _) = central_header(&bytes, central).unwrap();
        assert!(header.entry(&bytes, 3).unwrap_err().contains("size limit"));
    }

    #[test]
    fn zip64_and_non_zip_input_are_refused() {
        let mut bytes = write(&[entry("f", b"data", false)]).unwrap();
        let len = bytes.len();
        bytes[len - 6..len - 2].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read(&bytes).unwrap_err(), ZIP64);
        assert_eq!(read(b"plain text").unwrap_err(), "not a ZIP archive");
    }
}
