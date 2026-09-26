//! Exercise the production Seatbelt installer in a disposable child process.
//! These tests must run outside any surrounding sandbox that prohibits installing
//! a Seatbelt profile. They do not add a diagnostic mode to the shipped binary.
#![cfg(target_os = "macos")]

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

fn denied<T>(result: io::Result<T>) {
    match result {
        Err(error) => assert_eq!(error.raw_os_error(), Some(libc::EPERM), "{error}"),
        Ok(_) => panic!("operation escaped confinement"),
    }
}

#[test]
fn confined_child() {
    let Some(outside) = std::env::var_os("SOVATELA_CONFINEMENT_TEST") else {
        return;
    };
    let outside = PathBuf::from(outside);
    let canary = outside.join("canary");
    use std::os::fd::AsRawFd;
    let file = std::fs::File::open(&canary).unwrap();
    let inherited = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, 200) };
    assert!(inherited >= 200);
    drop(file);
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, unsafe {
        libc::getppid()
    }];
    let mut size = 0usize;
    // Query only the size, never the parent's actual argv/environment bytes.
    assert_eq!(
        unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        },
        0
    );
    let active = scale_lib::doc_confinement::enter().unwrap();
    assert_eq!(unsafe { libc::fcntl(inherited, libc::F_GETFD) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
    assert_eq!(
        unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        },
        -1
    );
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM));

    denied(std::fs::read(&canary));
    denied(std::fs::write(&canary, b"overwrite"));
    denied(std::fs::write(outside.join("new-file"), b"create"));
    denied(std::fs::read_dir(&outside));
    // Model path canonicalization needs metadata on /System/Library itself;
    // path-ancestors alone omits it. Its directory listing is still denied.
    std::fs::canonicalize("/System/Library/PrivateFrameworks").unwrap();
    denied(std::fs::read_dir("/System/Library"));
    let scratch = PathBuf::from(std::env::var_os("TMPDIR").unwrap());
    std::fs::write(scratch.join("allowed"), b"private scratch").unwrap();
    assert_eq!(
        std::fs::read(scratch.join("allowed")).unwrap(),
        b"private scratch"
    );
    std::os::unix::fs::symlink(&canary, scratch.join("escape")).unwrap();
    denied(std::fs::read(scratch.join("escape")));
    denied(std::fs::write(scratch.join("escape"), b"overwrite"));
    denied(std::fs::hard_link(&canary, scratch.join("hard-link")));
    denied(std::net::TcpStream::connect("127.0.0.1:9"));
    // The executable is inside readable scratch, so failure cannot merely be
    // attributed to denying reads of /usr/bin/true.
    denied(Command::new(scratch.join("true")).status());
    let forked = unsafe { libc::fork() };
    if forked == 0 {
        unsafe { libc::_exit(0) };
    }
    if forked > 0 {
        unsafe { libc::waitpid(forked, std::ptr::null_mut(), 0) };
        panic!("fork escaped confinement");
    }
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM));
    drop(active);
    println!("CONFINEMENT_CANARIES_PASSED");
}

#[test]
fn file_network_process_denials_and_parent_cleanup() {
    let parent = scale_lib::doc_confinement::Prepared::new().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    parent.configure(&mut command);
    let env_path = |name: &str| -> PathBuf {
        command
            .get_envs()
            .find(|(key, _)| *key == name)
            .and_then(|(_, value)| value)
            .unwrap()
            .into()
    };
    let temp = env_path("SOVATELA_DOC_SANDBOX_TEMP");
    let cache = env_path("SOVATELA_DOC_SANDBOX_CACHE");
    let outside = temp.with_extension("canaries");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("canary"), b"unchanged").unwrap();
    std::fs::copy("/usr/bin/true", temp.join("true")).unwrap();
    let result = command
        .args(["--exact", "confined_child", "--nocapture"])
        .env("SOVATELA_CONFINEMENT_TEST", &outside)
        .output()
        .unwrap();
    drop(parent);
    assert!(!temp.exists(), "temporary directory leaked");
    assert!(!cache.exists(), "cache directory leaked");
    let canary = std::fs::read(outside.join("canary")).unwrap();
    let extra_created = outside.join("new-file").exists();
    std::fs::remove_dir_all(&outside).unwrap();
    assert_eq!(canary, b"unchanged");
    assert!(!extra_created);
    assert!(
        result.status.success(),
        "child failed: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("CONFINEMENT_CANARIES_PASSED"));
}

#[test]
fn invalid_profile_paths_fail_closed_before_document_reading() {
    // A valid helper invocation with corrupt parent configuration must refuse
    // explicitly, not retry without confinement or enter the document parser.
    let result = Command::new(env!("CARGO_BIN_EXE_scale"))
        .args(["--sovatela-extract-doc-helper", "pdf"])
        .env("SOVATELA_DOC_SANDBOX_SUFFIX", "../../outside")
        .env("SOVATELA_DOC_SANDBOX_TEMP", Path::new("/private/tmp"))
        .env("SOVATELA_DOC_SANDBOX_CACHE", Path::new("/private/tmp"))
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(33));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "SOVATELA-PDF/1\nthe document reader could not start safely, so this file was not read."
    );
}
