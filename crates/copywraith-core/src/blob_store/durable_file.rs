use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    DirectoryCreated,
    ParentSynced,
    TempCreated,
    TempWritten,
    FileSynced,
    Published,
    DirectorySynced,
}

fn boundary(point: Boundary) -> io::Result<()> {
    #[cfg(feature = "blob-fault-injection")]
    super::fault_injection::check(point)?;
    let _ = point;
    Ok(())
}

pub(super) fn prepare_directory(path: &Path) -> io::Result<()> {
    let path = std::path::absolute(path)?;
    // Walk ancestors on retry too: an existing directory may have been created
    // just before a failed parent sync. Existence alone proves no durability.
    if let Some(parent) = path.parent() {
        prepare_directory(parent)?;
        match std::fs::create_dir(&path) {
            Ok(()) => boundary(Boundary::DirectoryCreated)?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => {}
            Err(error) => return Err(error),
        }
        sync_directory(parent)?;
        boundary(Boundary::ParentSynced)?;
    }
    Ok(())
}

pub(super) fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Blob path lacks a parent"))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".blob-")
        .tempfile_in(parent)?;
    boundary(Boundary::TempCreated)?;
    temporary.write_all(bytes)?;
    boundary(Boundary::TempWritten)?;
    temporary.as_file().sync_all()?;
    boundary(Boundary::FileSynced)?;

    // Only a complete synchronized replacement can displace the previous file.
    publish(temporary.path(), path)?;
    boundary(Boundary::Published)?;
    sync_directory(parent)?;
    boundary(Boundary::DirectorySynced)
}

pub(super) fn synchronize_existing(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()?;
    boundary(Boundary::FileSynced)?;
    sync_directory(
        path.parent()
            .ok_or_else(|| io::Error::other("Blob path lacks a parent"))?,
    )?;
    boundary(Boundary::DirectorySynced)
}

#[cfg(unix)]
fn publish(source: &Path, destination: &Path) -> io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(unix)]
pub(super) fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

// No unprivileged, documented Windows directory-flush guarantee was established.
// Fail closed rather than acknowledge durable blobs using Unix assumptions.
#[cfg(not(unix))]
fn publish(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(unsupported_durability())
}

#[cfg(not(unix))]
pub(super) fn sync_directory(_path: &Path) -> io::Result<()> {
    Err(unsupported_durability())
}

#[cfg(not(unix))]
fn unsupported_durability() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "Durable blob directory publication is not implemented on this platform",
    )
}
