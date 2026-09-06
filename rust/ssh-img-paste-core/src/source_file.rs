use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFileError {
    PathMustBeAbsolute,
    Open(std::io::ErrorKind),
    NotRegularFile,
    ReparsePoint,
    InvalidPng,
}

#[cfg(unix)]
fn absolute_parts(path: &Path) -> std::io::Result<(PathBuf, Vec<OsString>)> {
    use std::path::Component;

    let mut components = path.components();
    if !matches!(components.next(), Some(Component::RootDir)) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "source path is not absolute",
        ));
    }
    let mut parts = Vec::new();
    for component in components {
        match component {
            Component::Normal(part) => parts.push(part.to_owned()),
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "source path contains a non-normal component",
                ));
            }
        }
    }
    Ok((PathBuf::from("/"), parts))
}

#[cfg(windows)]
fn absolute_parts(path: &Path) -> std::io::Result<(PathBuf, Vec<OsString>)> {
    use std::path::Component;

    let mut components = path.components();
    let prefix = match components.next() {
        Some(Component::Prefix(prefix)) => prefix.as_os_str().to_owned(),
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "source path has no Windows prefix",
            ));
        }
    };
    if !matches!(components.next(), Some(Component::RootDir)) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "source path is not rooted",
        ));
    }
    let mut anchor = PathBuf::from(prefix);
    anchor.push(Path::new(r"\"));
    let mut parts = Vec::new();
    for component in components {
        match component {
            Component::Normal(part) => parts.push(part.to_owned()),
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "source path contains a non-normal component",
                ));
            }
        }
    }
    Ok((anchor, parts))
}

fn open_absolute_nofollow(path: &Path) -> std::io::Result<File> {
    let (anchor, parts) = absolute_parts(path)?;
    let (file_name, parents) = parts.split_last().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "source path has no file name",
        )
    })?;
    let root = cap_std::fs::Dir::open_ambient_dir(anchor, cap_std::ambient_authority())?;
    let mut directory = root.into_std_file();
    for parent in parents {
        directory = cap_primitives::fs::open_dir_nofollow(&directory, Path::new(parent))?;
    }

    let mut options = cap_primitives::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use cap_primitives::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use cap_primitives::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    cap_primitives::fs::open(&directory, Path::new(file_name), &options)
}

pub fn open_upload_source(path: &Path) -> Result<File, SourceFileError> {
    if !path.is_absolute() {
        return Err(SourceFileError::PathMustBeAbsolute);
    }

    let mut file =
        open_absolute_nofollow(path).map_err(|error| SourceFileError::Open(error.kind()))?;
    let metadata = file
        .metadata()
        .map_err(|error| SourceFileError::Open(error.kind()))?;

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(SourceFileError::ReparsePoint);
        }
    }
    if !metadata.is_file() {
        return Err(SourceFileError::NotRegularFile);
    }
    let mut signature = [0_u8; 8];
    if let Err(error) = file.read_exact(&mut signature) {
        return if error.kind() == std::io::ErrorKind::UnexpectedEof {
            Err(SourceFileError::InvalidPng)
        } else {
            Err(SourceFileError::Open(error.kind()))
        };
    }
    if signature != *b"\x89PNG\r\n\x1a\n" {
        return Err(SourceFileError::InvalidPng);
    }
    file.rewind()
        .map_err(|error| SourceFileError::Open(error.kind()))?;
    Ok(file)
}

pub fn generate_remote_name(epoch_seconds: u64, entropy: [u8; 8]) -> String {
    let mut suffix = String::with_capacity(16);
    for byte in entropy {
        use std::fmt::Write;
        write!(&mut suffix, "{byte:02x}").expect("writing to a String cannot fail");
    }
    format!("ssh-img-{epoch_seconds}-{suffix}.png")
}
