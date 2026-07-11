//! Descriptor-anchored filesystem helpers for security-sensitive writes.

#[cfg(unix)]
use cap_std::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, DirBuilder, OpenOptions},
};
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryIdentity {
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
}

/// Result of an atomic replacement. A post-rename directory-sync failure is reported as a
/// committed outcome rather than an ordinary error: once the new name is visible, callers must
/// not behave as though the old file is still authoritative.
#[derive(Debug)]
pub enum AtomicWriteOutcome {
    Durable,
    CommittedButDirectorySyncFailed(io::Error),
}

pub fn directory_identity(path: &Path) -> io::Result<DirectoryIdentity> {
    let dir = Dir::open_ambient_dir(path, ambient_authority())?;
    directory_identity_of(&dir)
}

pub fn directory_identity_of(dir: &Dir) -> io::Result<DirectoryIdentity> {
    #[cfg(unix)]
    {
        let (dev, ino) = identity(dir)?;
        Ok(DirectoryIdentity { dev, ino })
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "secure review writes are supported on Darwin and Linux",
        ))
    }
}

/// Opens an existing absolute directory and rejects the directory actually opened when it is
/// inside `forbidden_root`. The descriptor, rather than a previously canonicalized pathname, is
/// the authority returned to the caller.
pub fn open_dir_outside(path: &Path, forbidden_root: DirectoryIdentity) -> io::Result<Dir> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "secure directory path must be absolute",
        ));
    }
    let dir = Dir::open_ambient_dir(path, ambient_authority())?;
    reject_if_within(&dir, forbidden_root)?;
    Ok(dir)
}

/// Creates an absolute directory tree a component at a time from an opened ancestor. Every later
/// operation stays relative to a held directory descriptor, so renaming or replacing an ancestor
/// pathname cannot redirect the operation.
pub fn open_or_create_dir_outside(
    path: &Path,
    forbidden_root: DirectoryIdentity,
) -> io::Result<Dir> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "secure directory path must be absolute",
        ));
    }

    let (existing, missing) = nearest_existing_ancestor(path)?;
    let mut dir = Dir::open_ambient_dir(&existing, ambient_authority())?;
    // Do not create even an empty directory below the forbidden tree.
    reject_if_within(&dir, forbidden_root)?;

    for component in missing.iter().rev() {
        let mut builder = DirBuilder::new();
        #[cfg(unix)]
        builder.mode(0o700);
        if let Err(create_error) = dir.create_dir_with(component, &builder) {
            // Another process may have won the mkdir race. Accept only an actual no-follow
            // directory open; otherwise preserve the mkdir error when useful.
            match open_child_dir_nofollow_retry(&dir, component) {
                Ok(next) => {
                    dir = next;
                    reject_if_within(&dir, forbidden_root)?;
                    set_private_dir_permissions(&dir)?;
                    continue;
                }
                Err(_) => return Err(create_error),
            }
        }
        dir = open_child_dir_nofollow_retry(&dir, component)?;
        reject_if_within(&dir, forbidden_root)?;
        set_private_dir_permissions(&dir)?;
    }
    set_private_dir_permissions(&dir)?;
    Ok(dir)
}

pub fn open_child_dir_nofollow(dir: &Dir, name: &OsStr) -> io::Result<Dir> {
    validate_file_name(name)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW);
    let file = dir
        .open_with(Path::new(name), &options)
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("failed descriptor-relative open of {:?}: {error}", name),
            )
        })?
        .into_std();
    if !file.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "secure directory component is not a directory",
        ));
    }
    Ok(Dir::from_std_file(file))
}

fn open_child_dir_nofollow_retry(dir: &Dir, name: &OsStr) -> io::Result<Dir> {
    let mut last_error = None;
    for _ in 0..16 {
        match open_child_dir_nofollow(dir, name) {
            Ok(child) => return Ok(child),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::Interrupted
                ) =>
            {
                last_error = Some(error);
                std::thread::yield_now();
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.expect("retry loop records an error"))
}

/// Opens a regular file relative to `dir` without following a final symlink.
pub fn open_file_nofollow(
    dir: &Dir,
    name: &OsStr,
    read: bool,
    write: bool,
    create: bool,
) -> io::Result<fs::File> {
    open_regular_file_nofollow(dir, name, read, write, create, true)
}

pub fn open_read_file_nofollow(dir: &Dir, name: &OsStr) -> io::Result<fs::File> {
    open_regular_file_nofollow(dir, name, true, false, false, false)
}

fn open_regular_file_nofollow(
    dir: &Dir,
    name: &OsStr,
    read: bool,
    write: bool,
    create: bool,
    private: bool,
) -> io::Result<fs::File> {
    validate_file_name(name)?;
    let mut options = OpenOptions::new();
    options.read(read).write(write).create(create);
    #[cfg(unix)]
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    let mut attempts = 0;
    let file = loop {
        match dir.open_with(Path::new(name), &options) {
            Ok(file) => break file.into_std(),
            Err(error)
                if create
                    && matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::Interrupted
                    )
                    && attempts < 15 =>
            {
                // cap-std's component-safe open can transiently lose a simultaneous O_CREAT race
                // on Darwin. Retrying relative to the same held directory remains safe.
                attempts += 1;
                std::thread::yield_now();
            }
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "failed descriptor-relative file open of {:?}: {error}",
                        name
                    ),
                ));
            }
        }
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "secure file entry is not a regular file",
        ));
    }
    #[cfg(unix)]
    if private && std::os::unix::fs::MetadataExt::nlink(&metadata) != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "secure file entry must not have additional hard links",
        ));
    }
    #[cfg(unix)]
    if private {
        file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    }
    Ok(file)
}

/// Atomically replaces one file relative to a held directory descriptor.
pub fn atomic_write(dir: &Dir, name: &OsStr, bytes: &[u8]) -> io::Result<AtomicWriteOutcome> {
    atomic_write_with_sync(dir, name, bytes, sync_directory)
}

fn atomic_write_with_sync(
    dir: &Dir,
    name: &OsStr,
    bytes: &[u8],
    mut sync_dir: impl FnMut(&Dir) -> io::Result<()>,
) -> io::Result<AtomicWriteOutcome> {
    validate_file_name(name)?;
    let temporary = unique_temporary_name(name)?;
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let mut file = dir.open_with(Path::new(&temporary), &options)?.into_std();
        // O_CREAT's mode is still filtered by umask. Set the intended private mode explicitly so
        // the invariant does not depend on process configuration.
        #[cfg(unix)]
        file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // Make the temporary directory entry durable before it becomes the replacement. A failure
        // here is still a true pre-commit error and the temporary can be removed safely.
        sync_dir(dir)?;
        dir.rename(Path::new(&temporary), dir, Path::new(name))?;
        // The rename is the visibility boundary. Never turn a failure after this point into an
        // ordinary Err, because callers could then skip a paired publication even though these
        // bytes are already visible.
        Ok(match sync_dir(dir) {
            Ok(()) => AtomicWriteOutcome::Durable,
            Err(error) => AtomicWriteOutcome::CommittedButDirectorySyncFailed(error),
        })
    })();
    if result.is_err() {
        let _ = dir.remove_file(Path::new(&temporary));
    }
    result
}

fn sync_directory(dir: &Dir) -> io::Result<()> {
    dir.try_clone()?.into_std_file().sync_all()
}

fn nearest_existing_ancestor(path: &Path) -> io::Result<(PathBuf, Vec<OsString>)> {
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(existing) {
            Ok(_) => return Ok((existing.to_path_buf(), missing)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = existing.file_name().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("{} has no existing ancestor", path.display()),
                    )
                })?;
                missing.push(name.to_owned());
                existing = existing.parent().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("{} has no existing ancestor", path.display()),
                    )
                })?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn reject_if_within(dir: &Dir, forbidden_root: DirectoryIdentity) -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut current = dir.try_clone()?;
        loop {
            let current_identity = identity(&current)?;
            if current_identity == (forbidden_root.dev, forbidden_root.ino) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "opened directory is inside the forbidden tree",
                ));
            }
            let parent = current.open_parent_dir(ambient_authority())?;
            if identity(&parent)? == current_identity {
                break;
            }
            current = parent;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = forbidden_root;
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "secure review writes are supported on Darwin and Linux",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn identity(dir: &Dir) -> io::Result<(u64, u64)> {
    let metadata = dir.dir_metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}

fn set_private_dir_permissions(dir: &Dir) -> io::Result<()> {
    #[cfg(unix)]
    dir.set_permissions(".", cap_std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn validate_file_name(name: &OsStr) -> io::Result<()> {
    let path = Path::new(name);
    if name.is_empty() || path.file_name() != Some(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "secure file name must be one normal path component",
        ));
    }
    Ok(())
}

fn unique_temporary_name(name: &OsStr) -> io::Result<OsString> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    let suffix = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(format!(".{suffix}.tmp"));
    Ok(temporary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn held_directory_cannot_be_redirected_by_ancestor_swap() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let deck = root.path().join("deck");
        let output_parent = root.path().join("output");
        fs::create_dir(&deck).unwrap();
        fs::create_dir(&output_parent).unwrap();
        let dir = open_dir_outside(&output_parent, directory_identity(&deck).unwrap()).unwrap();

        let original = root.path().join("output-original");
        fs::rename(&output_parent, &original).unwrap();
        symlink(&deck, &output_parent).unwrap();
        atomic_write(&dir, OsStr::new("handoff.json"), b"safe\n").unwrap();

        assert_eq!(fs::read(original.join("handoff.json")).unwrap(), b"safe\n");
        assert!(!deck.join("handoff.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn forbidden_identity_survives_deck_rename_and_decoy_replacement() {
        let root = tempfile::tempdir().unwrap();
        let deck = root.path().join("deck");
        fs::create_dir(&deck).unwrap();
        fs::create_dir(deck.join("state")).unwrap();
        let identity = directory_identity(&deck).unwrap();
        let moved = root.path().join("deck-moved");
        fs::rename(&deck, &moved).unwrap();
        fs::create_dir(&deck).unwrap();

        let error = open_dir_outside(&moved.join("state"), identity).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    #[test]
    fn post_rename_sync_failure_is_a_committed_outcome() {
        let root = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
        let mut syncs = 0;

        let outcome = atomic_write_with_sync(&dir, OsStr::new("artifact.json"), b"new\n", |_| {
            syncs += 1;
            if syncs == 2 {
                Err(io::Error::other("injected post-rename sync failure"))
            } else {
                Ok(())
            }
        })
        .unwrap();

        assert!(matches!(
            outcome,
            AtomicWriteOutcome::CommittedButDirectorySyncFailed(_)
        ));
        assert_eq!(
            fs::read(root.path().join("artifact.json")).unwrap(),
            b"new\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn pre_rename_sync_failure_is_uncommitted_and_cleans_up() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("artifact.json"), b"old\n").unwrap();
        let dir = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();

        let error = atomic_write_with_sync(&dir, OsStr::new("artifact.json"), b"new\n", |_| {
            Err(io::Error::other("injected pre-rename sync failure"))
        })
        .unwrap_err();

        assert!(error.to_string().contains("injected pre-rename"));
        assert_eq!(
            fs::read(root.path().join("artifact.json")).unwrap(),
            b"old\n"
        );
        assert!(fs::read_dir(root.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
    }
}
