//! OS privilege restrictions for the document child.
//!
//! macOS uses Seatbelt, before stdin is consumed. Its parent owns two private
//! directories so it can clean them after killing the child. Vision uses both
//! Darwin's temporary and cache paths; TMPDIR alone does not redirect the latter.
//! Other platforms retain the existing process/resource boundary for now.

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{CStr, CString};
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const SUFFIX_ENV: &str = "SOVATELA_DOC_SANDBOX_SUFFIX";
    const TEMP_ENV: &str = "SOVATELA_DOC_SANDBOX_TEMP";
    const CACHE_ENV: &str = "SOVATELA_DOC_SANDBOX_CACHE";
    const PREFIX: &str = "com.anaubi.sovatela.doc-";

    extern "C" {
        fn sandbox_init(
            profile: *const libc::c_char,
            flags: u64,
            error: *mut *mut libc::c_char,
        ) -> i32;
        fn sandbox_free_error(error: *mut libc::c_char);
        // Apple SPI used by WebKit for per-process Darwin cache isolation.
        fn _set_user_dir_suffix(suffix: *const libc::c_char) -> bool;
    }

    fn failure(message: &str) -> io::Error {
        io::Error::other(message)
    }

    fn darwin_directory(key: i32) -> io::Result<PathBuf> {
        let mut buf = vec![0u8; 4096];
        let len = unsafe { libc::confstr(key, buf.as_mut_ptr().cast(), buf.len()) };
        if len == 0 || len > buf.len() {
            return Err(failure("Darwin scratch directory unavailable"));
        }
        use std::os::unix::ffi::OsStrExt;
        Ok(PathBuf::from(std::ffi::OsStr::from_bytes(&buf[..len - 1])))
    }

    fn verify_directory(path: &Path) -> io::Result<PathBuf> {
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::getuid() }
            || metadata.mode() & 0o777 != 0o700
        {
            return Err(failure("scratch directory is not private"));
        }
        std::fs::canonicalize(path)
    }

    fn valid_suffix(suffix: &str) -> bool {
        suffix
            .strip_prefix(PREFIX)
            .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
    }

    fn close_inherited_files() -> io::Result<()> {
        // Enumerate rather than trusting the current soft fd limit: inherited
        // descriptors can have numbers above a subsequently lowered limit.
        let pid = unsafe { libc::getpid() };
        let size = std::mem::size_of::<libc::proc_fdinfo>();
        for _ in 0..4 {
            let needed = unsafe {
                libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0)
            };
            if needed <= 0 || needed > 8 * 1024 * 1024 {
                return Err(failure("could not enumerate inherited descriptors"));
            }
            let slots = needed as usize / size + 64;
            let mut fds = vec![
                libc::proc_fdinfo {
                    proc_fd: 0,
                    proc_fdtype: 0
                };
                slots
            ];
            let capacity = (fds.len() * size) as i32;
            let used = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDLISTFDS,
                    0,
                    fds.as_mut_ptr().cast(),
                    capacity,
                )
            };
            if used <= 0 {
                return Err(failure("could not read inherited descriptors"));
            }
            if used >= capacity {
                continue;
            }
            for fd in &fds[..used as usize / size] {
                if fd.proc_fd > 2 {
                    unsafe { libc::close(fd.proc_fd) };
                }
            }
            return Ok(());
        }
        Err(failure("inherited descriptor list did not stabilise"))
    }

    /// The owning parent keeps this until the child has exited or been killed.
    pub struct Prepared {
        suffix: String,
        temp: PathBuf,
        cache: PathBuf,
    }

    impl Prepared {
        pub fn new() -> io::Result<Self> {
            let mut random = [0u8; 16];
            unsafe { libc::arc4random_buf(random.as_mut_ptr().cast(), random.len()) };
            let suffix = format!(
                "{PREFIX}{}",
                random
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            );
            let temp = std::fs::canonicalize(darwin_directory(libc::_CS_DARWIN_USER_TEMP_DIR)?)?
                .join(&suffix);
            let cache = std::fs::canonicalize(darwin_directory(libc::_CS_DARWIN_USER_CACHE_DIR)?)?
                .join(&suffix);
            std::fs::DirBuilder::new().mode(0o700).create(&temp)?;
            if let Err(error) = std::fs::DirBuilder::new().mode(0o700).create(&cache) {
                let _ = std::fs::remove_dir(&temp);
                return Err(error);
            }
            let prepared = Self {
                suffix,
                temp,
                cache,
            };
            verify_directory(&prepared.temp)?;
            verify_directory(&prepared.cache)?;
            Ok(prepared)
        }

        pub fn configure(&self, command: &mut Command) {
            // Do not inherit API keys, proxy settings or dynamic-loader overrides.
            command
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", &self.temp)
                .env("CFFIXED_USER_HOME", &self.temp)
                .env("TMPDIR", &self.temp)
                .env(SUFFIX_ENV, &self.suffix)
                .env(TEMP_ENV, &self.temp)
                .env(CACHE_ENV, &self.cache)
                .current_dir(&self.temp);
        }
    }

    impl Drop for Prepared {
        fn drop(&mut self) {
            // remove_dir_all does not follow symlinks, including a replaced root.
            // Only these two unpredictable directories are ever cleanup targets.
            let _ = std::fs::remove_dir_all(&self.temp);
            let _ = std::fs::remove_dir_all(&self.cache);
        }
    }

    // SBPL strings are Scheme strings, not shell strings. Reject control bytes
    // and escape both metacharacters; paths never become profile expressions.
    fn quote(path: &Path) -> io::Result<String> {
        let text = path
            .to_str()
            .ok_or_else(|| failure("non-UTF-8 sandbox path"))?;
        if !path.is_absolute() || text.chars().any(char::is_control) {
            return Err(failure("invalid sandbox path"));
        }
        Ok(format!(
            "\"{}\"",
            text.replace('\\', "\\\\").replace('"', "\\\"")
        ))
    }

    /// The application bundle the executable runs from, if it runs from one.
    ///
    /// `X.app/Contents/MacOS/<executable>` gives `X.app`; anything else, such as
    /// a bare binary under `target/`, gives nothing.
    fn bundle_root(executable: &Path) -> Option<&Path> {
        let macos = executable.parent()?;
        let contents = macos.parent()?;
        let bundle = contents.parent()?;
        (macos.file_name()? == "MacOS"
            && contents.file_name()? == "Contents"
            && bundle.extension()? == "app")
            .then_some(bundle)
    }

    fn profile(temp: &Path, cache: &Path, executable: &Path) -> io::Result<CString> {
        let mut result = include_str!("doc_seatbelt.sb").to_owned();
        for directory in [temp, cache] {
            let path = quote(directory)?;
            result.push_str(&format!("\n(allow file-read* file-write* (subpath {path}))\n(allow file-read-metadata file-test-existence (path-ancestors {path}))\n"));
        }
        let exe = quote(executable)?;
        let parent = quote(
            executable
                .parent()
                .ok_or_else(|| failure("missing executable parent"))?,
        )?;
        result.push_str(&format!("\n(allow file-read-metadata file-test-existence (literal {exe}) (path-ancestors {exe}))\n(allow file-read-data (literal {exe}) (literal {parent}))\n"));
        // Read-only access to the helper's own application bundle.
        //
        // Inside `Sovatela.app`, CoreFoundation resolves the main bundle — its
        // directory and `Info.plist` — and Vision fails without it: the
        // notarized 1.10.0 draft refused every scan with "__objc2.missingError"
        // while the same source read them as a bare binary. Every check before
        // then, local and CI, ran the helper bare, so none could see it. The
        // bundle is the application's own signed code and resources; reading
        // it grants nothing the helper's user could not already see.
        if let Some(bundle) = bundle_root(executable) {
            let bundle = quote(bundle)?;
            result.push_str(&format!("\n(allow file-read* (subpath {bundle}))\n"));
        }
        CString::new(result).map_err(|_| failure("invalid sandbox profile"))
    }

    /// Keeps scratch alive for direct CLI invocations (QA tools). App invocations
    /// instead use the parent's guard, which also survives a child crash.
    pub struct Active {
        _owned: Option<Prepared>,
    }

    /// Must run once, on the helper's main thread, before parsing any input.
    pub fn enter() -> io::Result<Active> {
        let owned = if std::env::var_os(SUFFIX_ENV).is_none() {
            Some(Prepared::new()?)
        } else {
            None
        };
        let (suffix, expected_temp, expected_cache) = if let Some(p) = &owned {
            (p.suffix.clone(), p.temp.clone(), p.cache.clone())
        } else {
            let suffix =
                std::env::var(SUFFIX_ENV).map_err(|_| failure("missing scratch suffix"))?;
            let temp =
                std::env::var_os(TEMP_ENV).ok_or_else(|| failure("missing temporary directory"))?;
            let cache =
                std::env::var_os(CACHE_ENV).ok_or_else(|| failure("missing cache directory"))?;
            (suffix, PathBuf::from(temp), PathBuf::from(cache))
        };
        if !valid_suffix(&suffix) {
            return Err(failure("invalid scratch suffix"));
        }
        let suffix = CString::new(suffix).map_err(|_| failure("invalid scratch suffix"))?;
        if !unsafe { _set_user_dir_suffix(suffix.as_ptr()) } {
            return Err(failure("could not isolate Darwin scratch paths"));
        }
        let temp = verify_directory(&darwin_directory(libc::_CS_DARWIN_USER_TEMP_DIR)?)?;
        let cache = verify_directory(&darwin_directory(libc::_CS_DARWIN_USER_CACHE_DIR)?)?;
        if temp != expected_temp || cache != expected_cache {
            return Err(failure(
                "Darwin scratch paths differ from the parent's paths",
            ));
        }
        std::env::set_var("TMPDIR", &temp);
        std::env::set_var("HOME", &temp);
        std::env::set_var("CFFIXED_USER_HOME", &temp);
        std::env::set_current_dir(&temp)?;
        let executable = std::fs::canonicalize(std::env::current_exe()?)?;
        let policy = profile(&temp, &cache, &executable)?;
        // Do not retain arbitrary inherited file descriptors alongside the two
        // IPC pipes and stderr. This cannot revoke pre-existing Mach ports.
        close_inherited_files()?;
        let mut error = std::ptr::null_mut();
        let status = unsafe { sandbox_init(policy.as_ptr(), 0, &mut error) };
        let message = if error.is_null() {
            "could not install document sandbox".to_owned()
        } else {
            let message = unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned();
            unsafe { sandbox_free_error(error) };
            message
        };
        if status != 0 {
            return Err(failure(&message));
        }
        Ok(Active { _owned: owned })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn paths_cannot_inject_profile_rules() {
            assert_eq!(
                quote(Path::new("/tmp/a\"b\\c")).unwrap(),
                "\"/tmp/a\\\"b\\\\c\""
            );
            assert!(quote(Path::new("relative")).is_err());
            assert!(quote(Path::new("/tmp/a\n(allow default)")).is_err());
        }

        #[test]
        fn the_bundle_is_found_only_for_an_app_executable() {
            assert_eq!(
                bundle_root(Path::new("/Applications/Sovatela.app/Contents/MacOS/scale")),
                Some(Path::new("/Applications/Sovatela.app"))
            );
            for bare in [
                "/Users/x/Scale/src-tauri/target/debug/scale",
                "/Applications/Sovatela.app/Contents/Resources/scale",
                "/tmp/Contents/MacOS/scale",
                "/scale",
            ] {
                assert_eq!(bundle_root(Path::new(bare)), None, "{bare}");
            }
        }

        #[test]
        fn a_bundled_helper_may_read_its_own_bundle_and_a_bare_one_gains_nothing() {
            let temp = Path::new("/private/var/folders/x/T/s");
            let cache = Path::new("/private/var/folders/x/C/s");
            let bundled = profile(
                temp,
                cache,
                Path::new("/Applications/Sovatela.app/Contents/MacOS/scale"),
            )
            .unwrap();
            assert!(bundled
                .to_str()
                .unwrap()
                .contains("(allow file-read* (subpath \"/Applications/Sovatela.app\"))"));
            let bare = profile(temp, cache, Path::new("/opt/scale/target/debug/scale")).unwrap();
            assert!(!bare
                .to_str()
                .unwrap()
                .contains("(allow file-read* (subpath \"/opt"));
        }

        #[test]
        fn suffix_cannot_escape_private_directory() {
            assert!(valid_suffix(&format!("{PREFIX}{}", "a".repeat(32))));
            for suffix in [
                "../outside",
                "",
                PREFIX,
                "com.anaubi.sovatela.doc-../../outside",
            ] {
                assert!(!valid_suffix(suffix));
            }
        }

        #[test]
        fn scratch_cleanup_does_not_follow_a_symlink() {
            let scratch = Prepared::new().unwrap();
            let other = Prepared::new().unwrap();
            let canary = other.temp.join("canary");
            std::fs::write(&canary, b"keep").unwrap();
            std::os::unix::fs::symlink(&other.temp, scratch.temp.join("outside")).unwrap();
            let paths = [scratch.temp.clone(), scratch.cache.clone()];
            drop(scratch);
            assert!(paths.iter().all(|p| !p.exists()));
            assert_eq!(std::fs::read(canary).unwrap(), b"keep");
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::{enter, Prepared};
