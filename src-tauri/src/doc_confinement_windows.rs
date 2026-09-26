//! Windows document-helper confinement: AppContainer at process creation.
//!
//! macOS confines the child from inside — `sandbox_init` after start, so the
//! helper seals itself. Windows cannot do that. An AppContainer is a property
//! of the token a process is *created* with, so the parent has to apply it,
//! and `std::process::Command` cannot carry the attribute list that does so
//! (`raw_attribute` is unstable). The helper is therefore created here with
//! `CreateProcessW` and its pipes wired by hand.
//!
//! That is a lot of machinery to take on for one platform, and the reason is
//! the same one the Seatbelt work gave: the helper decodes attacker-supplied
//! document data and then hands it to a system recogniser, while running with
//! the user's own rights. Bounding a crash and an allocation is not bounding a
//! compromise.
//!
//! ## Status: working on the application path, gated off by default
//!
//! Behind the `windows-confinement` feature, which is not enabled by default.
//! `doc_sandbox::run` uses this when it is compiled in, and fails closed if
//! the container cannot be entered — there is no unconfined retry.
//!
//! `windows-confinement-validate.yml` exercises the real path:
//! `doc_sandbox::extract_text` reads a scanned PDF through a confined helper,
//! and the result is compared against the same fixture read unconfined. As of
//! 2026-09-24 they are **byte for byte identical** — the scratch directory is
//! created and ACL'd, the container is entered, the child starts, WinRT OCR
//! activates inside it and returns `INUOICE 12345` with exit 0.
//!
//! That settles the two risks this could have foundered on. An AppContainer is
//! built for packaged applications, and a desktop process placed in one can
//! fail to activate WinRT classes; had OCR not started, every scanned PDF on
//! Windows would have become "this machine has no recogniser".
//!
//! **One runner image, once.** Not Intel, not a clean machine, not any Windows
//! release but the Server 2025 image, not a signed artifact, and not a
//! real-photo accuracy test — the fixture is a synthetic bitmap whose known
//! misreads are `INUOICE` and `SOURTELR`. Do not enable this by default on a
//! single observation.
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Foundation::{SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT};
use windows::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows::Win32::Security::{FreeSid, PSID, SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

/// The container's name, and so its identity on disk.
///
/// Stable rather than per-run: a profile is a registry-backed object, and
/// creating one per extraction would leave a trail of them behind. The
/// per-run isolation comes from the scratch directory, as it does on macOS.
const PROFILE: &str = "com.anaubi.sovatela.doc";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn failure(what: &str) -> std::io::Error {
    std::io::Error::other(format!("document confinement: {what}"))
}

/// A validation-only switch, compiled out of any build that could ship.
///
/// The third review: a default-on build must not carry an ambient environment
/// variable that disables the handle whitelist, replaces the minimised
/// environment, or forces a refusal. These exist for the gate, which builds
/// with `windows-confinement-testhooks`. Without that feature the macro
/// discards its argument, so neither the lookup nor the variable's name is in
/// the binary — which the gate checks for.
#[cfg(feature = "windows-confinement-testhooks")]
macro_rules! test_hook {
    ($name:literal) => {
        std::env::var_os($name)
    };
}
#[cfg(not(feature = "windows-confinement-testhooks"))]
macro_rules! test_hook {
    ($name:literal) => {
        None::<std::ffi::OsString>
    };
}

/// The AppContainer SID, freed when dropped.
///
/// Both `CreateAppContainerProfile` and
/// `DeriveAppContainerSidFromAppContainerName` return a SID the caller must
/// release with `FreeSid`. Neither was released: the first review listed the
/// SIDs among the per-extraction leaks, the fix for it freed the ACL and the
/// security descriptor and not these, and the record called the finding fixed
/// without reading this function. The third review found it.
pub struct ContainerSid(PSID);

impl ContainerSid {
    /// The container's SID, creating the profile if this is the first run.
    ///
    /// An existing profile makes `CreateAppContainerProfile` fail, which is
    /// not an error here: the SID derives from the name either way.
    /// Distinguishing "it already exists" from a real failure by error code
    /// would couple this to HRESULT values that say less than simply asking
    /// for the SID and seeing whether one comes back.
    fn derive() -> std::io::Result<Self> {
        let name = wide(PROFILE);
        unsafe {
            if let Ok(created) = CreateAppContainerProfile(
                PCWSTR(name.as_ptr()),
                PCWSTR(name.as_ptr()),
                PCWSTR(name.as_ptr()),
                None,
            ) {
                return Ok(ContainerSid(created));
            }
            DeriveAppContainerSidFromAppContainerName(PCWSTR(name.as_ptr()))
                .map(ContainerSid)
                .map_err(|e| failure(&format!("could not derive the container identity ({e})")))
        }
    }
}

impl Drop for ContainerSid {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeSid(self.0);
        }
    }
}

fn raw(h: &OwnedHandle) -> HANDLE {
    HANDLE(h.as_raw_handle())
}

/// Let this container reach a directory the parent created.
///
/// A regular AppContainer is denied the user's resources and the network
/// unless granted — not literally everything: it retains access to selected
/// system files, registry keys and COM objects, which is what distinguishes it
/// from an LPAC. The scratch directory is made with permissions that do not
/// mention the container, so without this the helper cannot write there — which the CI self-test did not reveal, because it hands the
/// helper the process temp directory instead.
///
/// Grants modify/inherit on the directory itself. The parent still owns
/// creation and cleanup; this only opens what it already made.
fn grant_container_access(path: &std::path::Path, sid: &ContainerSid) -> std::io::Result<()> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::GetNamedSecurityInfoW;
    use windows::Win32::Security::Authorization::{
        SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, GRANT_ACCESS, SE_FILE_OBJECT,
        TRUSTEE_IS_SID, TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
    };
    use windows::Win32::Security::{ACL, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
    use windows::Win32::Security::{
        CONTAINER_INHERIT_ACE, NO_INHERITANCE, NO_PROPAGATE_INHERIT_ACE, OBJECT_INHERIT_ACE,
    };
    use windows::Win32::Storage::FileSystem::{
        DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_TRAVERSE,
    };

    let mut wide_path = wide(&path.to_string_lossy());
    unsafe {
        let trustee = TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
            ptstrName: windows::core::PWSTR(sid.0 .0.cast()),
            ..Default::default()
        };
        // Two entries, because traversal and execute are the same bit.
        //
        // FILE_TRAVERSE on a directory is FILE_EXECUTE on a file (0x20), and it
        // sits under execute in the generic mapping, not under read — this
        // comment used to say FILE_GENERIC_READ carried it, and the third
        // review corrected that. Hosted runners worked anyway, most likely
        // through traverse bypass. So the directory itself is granted
        // traversal and nothing inherits it, while what files and
        // subdirectories inherit carries no execute: a compromised helper
        // should not be able to write a binary into scratch and start it.
        let access = [
            EXPLICIT_ACCESS_W {
                grfAccessPermissions: FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0 | DELETE.0,
                grfAccessMode: GRANT_ACCESS,
                grfInheritance: windows::Win32::Security::ACE_FLAGS(
                    CONTAINER_INHERIT_ACE.0 | OBJECT_INHERIT_ACE.0 | NO_PROPAGATE_INHERIT_ACE.0,
                ),
                Trustee: trustee,
            },
            EXPLICIT_ACCESS_W {
                grfAccessPermissions: FILE_TRAVERSE.0,
                grfAccessMode: GRANT_ACCESS,
                grfInheritance: NO_INHERITANCE,
                Trustee: trustee,
            },
        ];
        // Merge into the existing DACL rather than replacing it. With
        // OldAcl = None, SetEntriesInAclW builds an ACL from the supplied entry
        // alone and discards the owner, user and SYSTEM entries the directory
        // already had. Inheritance may restore some; relying on that implicitly
        // is not the same as preserving them, particularly when the parent has
        // to delete this directory afterwards.
        let mut existing: *mut ACL = std::ptr::null_mut();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        if GetNamedSecurityInfoW(
            windows::core::PCWSTR(wide_path.as_mut_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&mut existing),
            None,
            &mut descriptor,
        )
        .is_err()
        {
            return Err(failure("could not read the scratch access list"));
        }
        let mut acl: *mut ACL = std::ptr::null_mut();
        let rc = SetEntriesInAclW(
            Some(&access),
            if existing.is_null() {
                None
            } else {
                Some(existing)
            },
            &mut acl,
        );
        if rc.is_err() {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            return Err(failure("could not build the container access list"));
        }
        let rc = SetNamedSecurityInfoW(
            windows::core::PCWSTR(wide_path.as_mut_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(acl),
            None,
        );
        // Both allocations are ours to release. Leaking them once per
        // extraction is small and unbounded, the worst combination.
        let _ = LocalFree(Some(HLOCAL(acl.cast())));
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        if rc.is_err() {
            return Err(failure(
                "could not grant the container access to its scratch",
            ));
        }
    }
    Ok(())
}

/// A scratch directory that removes itself.
///
/// The macOS side has this shape already — `doc_confinement::Prepared` removes
/// its scratch in `Drop`, so cleanup happens however the function exits. The
/// Windows path called `remove_dir_all` on two error branches and nowhere
/// else, so every successful extraction left a directory behind, and each new
/// early return was another chance to forget.
///
/// Dropped only after the helper is dead: `Confined` terminates its job and
/// waits for it to empty, because termination is asynchronous and a directory
/// removed under a living process is a different bug.
///
/// It also owns the container's SID, so there is one derivation per
/// extraction rather than one for the grant and another for the spawn, and a
/// helper can only be spawned into a directory that was granted.
pub struct Scratch {
    path: std::path::PathBuf,
    sid: ContainerSid,
}

impl Scratch {
    /// Create it, and grant this container access to it.
    ///
    /// The guard exists before the grant. It used to be constructed after,
    /// so a failed grant returned early and left the directory with no owner —
    /// the third review found it. `create_dir`, not `create_dir_all`: a
    /// directory that already exists belongs to someone else, and this guard
    /// deletes what it owns.
    pub fn new(path: std::path::PathBuf) -> std::io::Result<Self> {
        let sid = ContainerSid::derive()?;
        std::fs::create_dir(&path)?;
        let scratch = Scratch { path, sid };
        if test_hook!("SOVATELA_CONFINE_FORCE_GRANT_FAILURE").is_some() {
            // Whether the directory existed at this point, so the gate can
            // tell "removed by the guard" from "never created".
            eprintln!(
                "confined: scratch existed before forced grant failure: {}",
                scratch.path.exists()
            );
            return Err(failure("scratch grant failure forced for testing"));
        }
        grant_container_access(&scratch.path, &scratch.sid)?;
        Ok(scratch)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Best effort, as on macOS, but not a single attempt: a process that
        // has just been terminated can still be closing its handles, and one
        // attempt against that is a coin toss. Bounded, because a destructor
        // that can wait forever is worse than a directory left behind.
        for _ in 0..20 {
            if std::fs::remove_dir_all(&self.path).is_ok() || !self.path.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        eprintln!(
            "confined: scratch could not be removed: {}",
            self.path.display()
        );
    }
}

/// One end of a pipe the child inherits, and one the parent keeps.
struct Pipe {
    parent: OwnedHandle,
    child: OwnedHandle,
}

/// The parent end is never inheritable. A parent end the child could inherit
/// would keep the pipe open after the child's own copy closed, so the parent
/// would wait for EOF that never arrives — a hang indistinguishable from a
/// wedged recogniser.
///
/// Both ends are owned from the moment they exist, so a failure detaching one
/// closes both rather than leaking them.
fn pipe(child_is_read_end: bool) -> std::io::Result<Pipe> {
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
    unsafe {
        CreatePipe(&mut read, &mut write, Some(&sa), 0)
            .map_err(|e| failure(&format!("could not create a pipe ({e})")))?;
        let read = OwnedHandle::from_raw_handle(read.0);
        let write = OwnedHandle::from_raw_handle(write.0);
        let (child, parent) = if child_is_read_end {
            (read, write)
        } else {
            (write, read)
        };
        SetHandleInformation(raw(&parent), HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0))
            .map_err(|e| failure(&format!("could not detach the pipe ({e})")))?;
        Ok(Pipe { parent, child })
    }
}

/// An initialised attribute list, deleted when dropped.
///
/// Every early return in `spawn` after initialisation used to have to
/// remember `DeleteProcThreadAttributeList`; one owner means none can forget.
/// The buffer is on the heap, so moving this does not move what the list
/// points into.
struct AttributeList {
    _buffer: Vec<u8>,
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl AttributeList {
    fn new(count: u32) -> std::io::Result<Self> {
        unsafe {
            let mut size = 0usize;
            // The first call always fails with ERROR_INSUFFICIENT_BUFFER; it
            // is how the required size is obtained.
            let _ = InitializeProcThreadAttributeList(None, count, None, &mut size);
            let mut buffer = vec![0u8; size];
            let list = LPPROC_THREAD_ATTRIBUTE_LIST(buffer.as_mut_ptr().cast());
            InitializeProcThreadAttributeList(Some(list), count, None, &mut size)
                .map_err(|e| failure(&format!("could not prepare process attributes ({e})")))?;
            Ok(AttributeList {
                _buffer: buffer,
                list,
            })
        }
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.list) }
    }
}

/// A helper created inside an AppContainer, presenting what the caller needs
/// from `std::process::Child`.
///
/// Deliberately the same surface: `doc_sandbox::run` already feeds stdin and
/// drains stdout on their own threads, because a full pipe otherwise deadlocks
/// the parent against the child. That logic is correct and platform-neutral,
/// so this exists to hand it the same pieces rather than to replace it.
pub struct Confined {
    process: OwnedHandle,
    /// Closing this kills every process in it.
    ///
    /// `TerminateProcess` ends one process. A compromised parser can create
    /// descendants — they inherit the AppContainer token, so this is not an
    /// escape, but they can outlive the deadline, keep stdout or scratch
    /// handles open and so prevent cleanup, and burn CPU and disk. A job with
    /// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` makes the tree die as a unit.
    job: OwnedHandle,
    pub stdin: Option<std::fs::File>,
    pub stdout: Option<std::fs::File>,
}

impl Confined {
    /// Create the helper inside the container.
    ///
    /// `command_line` is the whole line, already quoted, because
    /// `CreateProcessW` parses it itself and will write into the buffer it is
    /// given — hence the owned `Vec<u16>`.
    ///
    /// The child gets no inherited environment. `CreateProcessW` with a null
    /// environment block would hand it the parent's, which on this path
    /// carries whatever the user's shell had — API keys among it. The macOS
    /// side calls `env_clear` for the same reason; this is that call.
    pub fn spawn(command_line: &str, scratch: &Scratch) -> std::io::Result<Self> {
        // A seam for testing the refusal, present only in a testhooks build.
        //
        // run() refuses the document when the container cannot be entered and
        // never retries unconfined. That is a security property, and until
        // this existed it had no test: every run took the success path.
        if test_hook!("SOVATELA_CONFINE_FORCE_FAILURE").is_some() {
            return Err(failure("confinement failure forced for testing"));
        }
        // Every handle and allocation below is owned from the moment it
        // exists, so any early return releases what was made before it. The
        // third review found the job, the pipes, the null device and the
        // attribute list each leaked on some error path before CreateProcessW.
        //
        // Breakaway is not permitted: a child that could leave the job could
        // outlive it.
        let job = unsafe {
            let job = OwnedHandle::from_raw_handle(
                CreateJobObjectW(None, PCWSTR::null())
                    .map_err(|e| failure(&format!("could not create the job object ({e})")))?
                    .0,
            );
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                raw(&job),
                JobObjectExtendedLimitInformation,
                std::ptr::from_mut(&mut limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(|_| failure("could not set the job object limits"))?;
            job
        };

        let sa_inherit = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: true.into(),
        };
        let stdin = pipe(true)?;
        let stdout = pipe(false)?;

        let mut caps = SECURITY_CAPABILITIES {
            AppContainerSid: scratch.sid.0,
            Capabilities: std::ptr::null_mut(),
            // No capabilities at all. Capabilities are what let an AppContainer
            // reach the network, the user's documents, the camera. The helper
            // needs none of them: it is handed bytes on a pipe and answers on
            // another, and the scratch directory is reached by an explicit ACL
            // rather than by a capability.
            CapabilityCount: 0,
            Reserved: 0,
        };

        unsafe {
            // Two attributes: the container, and the list of handles the child
            // may inherit.
            let attributes = AttributeList::new(2)?;
            UpdateProcThreadAttribute(
                attributes.list,
                0,
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
                Some(std::ptr::from_mut(&mut caps).cast()),
                std::mem::size_of::<SECURITY_CAPABILITIES>(),
                None,
                None,
            )
            .map_err(|e| failure(&format!("could not apply the container ({e})")))?;

            // STARTF_USESTDHANDLES means all three handles are used, so a
            // null one is not "no stderr" — it is an invalid handle, and
            // CreateProcessW refuses the call. Open the null device instead,
            // which is what Stdio::null() does on the path this replaces.
            let nul_name = wide("NUL");
            let nul = OwnedHandle::from_raw_handle(
                CreateFileW(
                    PCWSTR(nul_name.as_ptr()),
                    GENERIC_WRITE.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    Some(&sa_inherit),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )
                .map_err(|e| failure(&format!("could not open the null device ({e})")))?
                .0,
            );

            // Exactly the three handles this code prepared, and nothing else.
            //
            // bInheritHandles inherits *every* inheritable handle in the
            // parent, and the parent is a multithreaded Tauri process that may
            // hold inheritable files, pipes, mappings or sockets opened
            // anywhere. An inherited handle keeps the access it already had;
            // the container does not revoke it. Clearing the flag on the two
            // parent pipe ends, which is all this used to do, says nothing
            // about handles it never touched.
            // The whitelist can be switched off for one test only, and only in
            // a testhooks build.
            //
            // Its correctness otherwise rests on reading the code. The
            // convincing evidence is a parent handle that a child can use
            // without the whitelist and cannot with it, and that requires
            // being able to run both ways.
            let whitelist = test_hook!("SOVATELA_CONFINE_NO_HANDLE_LIST").is_none();
            // Said by the code under test, so the regression can require the
            // mode it meant to exercise rather than trust that an environment
            // variable reached this process.
            eprintln!(
                "confined: handle whitelist {}",
                if whitelist { "on" } else { "OFF" }
            );
            let _ = std::io::Write::flush(&mut std::io::stderr());
            let mut inheritable = [raw(&stdin.child), raw(&stdout.child), raw(&nul)];
            if whitelist {
                UpdateProcThreadAttribute(
                    attributes.list,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    Some(inheritable.as_mut_ptr().cast()),
                    std::mem::size_of_val(&inheritable),
                    None,
                    None,
                )
                .map_err(|e| failure(&format!("could not limit inherited handles ({e})")))?;
            }

            let mut si = STARTUPINFOEXW::default();
            si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            si.lpAttributeList = attributes.list;
            si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            si.StartupInfo.hStdInput = raw(&stdin.child);
            si.StartupInfo.hStdOutput = raw(&stdout.child);
            // stderr goes nowhere, as it does on the other platforms: it
            // carries the allocator's abort message, which is noise on the
            // parent's console and says nothing the exit status does not.
            si.StartupInfo.hStdError = raw(&nul);

            let mut line = wide(command_line);
            let working_dir = scratch.path();
            let dir = wide(&working_dir.to_string_lossy());
            // A block of NUL-terminated KEY=VALUE strings, terminated by a
            // second NUL.
            //
            // Not just TEMP and TMP. A block that narrow made CreateProcessW
            // fail with ERROR_ENVVAR_NOT_FOUND (0x800700CB): a Windows process
            // cannot start without SystemRoot, which is where the loader finds
            // the system DLLs every process links. The macOS side sets
            // PATH=/usr/bin:/bin for the same reason — the point is a minimum,
            // not an emptiness.
            //
            // SystemRoot is carried over from this process rather than
            // hardcoded to C:\Windows, because Windows is not guaranteed to be
            // there. Everything else the parent holds is
            // dropped, which is the reason for building a block at all: a null
            // block would hand the child the parent's environment, API keys
            // included.
            //
            // What an AppContainer needs to start, and nothing else.
            //
            // A block carrying only SystemRoot, SystemDrive, TEMP and TMP was
            // refused with ERROR_ENVVAR_NOT_FOUND, which reads as a misleading
            // error and is a literal one: an AppContainer resolves its own
            // per-container storage under %LOCALAPPDATA%\Packages, so a block
            // without LOCALAPPDATA leaves it unable to find a variable it
            // requires. The commit before the block existed passed None here
            // and worked, which is the evidence that should have been read
            // first.
            //
            // The block still exists rather than passing None, because None
            // hands the child the parent's entire environment. The macOS side
            // calls env_clear() for the same reason.
            // Minimised empirically, not by guess.
            //
            // This carried ten variables, eight of them added on suspicion
            // while ERROR_ENVVAR_NOT_FOUND was being chased. Run 857f1b1
            // removed each group in turn on windows-latest and windows-2022:
            // SystemRoot and LOCALAPPDATA together read the scan on both, and
            // neither is enough alone. SystemRoot alone failed on both.
            // LOCALAPPDATA alone read on windows-latest and failed on
            // windows-2022 — and SystemRoot stays regardless, because one
            // image tolerating its absence is not a licence to drop what the
            // loader documents it needs.
            //
            // Without SystemRoot the loader cannot find the system DLLs; without
            // LOCALAPPDATA an AppContainer cannot resolve its own storage under
            // %LOCALAPPDATA%\Packages.
            //
            // SOVATELA_CONFINE_ENV overrides the set so the gate can re-measure
            // rather than re-argue; unset, this is what is carried. It exists
            // only in a testhooks build.
            const SHIPPED: &[&str] = &["SystemRoot", "LOCALAPPDATA"];
            let chosen: Vec<String> = match test_hook!("SOVATELA_CONFINE_ENV") {
                Some(list) if !list.to_string_lossy().trim().is_empty() => list
                    .to_string_lossy()
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect(),
                _ => SHIPPED.iter().map(|s| s.to_string()).collect(),
            };

            let mut env: Vec<u16> = Vec::new();
            let mut put = |k: &str, v: &str| {
                env.extend(wide(&format!("{k}={v}")));
            };
            let mut carried: Vec<String> = Vec::new();
            for var in &chosen {
                match std::env::var(var) {
                    Ok(value) => {
                        carried.push(var.clone());
                        put(var, &value);
                    }
                    // A process with no SystemRoot cannot start at all, so a
                    // missing one is worth a guess rather than a silent
                    // omission — and worth saying so.
                    Err(_) if var == "SystemRoot" => {
                        carried.push("SystemRoot(guessed)".into());
                        put(var, "C:\\Windows");
                    }
                    Err(_) => carried.push(format!("{var}(absent)")),
                }
            }
            eprintln!("confined: environment carries {}", carried.join(", "));
            let _ = std::io::Write::flush(&mut std::io::stderr());
            // TEMP and TMP last, naming the scratch directory rather than
            // whatever the parent had — but for an AppContainer process the
            // system replaces both with the profile's own AC\Temp. Run
            // 36199760841 measured it on both images: inside the container
            // TEMP and temp_dir() name that directory, not this one. It is
            // stable across extractions, so anything written through TEMP
            // persists between documents; the gate lists what is there. The
            // granted scratch is the working directory, set below.
            let scratch_dir = working_dir.to_string_lossy().to_string();
            put("TEMP", &scratch_dir);
            put("TMP", &scratch_dir);
            // wide() already NUL-terminates each entry; this is the block's
            // own terminating NUL.
            env.push(0);

            // Suspended, then placed in the job, then resumed.
            //
            // It used to be created running and assigned afterwards, so for a
            // moment the helper ran outside its job and anything it started in
            // that window would never be collected — the third review's
            // finding against condition 5. A suspended process has run nothing,
            // not even its loader, until the job holds it.
            let mut pi = PROCESS_INFORMATION::default();
            CreateProcessW(
                None,
                Some(PWSTR(line.as_mut_ptr())),
                None,
                None,
                // Inheritance is on so the prepared handles reach the child;
                // the attribute list above limits it to exactly those.
                true,
                EXTENDED_STARTUPINFO_PRESENT
                    | CREATE_NO_WINDOW
                    | CREATE_UNICODE_ENVIRONMENT
                    | CREATE_SUSPENDED,
                Some(env.as_mut_ptr().cast()),
                PCWSTR(dir.as_ptr()),
                &si.StartupInfo,
                &mut pi,
            )
            .map_err(|e| {
                failure(&format!(
                    "could not start the document reader in a container ({e})"
                ))
            })?;
            let process = OwnedHandle::from_raw_handle(pi.hProcess.0);
            let thread = OwnedHandle::from_raw_handle(pi.hThread.0);
            if AssignProcessToJobObject(raw(&job), raw(&process)).is_err() {
                // Fail closed: a helper outside the job is one whose
                // descendants nothing will collect. It has not run yet.
                let _ = TerminateProcess(raw(&process), 1);
                return Err(failure("could not place the document reader in its job"));
            }
            if ResumeThread(raw(&thread)) == u32::MAX {
                let _ = TerminateJobObject(raw(&job), 1);
                return Err(failure("could not start the document reader's thread"));
            }
            drop(thread);

            // The child's pipe ends and the null device close here, when this
            // returns: the parent must not hold them, or it never sees EOF.
            Ok(Confined {
                process,
                job,
                stdin: Some(std::fs::File::from(stdin.parent)),
                stdout: Some(std::fs::File::from(stdout.parent)),
            })
        }
    }

    /// Kill the helper and everything it started, and wait until they are
    /// gone.
    ///
    /// Terminating the job takes the descendants with it; terminating only the
    /// process would leave them holding pipes and scratch. Termination is
    /// asynchronous, so returning straight after it — which this did — let
    /// the scratch guard race a dying process. The third review found it.
    ///
    /// Waiting for the job's active-process count to reach zero is not
    /// waiting for termination. Run 36199760841 measured it on both images:
    /// the count was zero at once, while a descendant holding a file in
    /// scratch was still alive. So this takes a handle to every process the
    /// job holds before terminating it, and waits on those.
    ///
    /// What that does not cover, as the fourth review put it: a process
    /// created after the listing and before the termination is still killed —
    /// termination targets the whole job, which nothing can leave — but is not
    /// waited on through a handle. A process ID reused between the listing and
    /// `OpenProcess` costs at most a bounded extra wait on an unrelated
    /// process; it cannot get that process killed. Above 256 processes the
    /// list is not read and only the job's count is waited on. So this bounds
    /// cleanup; it does not prove every descendant has exited. If that ever
    /// has to be proved, `JOB_OBJECT_LIMIT_ACTIVE_PROCESS = 1` would, provided
    /// the helper never needs a child of its own.
    pub fn kill(&self) {
        let members = self.member_handles();
        unsafe {
            let _ = TerminateJobObject(raw(&self.job), 1);
        }
        let deadline = std::time::Instant::now() + KILL_GRACE;
        let mut dead = true;
        for h in members.iter().chain(std::iter::once(&self.process)) {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let ms = left.as_millis().min(u128::from(u32::MAX)) as u32;
            if unsafe { WaitForSingleObject(raw(h), ms) }.0 != 0 {
                dead = false;
            }
        }
        if !dead || !self.wait_until_empty(KILL_GRACE) {
            eprintln!("confined: processes in the job outlived termination");
        }
    }

    /// A handle to every process in the job, for waiting on.
    ///
    /// Empty if the job could not be asked or holds more than the list has
    /// room for; `kill` still waits on the helper and the job's count then.
    fn member_handles(&self) -> Vec<OwnedHandle> {
        use windows::Win32::System::JobObjects::JobObjectBasicProcessIdList;
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
        // JOBOBJECT_BASIC_PROCESS_ID_LIST with room for its list: two counts
        // and the identifiers, laid out as the header declares them.
        #[repr(C)]
        struct Ids {
            assigned: u32,
            listed: u32,
            ids: [usize; 256],
        }
        let mut list = Ids {
            assigned: 0,
            listed: 0,
            ids: [0; 256],
        };
        let asked = unsafe {
            QueryInformationJobObject(
                Some(raw(&self.job)),
                JobObjectBasicProcessIdList,
                std::ptr::from_mut(&mut list).cast(),
                std::mem::size_of::<Ids>() as u32,
                None,
            )
        };
        if asked.is_err() {
            return Vec::new();
        }
        let listed = (list.listed as usize).min(list.ids.len());
        list.ids[..listed]
            .iter()
            .filter_map(|&pid| unsafe {
                OpenProcess(PROCESS_SYNCHRONIZE, false, pid as u32)
                    .ok()
                    .map(|h| OwnedHandle::from_raw_handle(h.0))
            })
            .collect()
    }

    /// How many processes the job holds, or `None` if it could not be asked.
    fn active_processes(&self) -> Option<u32> {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(raw(&self.job)),
                JobObjectBasicAccountingInformation,
                std::ptr::from_mut(&mut info).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                None,
            )
            .ok()?;
        }
        Some(info.ActiveProcesses)
    }

    /// Wait, within a bound, for the job to hold no processes.
    fn wait_until_empty(&self, limit: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + limit;
        loop {
            match self.active_processes() {
                Some(0) => return true,
                None => return false,
                Some(_) => {}
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The exit code, or `None` while it is still running.
    pub fn try_exit_code(&self) -> Option<u32> {
        const STILL_ACTIVE: u32 = 259;
        let mut code = 0u32;
        unsafe {
            if GetExitCodeProcess(raw(&self.process), &mut code).is_err() {
                return Some(1);
            }
        }
        (code != STILL_ACTIVE).then_some(code)
    }

    pub fn wait(&self, millis: u32) -> bool {
        unsafe { WaitForSingleObject(raw(&self.process), millis).0 == 0 }
    }
}

/// How long `kill` waits for the job to empty before giving up on it.
const KILL_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

impl Drop for Confined {
    fn drop(&mut self) {
        // However the caller got here — the helper exited, timed out, or the
        // caller returned early — anything still in the job is killed and
        // waited for before the handles close. The helper exiting does not
        // mean its descendants did, and the scratch guard is dropped after
        // this.
        self.kill();
    }
}

/// The helper as `doc_sandbox::run` needs to hold it.
///
/// `run` was written against `std::process::Child` and its loop — try_wait,
/// kill, wait, take the two pipes — is correct and platform-neutral. Rather
/// than fork that loop per platform, this presents the same shape over either
/// kind of child, so the only Windows-specific thing in `run` is which one it
/// constructs.
pub enum Helper {
    Confined(Confined),
    Plain(std::process::Child),
}

impl Helper {
    pub fn take_stdin(&mut self) -> Option<Box<dyn std::io::Write + Send>> {
        match self {
            Helper::Confined(c) => c
                .stdin
                .take()
                .map(|f| Box::new(f) as Box<dyn std::io::Write + Send>),
            Helper::Plain(c) => c
                .stdin
                .take()
                .map(|f| Box::new(f) as Box<dyn std::io::Write + Send>),
        }
    }

    pub fn take_stdout(&mut self) -> Option<Box<dyn std::io::Read + Send>> {
        match self {
            Helper::Confined(c) => c
                .stdout
                .take()
                .map(|f| Box::new(f) as Box<dyn std::io::Read + Send>),
            Helper::Plain(c) => c
                .stdout
                .take()
                .map(|f| Box::new(f) as Box<dyn std::io::Read + Send>),
        }
    }

    /// `Ok(Some(code))` once it has exited. Matches `Child::try_wait`'s shape,
    /// but yields the code directly: `classify` takes `Option<i32>`, so an
    /// `ExitStatus` would only be unwrapped again.
    pub fn try_exit(&mut self) -> std::io::Result<Option<i32>> {
        match self {
            Helper::Confined(c) => Ok(c.try_exit_code().map(|c| c as i32)),
            Helper::Plain(c) => match c.try_wait() {
                Ok(Some(s)) => Ok(Some(s.code().unwrap_or(-1))),
                Ok(None) => Ok(None),
                Err(e) => Err(e),
            },
        }
    }

    pub fn kill(&mut self) {
        match self {
            Helper::Confined(c) => c.kill(),
            Helper::Plain(c) => {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
}

/// The validation entry points: the self-test and the probes the gate runs.
///
/// Compiled only with `windows-confinement-testhooks`. They are what the gate
/// measures confinement with, and nothing the application uses; a build that
/// could ship carries none of them, and `main` exits 64 on their flags.
#[cfg(feature = "windows-confinement-testhooks")]
pub mod validation {
    use super::*;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ};

    /// The flag that runs one document through a confined helper and prints what
    /// comes back.
    ///
    /// This exists for `windows-confinement-validate.yml` and nothing else. The
    /// module cannot be exercised from a unit test — it creates a real process in
    /// a real container — and it is not wired into the application yet, so
    /// without an entry point there is no way to find out whether any of it
    /// works. A self-test that a CI job can run is the smallest thing that turns
    /// "type-checks" into "observed".
    pub const SELFTEST_FLAG: &str = "--sovatela-confined-helper-selftest";

    /// Returns true if it handled the invocation.
    ///
    /// Goes through `doc_sandbox::extract_text` — the parent entry point the
    /// application calls — rather than `Confined::spawn` directly.
    ///
    /// The first version called spawn, and the CI steps built on it were vacuous:
    /// they exercised the container and skipped the scratch directory, its ACL,
    /// the IO threads and the wait loop. The fixture harnesses have the same gap
    /// for a different reason — `check-mixed.mjs` invokes
    /// `--sovatela-extract-doc-helper` directly, so it tests the child and never
    /// the parent that confines it. Nothing in this repository called
    /// `extract_text` before this did.
    pub fn run_selftest_if_requested() -> bool {
        let mut args = std::env::args();
        let Some(flag) = args.nth(1) else {
            return false;
        };
        if flag != SELFTEST_FLAG {
            return false;
        }
        macro_rules! mark {
            ($($a:tt)*) => {{
                eprintln!($($a)*);
                let _ = std::io::Write::flush(&mut std::io::stderr());
            }};
        }
        mark!("selftest: entered, going through doc_sandbox::extract_text");

        use std::io::{Read, Write};
        let mut input = Vec::new();
        if let Err(e) = std::io::stdin().read_to_end(&mut input) {
            mark!("selftest: could not read stdin: {e}");
            std::process::exit(2);
        }
        mark!("selftest: read {} bytes", input.len());

        // The kind comes from argv, as it does for the helper flag, and every
        // kind the helper reads is accepted. This took only `pdf` until the
        // third review pointed out that enabling confinement changes DOCX,
        // ODT, PPTX and XLSX too, and none of them had been read confined.
        let token = args.next().unwrap_or_else(|| "pdf".to_string());
        let Some(kind) = crate::doc_sandbox::Kind::from_token(&token) else {
            mark!("selftest: unsupported kind {token}");
            std::process::exit(2);
        };
        match crate::doc_sandbox::extract_text(kind, &input) {
            Ok(text) => {
                mark!("selftest: extracted {} bytes", text.len());
                let mut out = std::io::stdout();
                let _ = out.write_all(crate::doc_sandbox::REPLY_MAGIC);
                let _ = out.write_all(text.as_bytes());
                let _ = out.flush();
                std::process::exit(0);
            }
            Err(message) => {
                mark!("selftest: refused: {message}");
                std::process::exit(33);
            }
        }
    }

    /// A probe that reports what it is and precisely what happened when it tried
    /// to reach things.
    ///
    /// The first version answered in booleans, and a boolean cannot tell denial
    /// from absence: a wrong path produced the same three `false` values as a
    /// working container and passed. It also reused the file the control had just
    /// created, so its "create" case was an overwrite, and it took the runner's
    /// temp root as scratch — which on Actions is the *parent* of the canary, so
    /// the boundary it claimed to test was not the boundary at all.
    ///
    /// Every result now carries the OS error, so the workflow can require
    /// `ERROR_ACCESS_DENIED` rather than merely "not success".
    ///
    /// Arguments: a file to read, a file to overwrite, a path to create, and a
    /// loopback port.
    pub const PROBE_FLAG: &str = "--sovatela-confinement-probe";

    /// `ok`, or the raw OS error and kind. A caller asserting `denied:5` cannot be
    /// satisfied by a missing file, a bad path or a sharing violation.
    fn outcome(result: std::io::Result<()>) -> String {
        match result {
            Ok(()) => "ok".to_string(),
            Err(e) => match e.raw_os_error() {
                Some(5) => "denied:5".to_string(),
                Some(code) => format!("err:{code}"),
                None => format!("err:{:?}", e.kind()),
            },
        }
    }

    pub fn run_probe_if_requested() -> bool {
        let mut args = std::env::args();
        let Some(flag) = args.nth(1) else {
            return false;
        };
        if flag != PROBE_FLAG {
            return false;
        }
        let read_target = args.next().unwrap_or_default();
        let overwrite_target = args.next().unwrap_or_default();
        let create_target = args.next().unwrap_or_default();
        let port: u16 = args.next().and_then(|p| p.parse().ok()).unwrap_or(0);

        match is_app_container() {
            Ok(v) => println!("probe: appcontainer=ok:{v}"),
            // A failed query is not "false". Reporting it as one let a broken
            // token check pass as a well-behaved control.
            Err(code) => println!("probe: appcontainer=err:{code}"),
        }

        println!(
            "probe: read={}",
            outcome(std::fs::read(&read_target).map(|_| ()))
        );
        println!(
            "probe: overwrite={}",
            outcome(std::fs::write(&overwrite_target, b"overwritten"))
        );
        println!(
            "probe: create={}",
            outcome(std::fs::write(&create_target, b"created"))
        );

        // Scratch must stay writable, or "denied everything" would be
        // indistinguishable from a container that cannot work at all.
        //
        // The working directory, which spawn sets to the granted scratch —
        // not temp_dir(). Inside an AppContainer, GetTempPath may name the
        // container's own profile temp rather than TEMP, so this check could
        // have been measuring a directory the application never granted.
        let scratch = std::env::current_dir()
            .unwrap_or_default()
            .join("probe-scratch-write");
        println!("probe: temp-dir={}", std::env::temp_dir().display());
        println!(
            "probe: scratch={}",
            outcome(std::fs::write(&scratch, b"ok"))
        );

        if port != 0 {
            let addr: std::net::SocketAddr = format!("127.0.0.1:{port}")
                .parse()
                .expect("loopback address");
            let r = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5))
                .map(|_| ());
            // 10013 is WSAEACCES: refused by policy. A timeout or a connection
            // refusal would mean the listener was gone, not that the container
            // blocked it, so those must not read as denial.
            println!(
                "probe: loopback={}",
                match r {
                    Ok(()) => "ok".to_string(),
                    Err(e) => match e.raw_os_error() {
                        Some(10013) => "denied:10013".to_string(),
                        Some(code) => format!("err:{code}"),
                        None => format!("err:{:?}", e.kind()),
                    },
                }
            );
        }
        true
    }

    /// Whether this process holds an AppContainer token, or why the question could
    /// not be answered.
    fn is_app_container() -> Result<bool, u32> {
        use windows::Win32::Security::{GetTokenInformation, TokenIsAppContainer, TOKEN_QUERY};
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        unsafe {
            let mut token = windows::Win32::Foundation::HANDLE::default();
            if let Err(e) = OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) {
                return Err(e.code().0 as u32);
            }
            let mut value: u32 = 0;
            let mut size: u32 = 0;
            let queried = GetTokenInformation(
                token,
                TokenIsAppContainer,
                Some(std::ptr::from_mut(&mut value).cast()),
                std::mem::size_of::<u32>() as u32,
                &mut size,
            );
            let _ = CloseHandle(token);
            match queried {
                Ok(()) => Ok(value != 0),
                Err(e) => Err(e.code().0 as u32),
            }
        }
    }

    /// Run the probe inside a container and relay what it says.
    ///
    /// Takes its own scratch directory and grants the container access to it, as
    /// `doc_sandbox::run` does. The first version passed `std::env::temp_dir()`
    /// and never called `grant_container_access`, so a successful scratch write
    /// showed ambient access to the runner's temp root rather than access the
    /// application had granted — and, because the canary lived under that same
    /// root, the denial it observed was not an outside-scratch denial at all.
    pub const CONFINED_PROBE_FLAG: &str = "--sovatela-confined-probe";

    pub fn run_confined_probe_if_requested() -> bool {
        let mut args = std::env::args();
        let Some(flag) = args.nth(1) else {
            return false;
        };
        if flag != CONFINED_PROBE_FLAG {
            return false;
        }
        let scratch = std::path::PathBuf::from(args.next().unwrap_or_default());
        let rest: Vec<String> = args.collect();
        if rest.len() != 4 {
            eprintln!("confined probe: expected scratch, read, overwrite, create, port");
            std::process::exit(2);
        }

        let scratch = match Scratch::new(scratch) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("confined probe: could not prepare scratch: {e}");
                std::process::exit(3);
            }
        };

        let exe = std::env::current_exe().expect("current exe");
        let line = format!(
            "\"{}\" {} \"{}\" \"{}\" \"{}\" {}",
            exe.display(),
            PROBE_FLAG,
            rest[0],
            rest[1],
            rest[2],
            rest[3]
        );

        let mut child = match Confined::spawn(&line, &scratch) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("confined probe: could not start: {e}");
                std::process::exit(4);
            }
        };
        drop(child.stdin.take());

        // Wait first, then read.
        //
        // The previous version read stdout to EOF and only then checked its
        // thirty-second bound, so a child holding the pipe open made the bound
        // unreachable — the same ordering fault this project has now hit three
        // times, twice in my own harnesses. The probe's output is a handful of
        // lines and cannot fill a pipe, so waiting first is safe here and the
        // deadline stays enforceable.
        let finished = child.wait(30_000);
        if !finished {
            child.kill();
            eprintln!("confined probe: did not exit within 30s");
            std::process::exit(5);
        }
        let mut out = String::new();
        if let Some(mut pipe) = child.stdout.take() {
            use std::io::Read;
            let _ = pipe.read_to_string(&mut out);
        }
        print!("{out}");
        let code = child.try_exit_code().unwrap_or(1) as i32;
        // exit() runs no destructors, and the scratch must outlive the child.
        drop(child);
        drop(scratch);
        std::process::exit(code);
    }

    /// Can this process use a handle its parent left inheritable?
    ///
    /// The child half of the whitelist regression; the parent half is
    /// `CONFINED_HANDLE_PROBE_FLAG`. It reports its own token first, so a
    /// "usable" answer cannot have come from a child that was never in a
    /// container, then what reading through the given handle number produced.
    ///
    /// An unusable handle does not necessarily come back as an error. The first
    /// run of this regression, on both images, ended the whitelisted child with
    /// `STATUS_INVALID_HANDLE` (0xC0000008) as an unhandled exception rather than
    /// `ERROR_INVALID_HANDLE` from the read — the behaviour of strict handle
    /// checking, under which a bad handle reference raises instead of failing.
    /// So the probe reports that policy as measured, and marks the moment before
    /// the read, so the workflow can accept that exit only when the policy that
    /// explains it is on and the read is what raised it.
    pub const HANDLE_PROBE_FLAG: &str = "--sovatela-inherited-handle-probe";

    /// Whether this process raises on an invalid handle reference, or why the
    /// question could not be answered.
    ///
    /// Read as the policy's flags word — `RaiseExceptionOnInvalidHandleReference`
    /// is bit 0 — rather than through its struct, which lives behind a crate
    /// feature the application does not otherwise need.
    fn strict_handle_checks() -> Result<bool, u32> {
        use windows::Win32::System::Threading::{
            GetCurrentProcess, GetProcessMitigationPolicy, ProcessStrictHandleCheckPolicy,
        };
        let mut flags: u32 = 0;
        unsafe {
            GetProcessMitigationPolicy(
                GetCurrentProcess(),
                ProcessStrictHandleCheckPolicy,
                std::ptr::from_mut(&mut flags).cast(),
                std::mem::size_of::<u32>(),
            )
        }
        .map(|()| flags & 1 != 0)
        .map_err(|e| e.code().0 as u32)
    }

    pub fn run_handle_probe_if_requested() -> bool {
        let mut args = std::env::args();
        let Some(flag) = args.nth(1) else {
            return false;
        };
        if flag != HANDLE_PROBE_FLAG {
            return false;
        }
        match is_app_container() {
            Ok(v) => println!("handle: appcontainer=ok:{v}"),
            Err(code) => println!("handle: appcontainer=err:{code}"),
        }
        match strict_handle_checks() {
            Ok(v) => println!("handle: strict-handle-checks=ok:{v}"),
            Err(code) => println!("handle: strict-handle-checks=err:{code}"),
        }
        let raw: usize = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        if raw == 0 {
            println!("handle: no-handle-given");
            return true;
        }
        use std::io::Read;
        use std::os::windows::io::FromRawHandle;
        let mut file = unsafe { std::fs::File::from_raw_handle(raw as *mut std::ffi::c_void) };
        let mut buf = String::new();
        // The read is the only use of the lent handle after this line, so an
        // exception after it and before a result line is the read's.
        println!("handle: reading {raw}");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        match file.read_to_string(&mut buf) {
            Ok(_) => println!("handle: usable:{}", buf.trim()),
            Err(e) => println!("handle: unusable:{}", e.raw_os_error().unwrap_or(-1)),
        }
        // Do not close it: the handle belongs to the parent's lifetime, and
        // dropping the File would close a descriptor this process was only lent.
        std::mem::forget(file);
        true
    }

    /// The parent half of the whitelist regression.
    ///
    /// Opens `file` with an inheritable handle, starts the handle probe inside a
    /// container through `Confined::spawn` with that handle's number, and relays
    /// what it says. `SOVATELA_CONFINE_NO_HANDLE_LIST` decides which way spawn
    /// runs, and spawn says which on stderr.
    ///
    /// The workflow runs it both ways and requires opposite answers. Without the
    /// whitelist the child must read the file's contents — which also shows the
    /// container does not revoke an inherited handle, since the file sits where
    /// the container cannot open it by path. With the whitelist it must not. A
    /// one-way test would pass on a handle unusable for any unrelated reason.
    ///
    /// Arguments: a scratch directory, and the file to open.
    pub const CONFINED_HANDLE_PROBE_FLAG: &str = "--sovatela-confined-handle-probe";

    pub fn run_confined_handle_probe_if_requested() -> bool {
        let mut args = std::env::args();
        let Some(flag) = args.nth(1) else {
            return false;
        };
        if flag != CONFINED_HANDLE_PROBE_FLAG {
            return false;
        }
        let (Some(scratch), Some(target)) = (args.next(), args.next()) else {
            eprintln!("confined handle probe: expected scratch, file");
            std::process::exit(2);
        };
        let scratch = match Scratch::new(std::path::PathBuf::from(scratch)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("confined handle probe: could not prepare scratch: {e}");
                std::process::exit(3);
            }
        };

        let sa_inherit = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: true.into(),
        };
        let name = wide(&target);
        let handle = match unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_READ,
                Some(&sa_inherit),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        } {
            Ok(h) => h,
            Err(e) => {
                eprintln!("confined handle probe: could not open {target}: {e}");
                std::process::exit(3);
            }
        };
        let number = handle.0 as usize;
        println!("handle-parent: passing {number}");

        let exe = std::env::current_exe().expect("current exe");
        let line = format!("\"{}\" {} {}", exe.display(), HANDLE_PROBE_FLAG, number);
        let mut child = match Confined::spawn(&line, &scratch) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("confined handle probe: could not start: {e}");
                std::process::exit(4);
            }
        };
        drop(child.stdin.take());

        // Wait first, then read: the output is two lines and cannot fill a pipe.
        let code = if child.wait(30_000) {
            let mut out = String::new();
            if let Some(mut pipe) = child.stdout.take() {
                use std::io::Read;
                let _ = pipe.read_to_string(&mut out);
            }
            print!("{out}");
            child.try_exit_code().unwrap_or(1) as i32
        } else {
            child.kill();
            eprintln!("confined handle probe: did not exit within 30s");
            5
        };
        // The child had its copy, if it was given one, from the moment it was
        // created; the parent's can go now.
        unsafe {
            let _ = CloseHandle(handle);
        }
        // exit() runs no destructors, and the scratch must outlive the child.
        drop(child);
        drop(scratch);
        std::process::exit(code);
    }

    /// Read lines from a pipe on a thread, so a child that never exits cannot
    /// block the caller: its stdout never reaches EOF.
    fn line_reader(pipe: Option<std::fs::File>) -> std::sync::mpsc::Receiver<String> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::BufRead;
            if let Some(pipe) = pipe {
                for line in std::io::BufReader::new(pipe).lines() {
                    let Ok(line) = line else { break };
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            }
        });
        rx
    }

    fn system32(program: &str) -> std::path::PathBuf {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        std::path::Path::new(&root).join("System32").join(program)
    }

    /// The confined half of the descendant test: start a grandchild that holds
    /// a file open in scratch, report it, then wedge.
    ///
    /// The grandchild is `cmd /c pause > held.txt` with a stdin pipe this
    /// process keeps and never writes, so it blocks for ever with the
    /// redirection target open — the case the job object and the scratch
    /// guard exist for. `cmd` rather than this binary, because an image the
    /// container can execute is needed and System32 is one.
    pub const DESCENDANT_CHILD_FLAG: &str = "--sovatela-descendant-child";

    pub fn run_descendant_child_if_requested() -> bool {
        use std::io::Write;
        use std::os::windows::process::CommandExt;
        let mut args = std::env::args();
        let Some(flag) = args.nth(1) else {
            return false;
        };
        if flag != DESCENDANT_CHILD_FLAG {
            return false;
        }
        // The working directory is the granted scratch; temp_dir() inside a
        // container may not be. stdout and stderr are inherited rather than
        // opened on NUL: the first run of this step could not start the
        // grandchild at all, and a null device the container may not open is
        // one candidate the execute probe measures.
        let held = std::env::current_dir().unwrap_or_default().join("held.txt");
        let spawned = std::process::Command::new(system32("cmd.exe"))
            .arg("/d")
            .arg("/c")
            .raw_arg(format!("pause > \"{}\"", held.display()))
            .stdin(std::process::Stdio::piped())
            .spawn();
        let grandchild = match spawned {
            Ok(c) => c,
            Err(e) => {
                println!("descendant: spawn=err:{}", e.raw_os_error().unwrap_or(-1));
                let _ = std::io::stdout().flush();
                std::process::exit(6);
            }
        };
        println!("descendant: pid={}", grandchild.id());
        let _ = std::io::stdout().flush();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !held.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        println!("descendant: holding={}", held.exists());
        let _ = std::io::stdout().flush();
        // Wedged, as a compromised or stuck helper would be, keeping the
        // grandchild's stdin open so it never finishes either.
        let _keep = grandchild;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }

    /// The parent half: does `kill` take the descendant with it, and is the
    /// scratch removed afterwards?
    ///
    /// Arguments: a scratch directory, and `kill` or `no-kill`. The gate runs
    /// both. Before anything is killed it establishes that the grandchild is
    /// alive, is in the helper's job, and is holding a file that cannot be
    /// deleted — without those, "dead afterwards" and "removed afterwards"
    /// would prove nothing. `no-kill` measures the same checkpoint without
    /// calling `kill`, and must find the grandchild alive: the step's two
    /// directions, as in the handle regression.
    pub const CONFINED_DESCENDANT_FLAG: &str = "--sovatela-confined-descendant-probe";

    pub fn run_confined_descendant_probe_if_requested() -> bool {
        use windows::Win32::System::JobObjects::IsProcessInJob;
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };
        let mut args = std::env::args();
        let Some(flag) = args.nth(1) else {
            return false;
        };
        if flag != CONFINED_DESCENDANT_FLAG {
            return false;
        }
        let (Some(path), Some(mode)) = (args.next(), args.next()) else {
            eprintln!("confined descendant probe: expected scratch, kill|no-kill");
            std::process::exit(2);
        };
        let kill = match mode.as_str() {
            "kill" => true,
            "no-kill" => false,
            _ => std::process::exit(2),
        };
        let path = std::path::PathBuf::from(path);
        let scratch = match Scratch::new(path.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("confined descendant probe: could not prepare scratch: {e}");
                std::process::exit(3);
            }
        };
        let exe = std::env::current_exe().expect("current exe");
        let line = format!("\"{}\" {}", exe.display(), DESCENDANT_CHILD_FLAG);
        let mut child = match Confined::spawn(&line, &scratch) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("confined descendant probe: could not start: {e}");
                std::process::exit(4);
            }
        };
        drop(child.stdin.take());
        let rx = line_reader(child.stdout.take());

        let mut pid = None;
        let mut holding = None;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while (pid.is_none() || holding.is_none()) && std::time::Instant::now() < deadline {
            match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(l) => {
                    println!("{l}");
                    if let Some(v) = l.strip_prefix("descendant: pid=") {
                        pid = v.trim().parse::<u32>().ok();
                    }
                    if let Some(v) = l.strip_prefix("descendant: holding=") {
                        holding = Some(v.trim() == "true");
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
        }
        let Some(pid) = pid else {
            println!("descendant: no-pid");
            std::process::exit(6);
        };
        let grandchild = match unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
        } {
            Ok(h) => unsafe { OwnedHandle::from_raw_handle(h.0) },
            Err(e) => {
                println!("descendant: open=err:{}", e.code().0);
                std::process::exit(6);
            }
        };
        const WAIT_TIMEOUT: u32 = 258;
        let alive = |h: &OwnedHandle| unsafe { WaitForSingleObject(raw(h), 0).0 == WAIT_TIMEOUT };
        let mut in_job = windows::core::BOOL(0);
        let asked = unsafe { IsProcessInJob(raw(&grandchild), Some(raw(&child.job)), &mut in_job) };
        println!(
            "descendant: in-job={}",
            if asked.is_ok() {
                in_job.as_bool().to_string()
            } else {
                "err".into()
            }
        );
        println!("descendant: alive-before={}", alive(&grandchild));
        // Deleting the held file must fail now, or its removal later says
        // nothing about a descendant that held it.
        let held = scratch.path().join("held.txt");
        println!(
            "descendant: removable-before={}",
            std::fs::remove_file(&held).is_ok()
        );

        let started = std::time::Instant::now();
        if kill {
            child.kill();
        } else {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        println!("descendant: dead-after={}", !alive(&grandchild));
        println!(
            "descendant: job-empty-after={}",
            child.active_processes() == Some(0)
        );
        println!("descendant: kill-ms={}", started.elapsed().as_millis());
        // However it was left, dropping the helper kills what remains and
        // dropping the scratch removes it: the application's order.
        drop(child);
        drop(scratch);
        println!("descendant: scratch-removed={}", !path.exists());
        std::process::exit(0);
    }

    /// One access control entry, as compared: type, flags, mask, SID.
    #[derive(Clone, PartialEq, Eq)]
    struct Entry {
        kind: u8,
        flags: u8,
        mask: u32,
        sid: String,
    }

    impl std::fmt::Display for Entry {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "type={} flags=0x{:02x} mask=0x{:08x} sid={}",
                self.kind, self.flags, self.mask, self.sid
            )
        }
    }

    fn sid_string(sid: PSID) -> String {
        use windows::Win32::Foundation::{LocalFree, HLOCAL};
        use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
        let mut text = PWSTR::null();
        unsafe {
            if ConvertSidToStringSidW(sid, &mut text).is_err() {
                return "unconvertible".into();
            }
            let s = text.to_string().unwrap_or_default();
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            s
        }
    }

    /// The DACL of a file or directory, entry by entry.
    fn dacl_of(path: &std::path::Path) -> Result<Vec<Entry>, String> {
        use windows::Win32::Foundation::{LocalFree, HLOCAL};
        use windows::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
        use windows::Win32::Security::{
            AclSizeInformation, GetAce, GetAclInformation, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL,
            ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        };
        let name = wide(&path.to_string_lossy());
        let mut acl: *mut ACL = std::ptr::null_mut();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            if GetNamedSecurityInfoW(
                PCWSTR(name.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut acl),
                None,
                &mut descriptor,
            )
            .is_err()
            {
                return Err("could not read the DACL".into());
            }
            let mut entries = Vec::new();
            if !acl.is_null() {
                let mut info = ACL_SIZE_INFORMATION::default();
                if GetAclInformation(
                    acl,
                    std::ptr::from_mut(&mut info).cast(),
                    std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                    AclSizeInformation,
                )
                .is_ok()
                {
                    for i in 0..info.AceCount {
                        let mut ace: *mut std::ffi::c_void = std::ptr::null_mut();
                        if GetAce(acl, i, &mut ace).is_err() {
                            continue;
                        }
                        let header = &*(ace as *const ACE_HEADER);
                        // Allowed (0) and denied (1) entries share this
                        // layout; anything else is recorded by type only.
                        let (mask, sid) = if header.AceType <= 1 {
                            let a = &*(ace as *const ACCESS_ALLOWED_ACE);
                            (
                                a.Mask,
                                sid_string(PSID(std::ptr::addr_of!(a.SidStart) as *mut _)),
                            )
                        } else {
                            (0, format!("(type {})", header.AceType))
                        };
                        entries.push(Entry {
                            kind: header.AceType,
                            flags: header.AceFlags,
                            mask,
                            sid,
                        });
                    }
                }
            }
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            Ok(entries)
        }
    }

    /// Does the grant merge into the directory's DACL, and what does it give?
    ///
    /// Reads the DACL of a fresh directory, grants through the application's
    /// own function, and reads it again, along with a file created inside
    /// afterwards. Reports whether every entry that was there before is still
    /// there, what the container can do to the directory itself, and whether
    /// what files inherit carries execute.
    ///
    /// The merge check is also run against the container's entries alone —
    /// what the old `OldAcl = None` code produced — and must reject it, so the
    /// check is seen to be able to fail.
    pub const ACL_PROBE_FLAG: &str = "--sovatela-scratch-acl-probe";

    pub fn run_acl_probe_if_requested() -> bool {
        use windows::Win32::Storage::FileSystem::{
            DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_TRAVERSE,
        };
        let mut args = std::env::args();
        let Some(flag) = args.nth(1) else {
            return false;
        };
        if flag != ACL_PROBE_FLAG {
            return false;
        }
        let Some(dir) = args.next().map(std::path::PathBuf::from) else {
            std::process::exit(2);
        };
        if let Err(e) = std::fs::create_dir(&dir) {
            eprintln!("acl probe: could not create {}: {e}", dir.display());
            std::process::exit(3);
        }
        let result = (|| -> Result<(), String> {
            let before = dacl_of(&dir)?;
            let sid = ContainerSid::derive().map_err(|e| e.to_string())?;
            let container = sid_string(sid.0);
            grant_container_access(&dir, &sid).map_err(|e| e.to_string())?;
            let after = dacl_of(&dir)?;
            let file = dir.join("inherits.txt");
            std::fs::write(&file, b"x").map_err(|e| e.to_string())?;
            let on_file = dacl_of(&file)?;

            for e in &before {
                println!("acl: before {e}");
            }
            for e in &after {
                println!("acl: after {e}");
            }
            for e in &on_file {
                println!("acl: file {e}");
            }
            let mine = |e: &&Entry| e.sid == container && e.kind == 0;
            let contains_all =
                |outer: &[Entry], inner: &[Entry]| inner.iter().all(|e| outer.contains(e));
            let only_mine: Vec<Entry> = after.iter().filter(mine).cloned().collect();

            println!("acl: before-count={}", before.len());
            println!("acl: preserved={}", contains_all(&after, &before));
            println!(
                "acl: replacing-rejected={}",
                !contains_all(&only_mine, &before)
            );
            // What the container holds on the directory itself: entries that
            // are not inherit-only (0x08).
            let on_dir: u32 = after
                .iter()
                .filter(mine)
                .filter(|e| e.flags & 0x08 == 0)
                .fold(0, |m, e| m | e.mask);
            let wanted = FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0 | DELETE.0 | FILE_TRAVERSE.0;
            println!("acl: dir-container-mask=0x{on_dir:08x}");
            println!("acl: dir-container-complete={}", on_dir & wanted == wanted);
            // Execute on anything that files inherit (object-inherit, 0x01).
            let inheritable_execute = after
                .iter()
                .filter(mine)
                .any(|e| e.flags & 0x01 != 0 && e.mask & FILE_TRAVERSE.0 != 0);
            println!("acl: inheritable-execute={inheritable_execute}");
            let file_mask: u32 = on_file.iter().filter(mine).fold(0, |m, e| m | e.mask);
            println!(
                "acl: file-container-present={}",
                on_file.iter().any(|e| e.sid == container)
            );
            println!("acl: file-container-mask=0x{file_mask:08x}");
            println!(
                "acl: file-container-execute={}",
                file_mask & FILE_TRAVERSE.0 != 0
            );
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(e) = result {
            println!("acl: error={e}");
            std::process::exit(5);
        }
        std::process::exit(0);
    }

    /// The confined half of the execute test: copy a system program into
    /// scratch and try to run it from there.
    pub const EXEC_CHILD_FLAG: &str = "--sovatela-scratch-exec-child";

    fn run_outcome(r: std::io::Result<std::process::ExitStatus>) -> String {
        match r {
            Ok(s) => format!("ok:{}", s.code().unwrap_or(-1)),
            Err(e) => match e.raw_os_error() {
                Some(5) => "denied:5".into(),
                Some(c) => format!("err:{c}"),
                None => format!("err:{:?}", e.kind()),
            },
        }
    }

    fn quiet(path: &std::path::Path) -> std::io::Result<std::process::ExitStatus> {
        std::process::Command::new(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    }

    /// The same without the null device: stdin a pipe, output inherited.
    ///
    /// Run 36199760841: with the null device on stdin, the container could
    /// not start System32's whoami.exe either — while the descendant probe,
    /// which opened no null device, started cmd.exe. Opening NUL is measured
    /// directly below.
    fn piped(path: &std::path::Path) -> std::io::Result<std::process::ExitStatus> {
        std::process::Command::new(path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
    }

    pub fn run_exec_child_if_requested() -> bool {
        let mut args = std::env::args();
        let Some(flag) = args.nth(1) else {
            return false;
        };
        if flag != EXEC_CHILD_FLAG {
            return false;
        }
        match is_app_container() {
            Ok(v) => println!("exec: appcontainer=ok:{v}"),
            Err(code) => println!("exec: appcontainer=err:{code}"),
        }
        // Where this process thinks its temporary directory is, against what
        // it was given. The first run wrote run-me.exe through temp_dir() and
        // the parent then found nothing at the scratch path.
        let cwd = std::env::current_dir().unwrap_or_default();
        println!("exec: temp-dir={}", std::env::temp_dir().display());
        println!(
            "exec: env-temp={}",
            std::env::var("TEMP").unwrap_or_else(|_| "(unset)".into())
        );
        println!("exec: cwd={}", cwd.display());
        let target = cwd.join("run-me.exe");
        println!(
            "exec: copy={}",
            outcome(std::fs::copy(system32("whoami.exe"), &target).map(|_| ()))
        );
        // The control: the container must be able to run a system program at
        // all, or a refusal from scratch proves nothing. The first run had no
        // such control, and its descendant step could not start cmd.exe either.
        // Both stdio forms, because which one works is itself unknown.
        let whoami = system32("whoami.exe");
        println!(
            "exec: open-nul={}",
            outcome(
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open("NUL")
                    .map(|_| ())
            )
        );
        println!("exec: system32-null={}", run_outcome(quiet(&whoami)));
        println!("exec: system32-piped={}", run_outcome(piped(&whoami)));
        println!("exec: scratch-null={}", run_outcome(quiet(&target)));
        println!("exec: scratch-piped={}", run_outcome(piped(&target)));
        true
    }

    /// The parent half: the confined child must be refused running what it
    /// wrote into scratch, and the unconfined parent must be able to run the
    /// same file — so the refusal is the container's, not a broken copy.
    pub const CONFINED_EXEC_FLAG: &str = "--sovatela-confined-exec-probe";

    pub fn run_confined_exec_probe_if_requested() -> bool {
        let mut args = std::env::args();
        let Some(flag) = args.nth(1) else {
            return false;
        };
        if flag != CONFINED_EXEC_FLAG {
            return false;
        }
        let Some(path) = args.next().map(std::path::PathBuf::from) else {
            std::process::exit(2);
        };
        let scratch = match Scratch::new(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("confined exec probe: could not prepare scratch: {e}");
                std::process::exit(3);
            }
        };
        let exe = std::env::current_exe().expect("current exe");
        let line = format!("\"{}\" {}", exe.display(), EXEC_CHILD_FLAG);
        let mut child = match Confined::spawn(&line, &scratch) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("confined exec probe: could not start: {e}");
                std::process::exit(4);
            }
        };
        drop(child.stdin.take());
        if !child.wait(30_000) {
            eprintln!("confined exec probe: did not exit within 30s");
            std::process::exit(5);
        }
        let mut out = String::new();
        if let Some(mut pipe) = child.stdout.take() {
            use std::io::Read;
            let _ = pipe.read_to_string(&mut out);
        }
        print!("{out}");
        let code = child.try_exit_code().unwrap_or(1) as i32;
        drop(child);
        let target = scratch.path().join("run-me.exe");
        println!("exec: parent-run={}", run_outcome(quiet(&target)));
        drop(scratch);
        std::process::exit(code);
    }
}
