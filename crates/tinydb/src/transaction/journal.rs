use std::collections::HashSet;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::storage::PAGE_SIZE;

const MAGIC: &[u8; 8] = b"TDBWAL1\0";
const COMMIT: &[u8; 8] = b"TDBCMT1\0";

pub struct Journal {
    directory: PathBuf,
    file: File,
    _lock: File,
}

impl Journal {
    pub fn open(directory: impl AsRef<Path>) -> io::Result<Self> {
        let directory = std::path::absolute(directory)?;
        let lock_path = directory.join(".lock");
        regular_or_missing(&lock_path)?;
        let lock = control_options().open(lock_path)?;
        lock.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => io::Error::new(
                ErrorKind::WouldBlock,
                "database is already open by another owner",
            ),
            TryLockError::Error(error) => error,
        })?;
        let wal_path = directory.join("wal.log");
        regular_or_missing(&wal_path)?;
        let file = control_options().open(wal_path)?;
        lock.sync_all()?;
        file.sync_all()?;
        File::open(&directory)?.sync_all()?;
        Ok(Self {
            directory,
            file,
            _lock: lock,
        })
    }

    pub fn write_transaction(&mut self, staged: impl AsRef<Path>) -> io::Result<()> {
        if self.file.metadata()?.len() != 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "recover the journal before writing another transaction",
            ));
        }
        let files = managed_files(staged.as_ref())?;
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&(files.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&checksum(&bytes).to_le_bytes());
        for (name, path) in files {
            let image = fs::read(path)?;
            if image.len() < PAGE_SIZE || image.len() % PAGE_SIZE != 0 {
                return Err(invalid_data("WAL images must contain whole database pages"));
            }
            let start = bytes.len();
            bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
            bytes.extend_from_slice(&(image.len() as u64).to_le_bytes());
            let mut payload = name.into_bytes();
            payload.extend_from_slice(&image);
            bytes.extend_from_slice(&checksum(&payload).to_le_bytes());
            bytes.extend_from_slice(&checksum(&bytes[start..]).to_le_bytes());
            bytes.extend_from_slice(&payload);
        }
        let mut commit = COMMIT.to_vec();
        commit.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        commit.extend_from_slice(&checksum(&bytes).to_le_bytes());
        commit.extend_from_slice(&checksum(&commit).to_le_bytes());
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&bytes)?;
        self.file.sync_all()?;
        self.file.write_all(&commit)?;
        self.file.sync_all()
    }

    pub fn recover(&mut self) -> io::Result<bool> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        self.file.read_to_end(&mut bytes)?;
        let images = 'decode: {
            if bytes.is_empty() {
                break 'decode None;
            }
            if bytes.len() < 20 {
                if !MAGIC.starts_with(&bytes[..bytes.len().min(8)]) {
                    return Err(invalid_data("invalid WAL magic"));
                }
                break 'decode None;
            }
            if &bytes[..8] != MAGIC
                || checksum(&bytes[..16]) != u32::from_le_bytes(bytes[16..20].try_into().unwrap())
            {
                return Err(invalid_data("invalid WAL header or checksum"));
            }
            let count = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
            if count == 0 {
                return Err(invalid_data("WAL transaction has no file images"));
            }
            let mut images = Vec::new();
            let mut names = HashSet::new();
            let mut position = 20;
            for _ in 0..count {
                if bytes.len() - position < 18 {
                    break 'decode None;
                }
                let header = &bytes[position..position + 18];
                if checksum(&header[..14]) != u32::from_le_bytes(header[14..18].try_into().unwrap())
                {
                    return Err(invalid_data("invalid WAL image header checksum"));
                }
                let name_len = usize::from(u16::from_le_bytes(header[..2].try_into().unwrap()));
                let image_len =
                    usize::try_from(u64::from_le_bytes(header[2..10].try_into().unwrap()))
                        .map_err(|_| invalid_data("WAL image length exceeds address range"))?;
                if name_len == 0 || image_len < PAGE_SIZE || image_len % PAGE_SIZE != 0 {
                    return Err(invalid_data("invalid WAL image lengths"));
                }
                let payload_len = name_len
                    .checked_add(image_len)
                    .ok_or_else(|| invalid_data("WAL payload length overflow"))?;
                position += 18;
                if payload_len > bytes.len() - position {
                    break 'decode None;
                }
                let payload = &bytes[position..position + payload_len];
                if checksum(payload) != u32::from_le_bytes(header[10..14].try_into().unwrap()) {
                    return Err(invalid_data("invalid WAL image checksum"));
                }
                let name = std::str::from_utf8(&payload[..name_len])
                    .map_err(|_| invalid_data("invalid WAL filename encoding"))?;
                if !is_managed(name) || !names.insert(name) {
                    return Err(invalid_data("invalid or duplicate WAL filename"));
                }
                images.push((name, &payload[name_len..]));
                position += payload_len;
            }
            if !names.contains("catalog.db") {
                return Err(invalid_data("WAL transaction is missing the catalog"));
            }
            let mut commit = COMMIT.to_vec();
            commit.extend_from_slice(&(position as u64).to_le_bytes());
            commit.extend_from_slice(&checksum(&bytes[..position]).to_le_bytes());
            commit.extend_from_slice(&checksum(&commit).to_le_bytes());
            let remaining = &bytes[position..];
            if remaining.len() < commit.len() {
                if !commit.starts_with(remaining) {
                    return Err(invalid_data("invalid partial WAL commit record"));
                }
                break 'decode None;
            }
            if remaining != commit {
                return Err(invalid_data("invalid WAL commit record or trailing bytes"));
            }
            break 'decode Some(images);
        };
        let committed = images.is_some();
        if let Some(images) = images {
            let temporary = self.directory.join(".checkpoint.tmp");
            regular_or_missing(&temporary)?;
            for (name, image) in images {
                let target = self.directory.join(name);
                regular_or_missing(&target)?;
                let mut file = control_options().truncate(true).open(&temporary)?;
                file.write_all(image)?;
                file.sync_all()?;
                drop(file);
                fs::rename(&temporary, target)?;
            }
            File::open(&self.directory)?.sync_all()?;
        }
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.sync_all()?;
        Ok(committed)
    }
}

pub(crate) fn managed_files(directory: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if is_managed(&name) {
            if !entry.file_type()?.is_file() {
                return Err(invalid_data("managed database paths must be regular files"));
            }
            files.push((name, entry.path()));
        }
    }
    files.sort_by(|(left, _), (right, _)| left.cmp(right));
    if !files.iter().any(|(name, _)| name == "catalog.db") {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            "database catalog is missing",
        ));
    }
    Ok(files)
}

fn is_managed(name: &str) -> bool {
    if name == "catalog.db" {
        return true;
    }
    let Some(id) = name
        .strip_prefix("table-")
        .or_else(|| name.strip_prefix("index-"))
        .and_then(|name| name.strip_suffix(".db"))
    else {
        return false;
    };
    id.parse::<i64>()
        .is_ok_and(|value| value > 0 && value.to_string() == id)
}

pub(crate) fn regular_or_missing(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(invalid_data(
            "database control and image paths must be regular files",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn checksum(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn control_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}
