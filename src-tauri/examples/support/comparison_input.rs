//! QA-only input transport. A fixed deadline covers bytes AND EOF; an idle
//! timeout reset after each chunk would let a trickle keep the helper alive.
use std::io;
use std::os::fd::RawFd;
use std::time::{Duration, Instant};

pub fn read_input(
    fd: RawFd,
    limit: usize,
    budget: Duration,
    mut progress: impl FnMut(usize, bool),
) -> io::Result<Vec<u8>> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    struct Restore(RawFd, libc::c_int);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe { libc::fcntl(self.0, libc::F_SETFL, self.1) };
        }
    }
    let _restore = Restore(fd, flags);
    let start = Instant::now();
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    progress(0, false);
    loop {
        let remaining = budget.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "input deadline exceeded after {} bytes; EOF not received",
                    bytes.len()
                ),
            ));
        }
        let mut pollfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let wait_ms = remaining
            .as_millis()
            .saturating_add(1)
            .min(i32::MAX as u128) as i32;
        let ready = unsafe { libc::poll(&mut pollfd, 1, wait_ms) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if ready == 0 || start.elapsed() >= budget {
            continue;
        }
        if pollfd.revents & libc::POLLNVAL != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid input descriptor",
            ));
        }
        // Nonblocking read: a readiness notification is not permission to
        // enter an unbounded blocking read if the descriptor state changes.
        let capacity = buffer
            .len()
            .min(limit.saturating_sub(bytes.len()).saturating_add(1));
        let n = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), capacity) };
        if n < 0 {
            let error = io::Error::last_os_error();
            if matches!(
                error.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) {
                continue;
            }
            return Err(error);
        }
        if n == 0 {
            progress(bytes.len(), true);
            return Ok(bytes);
        }
        bytes.extend_from_slice(&buffer[..n as usize]);
        progress(bytes.len(), false);
        if bytes.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "PDF exceeds the upload limit",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    #[test]
    fn delivered_bytes_without_eof_still_time_out_and_flags_are_restored() {
        let (read, mut write) = UnixStream::pair().unwrap();
        write.write_all(b"delivered").unwrap();
        let flags = unsafe { libc::fcntl(read.as_raw_fd(), libc::F_GETFL) };
        let mut seen = 0;
        let error = read_input(read.as_raw_fd(), 1024, Duration::from_millis(60), |n, _| {
            seen = n
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(seen, 9);
        assert_eq!(
            unsafe { libc::fcntl(read.as_raw_fd(), libc::F_GETFL) },
            flags
        );
    }

    #[test]
    fn large_input_is_drained_and_eof_is_distinguished_from_progress() {
        let (read, mut write) = UnixStream::pair().unwrap();
        let bytes = vec![42; 512 * 1024];
        let expected = bytes.clone();
        let writer = std::thread::spawn(move || write.write_all(&bytes).unwrap());
        let mut eof = false;
        let actual = read_input(
            read.as_raw_fd(),
            expected.len(),
            Duration::from_secs(5),
            |_, done| eof = done,
        )
        .unwrap();
        writer.join().unwrap();
        assert_eq!(actual, expected);
        assert!(eof);
    }

    #[test]
    fn trickling_bytes_cannot_reset_the_absolute_deadline() {
        let (read, mut write) = UnixStream::pair().unwrap();
        let writer = std::thread::spawn(move || {
            for _ in 0..100 {
                if write.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let start = Instant::now();
        let result = read_input(read.as_raw_fd(), 1024, Duration::from_millis(80), |_, _| {});
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(2));
        drop(read);
        writer.join().unwrap();
    }

    #[test]
    fn oversize_input_is_rejected_without_waiting_for_eof() {
        let (read, mut write) = UnixStream::pair().unwrap();
        write.write_all(b"12345").unwrap();
        let error = read_input(read.as_raw_fd(), 4, Duration::from_secs(1), |_, _| {}).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
