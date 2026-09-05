//! Content-addressed publication shared by the client and encrypted server store.
//! Callers hold their storage transaction lock until publication and DB commit.
use std::io;
use std::path::{Path, PathBuf};

#[path = "blob_store/durable_file.rs"]
mod durable_file;

#[cfg(feature = "blob-fault-injection")]
#[path = "blob_store/fault_injection.rs"]
pub mod fault_injection;

pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn open(root: &Path) -> io::Result<Self> {
        durable_file::prepare_directory(root)?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Accept only validated bytes; even reused files need a durability barrier
    /// because they may be orphans from a previously interrupted publication.
    pub fn ensure(
        &self,
        hash: &str,
        bytes: &[u8],
        validate: impl Fn(&[u8]) -> bool,
    ) -> io::Result<()> {
        let path = self.path(hash)?;
        if !validate(bytes) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Blob bytes do not match their identity",
            ));
        }
        if self.read(hash)?.is_some_and(|existing| validate(&existing)) {
            return durable_file::synchronize_existing(&path);
        }
        durable_file::replace(&path, bytes)
    }

    pub fn read(&self, hash: &str) -> io::Result<Option<Vec<u8>>> {
        match std::fs::read(self.path(hash)?) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Reclamation follows the row commit; failures may leave harmless orphans.
    pub fn remove(&self, hash: &str) -> io::Result<()> {
        let path = self.path(hash)?;
        match std::fs::remove_file(path) {
            Ok(()) => durable_file::sync_directory(&self.root),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub fn hashes(&self) -> io::Result<Vec<String>> {
        let mut hashes = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if crate::content::is_valid_hash(&name) && entry.file_type()?.is_file() {
                hashes.push(name);
            }
        }
        Ok(hashes)
    }

    fn path(&self, hash: &str) -> io::Result<PathBuf> {
        if !crate::content::is_valid_hash(hash) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid blob hash",
            ));
        }
        Ok(self.root.join(hash))
    }
}
