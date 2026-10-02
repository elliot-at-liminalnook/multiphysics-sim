//! Retained directory capabilities. Every Rust file operation resolves relative
//! to an open directory descriptor with O_NOFOLLOW; pathname replacement cannot
//! redirect a read, create or publication into another directory.
#[cfg(unix)]
mod unix {
    use std::{
        ffi::CString,
        fs::File,
        io::Read,
        os::fd::{AsRawFd, FromRawFd},
        path::{Component, Path},
    };
    pub struct Directory {
        file: File,
    }
    fn name(value: &std::ffi::OsStr) -> Result<CString, String> {
        use std::os::unix::ffi::OsStrExt;
        CString::new(value.as_bytes()).map_err(|_| "path contains NUL".into())
    }
    fn result(fd: i32) -> Result<File, String> {
        if fd < 0 {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }
    impl Directory {
        pub fn open(path: &Path) -> Result<Self, String> {
            let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
            let mut at = Self {
                file: File::open("/").map_err(|e| e.to_string())?,
            };
            for c in absolute.components() {
                match c {
                    Component::RootDir => {}
                    Component::Normal(part) => at = at.child(part)?,
                    _ => return Err("directory path contains unsafe component".into()),
                }
            }
            Ok(at)
        }
        fn child(&self, part: &std::ffi::OsStr) -> Result<Self, String> {
            let part = name(part)?;
            let file = result(unsafe {
                libc::openat(
                    self.file.as_raw_fd(),
                    part.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            })?;
            Ok(Self { file })
        }
        pub fn create(&self, part: &str) -> Result<Self, String> {
            super::super::isolation::relative(part)?;
            if Path::new(part).components().count() != 1 {
                return Err("directory name must be one component".into());
            }
            let n = name(std::ffi::OsStr::new(part))?;
            if unsafe { libc::mkdirat(self.file.as_raw_fd(), n.as_ptr(), 0o700) } != 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            self.child(std::ffi::OsStr::new(part))
        }
        fn parent(&self, path: &Path, create: bool) -> Result<(Self, CString), String> {
            super::super::isolation::relative(path.to_str().ok_or("non-UTF8 relative file path")?)?;
            let mut at = Self {
                file: self.file.try_clone().map_err(|e| e.to_string())?,
            };
            let mut components = path.components().peekable();
            while let Some(Component::Normal(part)) = components.next() {
                if components.peek().is_none() {
                    return Ok((at, name(part)?));
                }
                at = match at.child(part) {
                    Ok(dir) => dir,
                    Err(error) => {
                        if create {
                            match at.create(part.to_str().ok_or("nonUTF8 directory")?) {
                                Ok(dir) => dir,
                                Err(_) => at.child(part).map_err(|_| error)?,
                            }
                        } else {
                            return Err(error);
                        }
                    }
                };
            }
            Err("missing filename".into())
        }
        pub fn new_file(&self, path: &Path) -> Result<File, String> {
            let (parent, name) = self.parent(path, true)?;
            result(unsafe {
                libc::openat(
                    parent.file.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            })
        }
        pub fn read(&self, path: &Path, max: u64) -> Result<Vec<u8>, String> {
            let (parent, name) = self.parent(path, false)?;
            let file = result(unsafe {
                libc::openat(
                    parent.file.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                )
            })?;
            let m = file.metadata().map_err(|e| e.to_string())?;
            if !m.is_file() || m.len() > max {
                return Err("nonregular or oversized input".into());
            }
            let mut bytes = Vec::new();
            file.take(max + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > max {
                return Err("input exceeds bound".into());
            }
            Ok(bytes)
        }
        pub fn link(&self, from: &Path, to: &Path) -> Result<(), String> {
            let (source, a) = self.parent(from, false)?;
            let (target, b) = self.parent(to, false)?;
            if unsafe {
                libc::linkat(
                    source.file.as_raw_fd(),
                    a.as_ptr(),
                    target.file.as_raw_fd(),
                    b.as_ptr(),
                    0,
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(())
        }
        pub fn sync(&self) -> Result<(), String> {
            self.file.sync_all().map_err(|e| e.to_string())
        }
    }
}
#[cfg(unix)]
pub use unix::Directory;
#[cfg(not(unix))]
pub struct Directory;
#[cfg(not(unix))]
impl Directory {
    pub fn open(_: &std::path::Path) -> Result<Self, String> {
        Err("retained directory ownership unavailable on this platform".into())
    }
    pub fn create(&self, _: &str) -> Result<Self, String> {
        Err("directory capability unavailable".into())
    }
    pub fn new_file(&self, _: &std::path::Path) -> Result<std::fs::File, String> {
        Err("directory capability unavailable".into())
    }
    pub fn read(&self, _: &std::path::Path, _: u64) -> Result<Vec<u8>, String> {
        Err("directory capability unavailable".into())
    }
    pub fn link(&self, _: &std::path::Path, _: &std::path::Path) -> Result<(), String> {
        Err("directory capability unavailable".into())
    }
    pub fn sync(&self) -> Result<(), String> {
        Err("directory capability unavailable".into())
    }
}

#[cfg(all(test, unix))]
mod fixtures {
    use super::Directory;
    use std::{io::Write, path::Path};
    #[test]
    fn retained_directory_refuses_symlink_reads_and_writes() {
        let base = std::env::temp_dir().canonicalize().unwrap();
        let name = format!(
            "cad-parity-fd-fixture-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = Directory::open(&base).unwrap().create(&name).unwrap();
        let path = base.join(name);
        root.create("owned").unwrap();
        let mut f = root.new_file(Path::new("owned/source")).unwrap();
        f.write_all(b"durable").unwrap();
        std::os::unix::fs::symlink("owned", path.join("escape")).unwrap();
        assert!(root.read(Path::new("escape/source"), 64).is_err());
        assert!(root.new_file(Path::new("escape/new")).is_err());
        assert_eq!(
            root.read(Path::new("owned/source"), 64).unwrap(),
            b"durable"
        );
        assert!(root.new_file(Path::new("owned/source")).is_err());
    }
}
