//! FM27 training schedules as people share them online: the `.fmf` that FM's schedule screen
//! exports.
//!
//! The file is an SI archive (see docs/TECHNICAL.md, "FM data files"): `02 01 "afe."` (records
//! AES-128-CTR-encrypted with the key in the file) or `02 01 "fmf."` (plain), each record a zstd
//! frame, with a zstd manifest at the end. A schedule archive holds three files:
//! `<name>.tsc` (the schedule), `_data/details.aom` (name, author) and `image.img`.
//!
//! `.tsc`, measured on five shared FM27 schedules (November 2026):
//! `03 01 "cst."`, then 7 bytes of header, then 7 days of
//! `[u8 4, u16 session 1, u16 session 2, u16 extra, u8 0]` (FM27 session codes, as in the calendar
//! in memory), then a u32-length description and 8 zero bytes. Day 1 is taken as Monday. Shared
//! schedules contain no match days: FM puts the match in itself, and the app never touches match
//! or travel days anyway.

use aes::cipher::{KeyIvInit, StreamCipher};
use anyhow::{Context, Result, anyhow, bail, ensure};
use std::io::Read;
use std::path::Path;

use crate::custom::CustomSchedule;

type Aes128Ctr = ctr::Ctr128BE<aes::Aes128>;

/// A schedule read from a file.
#[derive(Clone, Debug)]
pub struct Imported {
    /// With the author's description (empty when it is FM's placeholder).
    pub schedule: CustomSchedule,
    /// Session codes the app does not know; those slots are left to FM.
    pub unknown: Vec<u16>,
}

pub fn import_file(path: &Path) -> Result<Imported> {
    let data = std::fs::read(path).with_context(|| format!("{}", path.display()))?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    import_bytes(&data, &stem)
}

pub fn import_bytes(data: &[u8], fallback_name: &str) -> Result<Imported> {
    if data.get(2..6) == Some(b"cst.") {
        return parse_tsc(data, fallback_name);
    }
    let (root, files) = unpack(data)?;
    let (name, tsc) = files
        .iter()
        .find(|(n, _)| n.to_ascii_lowercase().ends_with(".tsc"))
        .ok_or_else(|| {
            let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
            anyhow!(
                "{}: {}",
                crate::t!("dit .fmf-bestand bevat geen trainingsschema (.tsc)", "this .fmf file holds no training schedule (.tsc)"),
                names.join(", ")
            )
        })?;
    let title = if !root.trim().is_empty() { root.trim().to_string() } else { name.trim_end_matches(".tsc").to_string() };
    parse_tsc(tsc, &title)
}

/// All files in an SI archive: (path, contents), plus the archive's root name.
pub fn unpack(d: &[u8]) -> Result<(String, Vec<(String, Vec<u8>)>)> {
    let magic = d.get(2..6).context("file too short")?;
    let encrypted = match magic {
        b"afe." => true,
        b"fmf." => false,
        _ => bail!(crate::t!("geen FM-bestand (.fmf)", "not an FM file (.fmf)")),
    };
    let footer = u64_at(d, 9)? as usize;
    ensure!(d.get(footer + 11..footer + 15) == Some(b"fmf."), "fmf: manifest not found");
    let manifest = zstd(d.get(footer + 18..).context("fmf: manifest")?)?;
    let mut r = Cursor { b: &manifest, p: 0 };
    let root = r.string()?;
    let mut entries = Vec::new();
    read_dir(&mut r, "", &mut entries, 0)?;
    let mut out = Vec::new();
    for (name, off, size) in entries {
        let rec = d.get(26 + off..26 + off + size).with_context(|| format!("fmf: {name} outside the file"))?;
        let z = if encrypted {
            let kl = u32_at(rec, 0)? as usize;
            let il = u32_at(rec, 4)? as usize;
            ensure!(kl == 16 && il == 16, "fmf: {name}: key {kl}, iv {il}");
            let (key, iv, ct) = (&rec[8..24], &rec[24..40], &rec[40..]);
            let mut buf = ct.to_vec();
            Aes128Ctr::new(key.into(), iv.into()).apply_keystream(&mut buf);
            buf
        } else {
            rec.to_vec()
        };
        out.push((name, zstd(&z)?));
    }
    Ok((root, out))
}

fn read_dir(r: &mut Cursor, path: &str, out: &mut Vec<(String, usize, usize)>, depth: u32) -> Result<()> {
    ensure!(depth < 16, "fmf: manifest too deep");
    let files = r.u32()?;
    ensure!(files < 10_000, "fmf: {files} files");
    for _ in 0..files {
        let name = r.string()?;
        let ext = r.string()?;
        let off = r.u64()? as usize;
        let size = r.u64()? as usize;
        let _raw = r.u64()?;
        r.skip(16)?; // two timestamps
        out.push((format!("{path}{name}{ext}"), off, size));
    }
    let dirs = r.u32()?;
    ensure!(dirs < 10_000, "fmf: {dirs} dirs");
    for _ in 0..dirs {
        let name = r.string()?;
        read_dir(r, &format!("{path}{name}/"), out, depth + 1)?;
    }
    Ok(())
}

/// One day: [u8 4, u16 × 3, u8 0].
const DAY: usize = 8;

fn parse_tsc(t: &[u8], name: &str) -> Result<Imported> {
    ensure!(t.get(2..6) == Some(b"cst."), "tsc: unknown header");
    // the days start at byte 13 in every file seen so far; look a bit around it for other versions
    let start = std::iter::once(13)
        .chain(6..64)
        .find(|o| days_at(t, *o).is_some())
        .context(crate::t!("tsc: de 7 dagen van het schema niet gevonden", "tsc: could not find the schedule's 7 days"))?;
    let days = days_at(t, start).unwrap();
    let mut s = CustomSchedule::new(name);
    // shared schedules are made to be used as they are: no automatic days around matches
    s.follow_match_rhythm = false;
    let mut unknown = Vec::new();
    for (d, day) in days.iter().enumerate() {
        for (i, c) in day.iter().enumerate() {
            if crate::custom::selectable(*c) {
                s.days[d][i] = Some(*c);
            } else if !unknown.contains(c) {
                unknown.push(*c);
            }
        }
    }
    let mut description = String::new();
    let p = start + 7 * DAY;
    if let Ok(n) = u32_at(t, p) {
        if let Some(b) = t.get(p + 4..p + 4 + n as usize) {
            description = String::from_utf8_lossy(b).trim().to_string();
        }
    }
    if description == "Schedule description..." {
        description.clear();
    }
    s.description = description;
    Ok(Imported { schedule: s, unknown })
}

/// The 7 days at offset `o`, if the bytes there look like them: every day framed by 4 … 0 and at
/// least 2/3 of the 21 slots a session code the app knows.
fn days_at(t: &[u8], o: usize) -> Option<[[u16; 3]; 7]> {
    let mut days = [[0u16; 3]; 7];
    let mut known = 0;
    for (d, day) in days.iter_mut().enumerate() {
        let b = t.get(o + d * DAY..o + (d + 1) * DAY)?;
        if b[0] != 4 || b[7] != 0 {
            return None;
        }
        for (i, slot) in day.iter_mut().enumerate() {
            *slot = u16::from_le_bytes([b[1 + 2 * i], b[2 + 2 * i]]);
            if crate::model::session_by_code(*slot).is_some() {
                known += 1;
            }
        }
    }
    (known >= 14).then_some(days)
}

fn zstd(b: &[u8]) -> Result<Vec<u8>> {
    let mut src = b;
    let mut dec = ruzstd::decoding::StreamingDecoder::new(&mut src).map_err(|e| anyhow!("zstd: {e}"))?;
    let mut out = Vec::new();
    dec.read_to_end(&mut out).context("zstd")?;
    Ok(out)
}

fn u32_at(b: &[u8], p: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(b.get(p..p + 4).context("unexpected end of file")?.try_into()?))
}

fn u64_at(b: &[u8], p: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(b.get(p..p + 8).context("unexpected end of file")?.try_into()?))
}

struct Cursor<'a> {
    b: &'a [u8],
    p: usize,
}

impl Cursor<'_> {
    fn u32(&mut self) -> Result<u32> {
        let v = u32_at(self.b, self.p)?;
        self.p += 4;
        Ok(v)
    }
    fn u64(&mut self) -> Result<u64> {
        let v = u64_at(self.b, self.p)?;
        self.p += 8;
        Ok(v)
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        ensure!(self.p + n <= self.b.len(), "fmf: manifest ends early");
        self.p += n;
        Ok(())
    }
    fn string(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        let s = self.b.get(self.p..self.p + n).context("fmf: manifest ends early")?;
        self.p += n;
        Ok(String::from_utf8_lossy(s).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `.tsc` of a shared schedule ("C1 min+24TE"), byte for byte.
    const C1: [u8; 105] = [
        3, 1, 99, 115, 116, 46, 3, 0, 5, 1, 0, 0, 0, 1, 4, 47, 1, 201, 0, 202, 0, 0, 4, 0, 0, 4, 0, 1, 0, 0, 4, 44, 1, 44, 1, 44, 1, 0, 4, 44, 1, 44, 1, 44, 1, 0, 4, 5, 0, 2, 0, 6, 0, 0, 4, 44, 1, 44, 1, 44, 1,
        0, 4, 44, 1, 44, 1, 44, 1, 0, 23, 0, 0, 0, 83, 99, 104, 101, 100, 117, 108, 101, 32, 100, 101, 115, 99, 114, 105, 112, 116, 105, 111, 110, 46, 46, 46, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    fn check_c1(s: &CustomSchedule) {
        let rest = [Some(300); 3];
        assert_eq!(s.days[0], [Some(303), Some(201), Some(202)]);
        assert_eq!(s.days[1], [Some(0), Some(4), Some(1)]);
        assert_eq!(s.days[2], rest);
        assert_eq!(s.days[3], rest);
        assert_eq!(s.days[4], [Some(5), Some(2), Some(6)]);
        assert_eq!(s.days[5], rest);
        assert_eq!(s.days[6], rest);
        assert!(!s.follow_match_rhythm);
    }

    #[test]
    fn reads_tsc() {
        let r = import_bytes(&C1, "C1").unwrap();
        assert_eq!(r.schedule.name, "C1");
        assert!(r.schedule.description.is_empty(), "FM's placeholder is dropped");
        assert!(r.unknown.is_empty());
        check_c1(&r.schedule);
    }

    /// Build an archive the way FM does (records encrypted or not, zstd manifest at the end) and
    /// read it back.
    fn archive(encrypted: bool, root: &str, files: &[(&str, &str, &[u8])]) -> Vec<u8> {
        use ruzstd::encoding::{CompressionLevel, compress_to_vec};
        let mut body = Vec::new();
        let mut entries = Vec::new();
        for (i, (name, ext, data)) in files.iter().enumerate() {
            let z = compress_to_vec(*data, CompressionLevel::Fastest);
            let rec = if encrypted {
                let (key, iv) = ([i as u8 + 7; 16], [0xA5u8; 16]);
                let mut ct = z.clone();
                Aes128Ctr::new(&key.into(), &iv.into()).apply_keystream(&mut ct);
                let mut r = Vec::new();
                r.extend(16u32.to_le_bytes());
                r.extend(16u32.to_le_bytes());
                r.extend(key);
                r.extend(iv);
                r.extend(ct);
                r
            } else {
                z
            };
            entries.push((name.to_string(), ext.to_string(), body.len() as u64, rec.len() as u64, data.len() as u64));
            body.extend(rec);
        }
        let s = |m: &mut Vec<u8>, x: &str| {
            m.extend((x.len() as u32).to_le_bytes());
            m.extend(x.as_bytes());
        };
        let mut m = Vec::new();
        s(&mut m, root);
        m.extend((entries.len() as u32).to_le_bytes());
        for (n, e, off, size, raw) in &entries {
            s(&mut m, n);
            s(&mut m, e);
            for v in [*off, *size, *raw, 0, 0] {
                m.extend(v.to_le_bytes());
            }
        }
        m.extend(0u32.to_le_bytes());
        let mut d = vec![2, 1];
        d.extend(if encrypted { b"afe." } else { b"fmf." });
        d.resize(26, 0);
        d.extend(body);
        let footer = d.len() - 9; // the trailer's "fmf." sits at footer + 11
        d[9..17].copy_from_slice(&(footer as u64).to_le_bytes());
        d.extend([2, 1]);
        d.extend(b"fmf.");
        d.extend([8, 0, 0]);
        d.extend(compress_to_vec(&m[..], CompressionLevel::Fastest));
        d
    }

    #[test]
    fn reads_archives() {
        for enc in [true, false] {
            let d = archive(enc, "C1 min+24TE", &[("image", ".img", &[1, 0, 0, 0]), ("C1 min+24TE", ".tsc", &C1)]);
            let r = import_bytes(&d, "file").unwrap();
            assert_eq!(r.schedule.name, "C1 min+24TE");
            check_c1(&r.schedule);
        }
        let d = archive(true, "x", &[("image", ".img", &[1])]);
        assert!(import_bytes(&d, "x").is_err(), "no schedule in it");
        assert!(import_bytes(b"hello world, not fm", "x").is_err());
    }

    /// Real shared files: `FM27_FMF_SAMPLES=<folder> cargo test -p fm27-trainer fmf`.
    #[test]
    fn reads_sample_folder() {
        let Some(dir) = std::env::var_os("FM27_FMF_SAMPLES") else { return };
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "fmf") {
                let r = import_file(&p).unwrap_or_else(|e| panic!("{}: {e:#}", p.display()));
                println!("{}: {} {:?} unknown {:?}", p.display(), r.schedule.name, r.schedule.days, r.unknown);
                assert!(r.unknown.is_empty());
                n += 1;
            }
        }
        assert!(n > 0);
    }
}
