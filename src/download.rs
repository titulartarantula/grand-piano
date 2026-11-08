//! First-run download of the Salamander Grand Piano samples, verified by SHA-256.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

pub const SAMPLE_URL: &str =
    "https://freepats.zenvoid.org/Piano/SalamanderGrandPiano/SalamanderGrandPiano-SFZ+FLAC-V3+20200602.tar.gz";
pub const SAMPLE_SHA256: &str = "b7760e168494cf095344e217b0af013fc449ad033abbbdf1c65211cf11dc038b";
pub const SAMPLE_BYTES: u64 = 741_757_374;
pub const SAMPLE_FOLDER: &str = "SalamanderGrandPiano-SFZ+FLAC-V3+20200602";
pub const SFZ_FILE: &str = "SalamanderGrandPiano-V3+20200602.sfz";
const MARKER: &str = ".verified-sha256";

#[derive(Default)]
pub struct DownloadProgress {
    pub bytes: AtomicU64,
    pub total: AtomicU64,
    /// 0 = idle, 1 = downloading, 2 = verifying, 3 = extracting, 4 = done
    pub stage: AtomicU8,
}

pub fn samples_root() -> PathBuf {
    crate::settings::data_dir().join("samples")
}

/// Returns the installed SFZ path if the samples are present and were verified.
pub fn find_installed(root: &Path) -> Option<PathBuf> {
    let folder = root.join(SAMPLE_FOLDER);
    let sfz = folder.join(SFZ_FILE);
    let marker = std::fs::read_to_string(folder.join(MARKER)).ok()?;
    if marker.trim() == SAMPLE_SHA256 && sfz.is_file() { Some(sfz) } else { None }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

/// Downloads (or uses a local archive), verifies the checksum, and extracts.
pub fn ensure_samples(root: &Path, local_archive: Option<&Path>, progress: Arc<DownloadProgress>) -> anyhow::Result<PathBuf> {
    if let Some(p) = find_installed(root) {
        progress.stage.store(4, Ordering::Relaxed);
        return Ok(p);
    }
    std::fs::create_dir_all(root)?;
    let archive = match local_archive {
        Some(a) => a.to_path_buf(),
        None => {
            let part = root.join("download.part");
            download(&part, progress.clone())?;
            part
        }
    };
    progress.stage.store(2, Ordering::Relaxed);
    let got = sha256_file(&archive)?;
    if got != SAMPLE_SHA256 {
        if local_archive.is_none() {
            let _ = std::fs::remove_file(&archive);
        }
        anyhow::bail!("checksum mismatch for sample archive: expected {SAMPLE_SHA256}, got {got}");
    }
    progress.stage.store(3, Ordering::Relaxed);
    extract(&archive, root)?;
    std::fs::write(root.join(SAMPLE_FOLDER).join(MARKER), SAMPLE_SHA256)?;
    if local_archive.is_none() {
        let _ = std::fs::remove_file(&archive);
    }
    progress.stage.store(4, Ordering::Relaxed);
    find_installed(root).ok_or_else(|| anyhow::anyhow!("samples extracted but {SFZ_FILE} not found"))
}

fn download(dest: &Path, progress: Arc<DownloadProgress>) -> anyhow::Result<()> {
    progress.stage.store(1, Ordering::Relaxed);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(20)))
        .user_agent("GrandPiano/0.1 (sample download)")
        .build()
        .into();
    let resp = agent.get(SAMPLE_URL).call()?;
    let total = resp.body().content_length().unwrap_or(SAMPLE_BYTES);
    progress.total.store(total, Ordering::Relaxed);
    let mut reader = resp.into_body().into_reader();
    let mut f = std::io::BufWriter::new(std::fs::File::create(dest)?);
    let mut buf = vec![0u8; 1 << 16];
    let mut got = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n])?;
        got += n as u64;
        progress.bytes.store(got, Ordering::Relaxed);
    }
    f.flush()?;
    if got != total {
        anyhow::bail!("download incomplete: {got} of {total} bytes");
    }
    Ok(())
}

fn extract(archive: &Path, root: &Path) -> anyhow::Result<()> {
    let f = std::fs::File::open(archive)?;
    let gz = flate2::read::GzDecoder::new(std::io::BufReader::new(f));
    let mut tar = tar::Archive::new(gz);
    for entry in tar.entries()? {
        let mut entry = entry?;
        // Only regular files and directories; unpack_in refuses paths outside root.
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            continue;
        }
        entry.unpack_in(root)?;
    }
    Ok(())
}
