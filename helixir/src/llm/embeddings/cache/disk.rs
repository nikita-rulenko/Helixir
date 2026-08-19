//! Locked append and bounded atomic snapshots for the embedding cache.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::CacheNamespace;

const FORMAT_VERSION: u8 = 2;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct DiskRecord {
    pub(super) version: u8,
    pub(super) namespace: String,
    pub(super) text_hash: String,
    pub(super) dimension: usize,
    pub(super) written_at_ns: u128,
    pub(super) embedding: Vec<f32>,
}

impl DiskRecord {
    pub(super) fn new(namespace: String, text_hash: String, embedding: Vec<f32>) -> Self {
        Self {
            version: FORMAT_VERSION,
            namespace,
            text_hash,
            dimension: embedding.len(),
            written_at_ns: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            embedding,
        }
    }

    fn is_valid_for(&self, namespaces: &HashMap<String, Option<usize>>) -> bool {
        self.version == FORMAT_VERSION
            && self.dimension > 0
            && self.dimension == self.embedding.len()
            && self.text_hash.len() == 64
            && self.text_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            && namespaces.get(&self.namespace).is_some_and(|expected| {
                expected.is_none_or(|dimension| dimension == self.dimension)
            })
    }

    fn key(&self) -> String {
        format!("{}:{}", self.namespace, self.text_hash)
    }
}

pub(super) struct PersistentStore {
    path: PathBuf,
    lock_path: PathBuf,
    namespaces: HashMap<String, Option<usize>>,
    max_entries: usize,
    max_bytes: u64,
    appends: AtomicUsize,
    compactions: AtomicU64,
}

impl PersistentStore {
    pub(super) fn open(
        path: &Path,
        namespaces: &[CacheNamespace],
        max_entries: usize,
        max_bytes: u64,
    ) -> io::Result<(Self, Vec<DiskRecord>, u64)> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let store = Self {
            path: path.to_path_buf(),
            lock_path: path.with_extension("lock"),
            namespaces: namespaces
                .iter()
                .map(|namespace| (namespace.id.clone(), namespace.expected_dimension))
                .collect(),
            max_entries: max_entries.max(1),
            max_bytes: max_bytes.max(1),
            appends: AtomicUsize::new(0),
            compactions: AtomicU64::new(0),
        };
        let lock = store.lock_exclusive()?;
        let (records, load) = store.load_bounded()?;
        if load.dirty || store.file_len()? > store.max_bytes {
            store.write_snapshot(&records)?;
            store.compactions.fetch_add(1, Ordering::Relaxed);
        } else {
            drop(private_append(&store.path)?);
        }
        FileExt::unlock(&lock)?;
        Ok((store, records, load.invalidated))
    }

    /// Append a record and report whether it fit the durable cache budget.
    pub(super) fn append_and_compact(&self, record: &DiskRecord) -> io::Result<bool> {
        let lock = self.lock_exclusive()?;
        if serialized_size(record)? as u64 > self.max_bytes {
            let (records, _) = self.load_bounded()?;
            self.write_snapshot(&records)?;
            self.appends.store(0, Ordering::Relaxed);
            self.compactions.fetch_add(1, Ordering::Relaxed);
            FileExt::unlock(&lock)?;
            return Ok(false);
        }
        let mut file = private_append(&self.path)?;
        serde_json::to_writer(&mut file, record)?;
        file.write_all(b"\n")?;
        file.flush()?;

        let appends = self.appends.fetch_add(1, Ordering::Relaxed) + 1;
        if self.file_len()? > self.max_bytes || appends >= self.max_entries {
            let (records, _) = self.load_bounded()?;
            self.write_snapshot(&records)?;
            self.appends.store(0, Ordering::Relaxed);
            self.compactions.fetch_add(1, Ordering::Relaxed);
        }
        FileExt::unlock(&lock)?;
        Ok(true)
    }

    pub(super) fn clear(&self) -> io::Result<()> {
        let lock = self.lock_exclusive()?;
        self.write_snapshot(&[])?;
        self.appends.store(0, Ordering::Relaxed);
        self.compactions.fetch_add(1, Ordering::Relaxed);
        FileExt::unlock(&lock)?;
        Ok(())
    }

    fn lock_exclusive(&self) -> io::Result<File> {
        let lock = private_read_write(&self.lock_path)?;
        lock.lock_exclusive()?;
        Ok(lock)
    }

    fn load_bounded(&self) -> io::Result<(Vec<DiskRecord>, LoadReport)> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok((Vec::new(), LoadReport::default()));
            }
            Err(error) => return Err(error),
        };
        let mut current = HashMap::<String, (u64, usize, DiskRecord)>::new();
        let mut order = BinaryHeap::<Reverse<(u64, String)>>::new();
        let mut sequence = 0u64;
        let mut report = LoadReport::default();
        let mut retained_bytes = 0usize;

        for line in BufReader::new(file).lines() {
            sequence = sequence.saturating_add(1);
            let Ok(line) = line else {
                report.reject();
                continue;
            };
            let Ok(record) = serde_json::from_str::<DiskRecord>(&line) else {
                report.reject();
                continue;
            };
            if !record.is_valid_for(&self.namespaces) {
                report.reject();
                continue;
            }
            let record_bytes = serialized_size(&record)?;
            let key = record.key();
            if let Some((_, previous_bytes, _)) =
                current.insert(key.clone(), (sequence, record_bytes, record))
            {
                retained_bytes = retained_bytes.saturating_sub(previous_bytes);
                report.reject();
            }
            retained_bytes = retained_bytes.saturating_add(record_bytes);
            order.push(Reverse((sequence, key)));
            while current.len() > self.max_entries || retained_bytes as u64 > self.max_bytes {
                let Some(Reverse((candidate_sequence, candidate_key))) = order.pop() else {
                    break;
                };
                if current
                    .get(&candidate_key)
                    .is_some_and(|(latest_sequence, _, _)| *latest_sequence == candidate_sequence)
                    && let Some((_, removed_bytes, _)) = current.remove(&candidate_key)
                {
                    retained_bytes = retained_bytes.saturating_sub(removed_bytes);
                    report.reject();
                }
            }
            if order.len() > self.max_entries.saturating_mul(4) {
                order = current
                    .iter()
                    .map(|(key, (position, _, _))| Reverse((*position, key.clone())))
                    .collect();
            }
        }

        let mut records: Vec<_> = current.into_values().collect();
        records.sort_by_key(|(position, _, _)| *position);
        Ok((
            records.into_iter().map(|(_, _, record)| record).collect(),
            report,
        ))
    }

    fn write_snapshot(&self, records: &[DiskRecord]) -> io::Result<()> {
        let temporary = self.path.with_extension(format!(
            "tmp.{}.{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let mut writer = BufWriter::new(private_truncate(&temporary)?);
        let mut seen = HashSet::new();
        for record in records {
            if seen.insert(record.key()) {
                serde_json::to_writer(&mut writer, record)?;
                writer.write_all(b"\n")?;
            }
        }
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        replace_file(&temporary, &self.path)?;
        ensure_private(&self.path)?;
        sync_parent(&self.path)
    }

    fn file_len(&self) -> io::Result<u64> {
        match fs::metadata(&self.path) {
            Ok(metadata) => Ok(metadata.len()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
            Err(error) => Err(error),
        }
    }

    pub(super) fn bytes(&self) -> io::Result<u64> {
        self.file_len()
    }

    pub(super) fn compactions(&self) -> u64 {
        self.compactions.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
struct LoadReport {
    dirty: bool,
    invalidated: u64,
}

impl LoadReport {
    fn reject(&mut self) {
        self.dirty = true;
        self.invalidated = self.invalidated.saturating_add(1);
    }
}

fn serialized_size(record: &DiskRecord) -> io::Result<usize> {
    serde_json::to_vec(record)
        .map(|bytes| bytes.len().saturating_add(1))
        .map_err(io::Error::other)
}

fn private_append(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    ensure_private(path)?;
    Ok(file)
}

fn private_read_write(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    ensure_private(path)?;
    Ok(file)
}

fn private_truncate(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    ensure_private(path)?;
    Ok(file)
}

#[cfg(unix)]
fn ensure_private(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn ensure_private(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both buffers are NUL-terminated and remain alive for the call.
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
