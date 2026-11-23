//! Bounded, local file inspection for navigation by byte offset.
use super::Rows;
use std::{io::Read, path::Path};

const LIMIT: u64 = 1024 * 1024;

fn read_file(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    crate::util::open_regular(path)?
        .take(LIMIT)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn points(bytes: &[u8]) -> Vec<(usize, String)> {
    if bytes.is_empty() {
        return vec![];
    }
    let format = [
        (&b"\x7fELF"[..], "ELF header"),
        (&b"\x89PNG\r\n\x1a\n"[..], "PNG signature"),
        (&b"PK\x03\x04"[..], "ZIP local file header"),
        (&b"%PDF-"[..], "PDF header"),
        (&b"\x1f\x8b"[..], "Gzip header"),
        (&b"MZ"[..], "DOS executable header"),
    ]
    .iter()
    .find_map(|&(magic, format)| bytes.starts_with(magic).then_some(format));
    let mut rows = vec![(0, format.unwrap_or("Beginning of file").into())];
    if format == Some("PNG signature") {
        let mut offset = 8;
        let mut seen = std::collections::HashSet::new();
        while let Some(header) = bytes.get(offset..offset + 8) {
            let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
            let Some(end) = offset
                .checked_add(12)
                .and_then(|base| base.checked_add(size))
            else {
                break;
            };
            if end > bytes.len() || !header[4..].iter().all(u8::is_ascii_alphabetic) {
                break;
            }
            if seen.len() < 16 && seen.insert(&header[4..]) {
                let kind = std::str::from_utf8(&header[4..]).unwrap();
                rows.push((offset, format!("PNG {kind} chunk · {size} B")));
            }
            offset = end;
        }
    }
    let mut runs = Vec::new();
    let mut offset = 0;
    for run in bytes.split(|b| !(b' '..=b'~').contains(b)) {
        if run.len() >= 8 {
            runs.push((offset, run.len()));
        }
        offset += run.len() + 1;
    }
    runs.sort_unstable_by_key(|&(offset, size)| (std::cmp::Reverse(size), offset));
    rows.extend(
        runs.into_iter()
            .take(8)
            .map(|(offset, len)| (offset, format!("Printable run · {len} B"))),
    );
    rows.sort_by_key(|row| row.0);
    rows.dedup_by_key(|row| row.0);
    rows
}

pub(crate) fn hexdump_candidates(right: &str, current: &str, fish: bool) -> Rows {
    // Treat the file to the right as one literal operand. Never expand a glob,
    // substitution, or command typed after the current command.
    let Some(words) = crate::cursor::literal_words(right, fish) else {
        return vec![];
    };
    let path = match words.as_slice() {
        [path] if !path.starts_with('-') => path,
        [end, path] if end == "--" => path,
        _ => return vec![],
    };
    let Ok(bytes) = read_file(Path::new(path)) else {
        return vec![];
    };
    points(&bytes)
        .into_iter()
        .filter_map(|(offset, desc)| {
            let value = if current.starts_with("0x") {
                format!("{offset:#x}")
            } else {
                offset.to_string()
            };
            value.starts_with(current).then_some((value, desc))
        })
        .collect()
}

pub(crate) fn run(args: &[String]) -> i32 {
    let [path] = args else {
        eprintln!("usage: complete-me bytes FILE");
        return 2;
    };
    let bytes = match read_file(Path::new(path)) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("complete-me: could not read a regular file: {error}");
            return 1;
        }
    };
    for (offset, desc) in points(&bytes) {
        println!("{offset:#x}\t{offset}\t{desc}");
    }
    0
}
