use anyhow::{anyhow, Context as _};
use config::{create_user_owned_dirs, UnixDomain};
use promise::spawn::spawn_into_main_thread;
use std::path::Path;
use std::time::Duration;
use wezterm_uds::{UnixListener, UnixStream};

pub struct LocalListener {
    listener: UnixListener,
}

impl LocalListener {
    pub fn new(listener: UnixListener) -> Self {
        Self { listener }
    }

    pub fn with_domain(unix_dom: &UnixDomain) -> anyhow::Result<Self> {
        let listener = safely_create_sock_path(unix_dom)?;
        Ok(Self::new(listener))
    }

    pub fn run(&mut self) {
        for stream in self.listener.incoming() {
            match stream {
                Ok(stream) => {
                    // LBTerm: a send buffer that holds a few of a busy pane's pushes, not tens
                    // (lbterm_flow.rs)
                    if let Err(err) = crate::lbterm_flow::set_send_buffer(&stream) {
                        log::error!("setting a client socket's send buffer: {err:#}");
                    }
                    spawn_into_main_thread(async move {
                        crate::dispatch::process(stream).await.map_err(|e| {
                            log::error!("{:#}", e);
                            e
                        })
                    })
                    .detach();
                }
                Err(err) => {
                    log::error!("accept failed: {}", err);
                    return;
                }
            }
        }
    }
}

/// Take care when setting up the listener socket;
/// we need to be sure that the directory that we create it in
/// is owned by the user and has appropriate file permissions
/// that prevent other users from manipulating its contents.
fn safely_create_sock_path(unix_dom: &UnixDomain) -> anyhow::Result<UnixListener> {
    let sock_path = &unix_dom.socket_path();
    log::trace!("setting up {}", sock_path.display());

    let sock_dir = sock_path
        .parent()
        .ok_or_else(|| anyhow!("sock_path {} has no parent dir", sock_path.display()))?;

    create_user_owned_dirs(sock_dir)?;

    #[cfg(unix)]
    {
        use config::running_under_wsl;
        use std::os::unix::fs::PermissionsExt;

        if !running_under_wsl() && !unix_dom.skip_permissions_check {
            // Let's be sure that the ownership looks sane
            let meta = sock_dir.symlink_metadata()?;

            let permissions = meta.permissions();
            if (permissions.mode() & 0o22) != 0 {
                anyhow::bail!(
                    "The permissions for {} are insecure and currently \
                     allow other users to write to it (permissions={:?})",
                    sock_dir.display(),
                    permissions
                );
            }
        }
    }

    // LBTerm: never take a socket another server is listening on. Below, a socket that's there is
    // removed and bound again, so a server started on a live server's path took it: on
    // 2026-10-29 a test's mux server whose config failed to load fell back to the default domain,
    // took ~/.local/share/lbterm/sock from Lachlan's daily server, then ended with its test,
    // leaving a dead file that nothing could reach the daily server through
    if UnixStream::connect(sock_path).is_ok() {
        anyhow::bail!(
            "another mux server is listening at {}: not taking its socket",
            sock_path.display()
        );
    }
    bind_over(sock_path)
}

/// The listener on `sock_path`, over whatever file is there (one nothing answers on, or one being
/// taken back by `keep_socket`)
fn bind_over(sock_path: &Path) -> anyhow::Result<UnixListener> {
    // We want to remove the socket if it exists.
    // However, on windows, we can't tell if the unix domain socket
    // exists using the methods on Path, so instead we just unconditionally
    // remove it and see what error occurs.
    match std::fs::remove_file(sock_path) {
        Ok(_) => {}
        Err(err) => match err.kind() {
            std::io::ErrorKind::NotFound => {}
            _ => return Err(err).context(format!("Unable to remove {}", sock_path.display())),
        },
    }

    let listener = UnixListener::bind(sock_path)
        .with_context(|| format!("Failed to bind to {}", sock_path.display()))?;

    config::set_sticky_bit(&sock_path);

    Ok(listener)
}

/// How often `keep_socket` looks at its socket's file
pub const KEEP_EVERY: Duration = Duration::from_secs(3);

/// A file, by volume and index (device and inode on unix)
type FileId = (u64, u64);

/// Which file is at `path`: its volume and file index, which a file made again under the same name
/// doesn't keep (its creation time it does, for 15 s: NTFS's tunnelling, so that can't tell them
/// apart). Opened as the reparse point it is, since an AF_UNIX socket's can't be followed
#[cfg(windows)]
fn file_at(path: &Path) -> Option<FileId> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT,
    };
    let file = std::fs::OpenOptions::new()
        .access_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 {
        return None;
    }
    Some((
        info.dwVolumeSerialNumber as u64,
        ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
    ))
}

#[cfg(unix)]
fn file_at(path: &Path) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(path).ok()?;
    Some((meta.dev(), meta.ino()))
}

/// LBTerm: the server keeps its socket. A process that removes the file at a server's socket path
/// and binds its own (an older LBTerm mux server whose config failed to load does that with the
/// default path) cuts the server off from every new client, and when that process ends it leaves
/// a dead file there. Every `KEEP_EVERY` this looks at the file; one that's gone or is another
/// file (by its file index, `file_at`) is taken back, with a listener of its own, so clients reach
/// this server again within a few seconds. Servers from this build never take a live socket
/// (`safely_create_sock_path`), so the only other process that can be there is one that
/// shouldn't be.
pub fn keep_socket(unix_dom: &UnixDomain) {
    let path = unix_dom.socket_path();
    let Some(ours) = file_at(&path) else {
        log::warn!(
            "{}: can't tell which file the socket is, so it isn't kept",
            path.display()
        );
        return;
    };
    let watched = path.clone();
    let spawned = std::thread::Builder::new()
        .name("lbterm-keep-socket".into())
        .spawn(move || {
            let mut ours = ours;
            let mut failed = 0u32;
            loop {
                // After a failure, wait longer each time (up to 32 times as long), so a socket
                // that can't be bound isn't tried, and logged, every few seconds for ever
                std::thread::sleep(KEEP_EVERY * 2u32.pow(failed.min(5)));
                match check_socket(&watched, ours) {
                    Kept::Ours => failed = 0,
                    Kept::TakenBack(now) => {
                        ours = now;
                        failed = 0;
                    }
                    Kept::Failed => failed += 1,
                    Kept::Lost => return,
                }
            }
        });
    if let Err(err) = spawned {
        log::error!("keeping {}: {:#}", path.display(), err);
    }
}

/// What a look at the socket found
#[derive(Debug, PartialEq)]
enum Kept {
    /// The file is still this server's
    Ours,
    /// It was gone or another's, and is this server's again: this file
    TakenBack(FileId),
    /// It couldn't be bound again: tried again later
    Failed,
    /// Bound again, but which file it is can't be told, so it can't be kept any more
    Lost,
}

/// One look at the socket file at `path`, which was `ours` when this server last bound it: when
/// it's gone or another file, bind it over with a listener of this server's
fn check_socket(path: &Path, ours: FileId) -> Kept {
    let now = file_at(path);
    if now == Some(ours) {
        return Kept::Ours;
    }
    log::error!(
        "{}: the socket {}; taking it back",
        path.display(),
        if now.is_none() {
            "was removed"
        } else {
            "was replaced by another process's"
        }
    );
    match bind_over(path) {
        Ok(listener) => {
            let mut listener = LocalListener::new(listener);
            std::thread::spawn(move || listener.run());
            match file_at(path) {
                Some(now) => Kept::TakenBack(now),
                None => {
                    log::warn!(
                        "{}: took the socket back but can't tell which file it is, so it isn't kept",
                        path.display()
                    );
                    Kept::Lost
                }
            }
        }
        Err(err) => {
            log::error!("taking back {}: {:#}", path.display(), err);
            Kept::Failed
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::path::PathBuf;

    /// A socket path in a folder of its own under the temp folder, which goes with it
    struct TempSock(PathBuf);

    impl TempSock {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("lbterm-keep-socket-{name}-{}", std::process::id()));
            std::fs::remove_dir_all(&dir).ok();
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir.join("sock"))
        }

        fn domain(&self) -> UnixDomain {
            UnixDomain {
                socket_path: Some(self.0.clone()),
                ..Default::default()
            }
        }
    }

    impl Drop for TempSock {
        fn drop(&mut self) {
            if let Some(dir) = self.0.parent() {
                std::fs::remove_dir_all(dir).ok();
            }
        }
    }

    #[test]
    fn a_live_socket_is_never_taken() {
        let sock = TempSock::new("live");
        let first = safely_create_sock_path(&sock.domain()).unwrap();
        let err = safely_create_sock_path(&sock.domain())
            .err()
            .expect("a live socket is never taken");
        assert!(
            format!("{err:#}").contains("another mux server is listening"),
            "{err:#}"
        );
        // A server that's gone leaves its file, which nothing answers: that one is bound over
        drop(first);
        safely_create_sock_path(&sock.domain()).expect("a dead socket is bound over");
    }

    #[test]
    fn a_socket_taken_over_is_taken_back() {
        let sock = TempSock::new("taken");
        let _ours = safely_create_sock_path(&sock.domain()).unwrap();
        let ours = file_at(&sock.0).expect("which file a socket is can be told");
        assert_eq!(check_socket(&sock.0, ours), Kept::Ours);

        // Another server's, bound over ours the way an older one does it, and then gone
        let theirs = bind_over(&sock.0).unwrap();
        let replaced = file_at(&sock.0).unwrap();
        assert_ne!(replaced, ours, "a file made again under the same name is another file");
        drop(theirs);
        match check_socket(&sock.0, ours) {
            Kept::TakenBack(now) => assert!(now != replaced && now != ours),
            other => panic!("not taken back: {:?}", other),
        }
    }

    #[test]
    fn a_removed_socket_is_bound_again() {
        let sock = TempSock::new("removed");
        let _ours = safely_create_sock_path(&sock.domain()).unwrap();
        let ours = file_at(&sock.0).unwrap();
        std::fs::remove_file(&sock.0).unwrap();
        assert!(matches!(check_socket(&sock.0, ours), Kept::TakenBack(_)));
        assert!(file_at(&sock.0).is_some());
    }
}
