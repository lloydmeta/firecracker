// Copyright 2017 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the THIRD-PARTY file.

use std::fs::File;
use std::io;
use std::mem;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::result;

use libc::{c_int, c_void, dup, eventfd, poll, pollfd, read, write, POLLIN};

/// A safe wrapper around a Linux eventfd (man 2 eventfd).
///
/// An eventfd is useful because it is sendable across processes and can be used for signaling in
/// and out of the KVM API. They can also be polled like any other file descriptor.
pub struct EventFd {
    eventfd: File,
}

impl EventFd {
    /// Creates a new blocking EventFd with an initial value of 0.
    pub fn new() -> result::Result<EventFd, io::Error> {
        // This is safe because eventfd merely allocated an eventfd for our process and we handle
        // the error case.
        let ret = unsafe { eventfd(0, 0) };
        if ret < 0 {
            Err(io::Error::last_os_error())
        } else {
            // This is safe because we checked ret for success and know the kernel gave us an fd that we
            // own.
            Ok(EventFd {
                eventfd: unsafe { File::from_raw_fd(ret) },
            })
        }
    }

    /// Adds `v` to the eventfd's count, blocking until this won't overflow the count.
    pub fn write(&self, v: u64) -> result::Result<(), io::Error> {
        // This is safe because we made this fd and the pointer we pass can not overflow because we
        // give the syscall's size parameter properly.
        let ret = unsafe {
            write(
                self.as_raw_fd(),
                &v as *const u64 as *const c_void,
                mem::size_of::<u64>(),
            )
        };
        if ret <= 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Blocks until the the eventfd's count is non-zero, then resets the count to zero.
    pub fn read(&self) -> result::Result<u64, io::Error> {
        let mut buf: u64 = 0;
        let ret = unsafe {
            // This is safe because we made this fd and the pointer we pass can not overflow because
            // we give the syscall's size parameter properly.
            read(
                self.as_raw_fd(),
                &mut buf as *mut u64 as *mut c_void,
                mem::size_of::<u64>(),
            )
        };
        if ret <= 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(buf)
        }
    }

    /// Tries to read the EventFd's count (but does not block if it is zero), then resets the count
    /// to zero.
    pub fn try_read(&self) -> result::Result<u64, Error> {
        let mut pd: pollfd;
        let poll_status = unsafe {
            pd = std::mem::uninitialized();
            pd.fd = self.as_raw_fd() as c_int;
            pd.events = POLLIN;
            poll(&mut pd, 1, 0)
        };
        if poll_status == 0 {
            Err(Error::NotReady)
        } else if poll_status > 0 {
            self.read().map_err(Error::ReadFailed)
        } else {
            Err(Error::ReadFailed(io::Error::last_os_error()))
        }
    }

    /// Clones this EventFd, internally creating a new file descriptor. The new EventFd will share
    /// the same underlying count within the kernel.
    pub fn try_clone(&self) -> result::Result<EventFd, io::Error> {
        // This is safe because we made this fd and properly check that it returns without error.
        let ret = unsafe { dup(self.as_raw_fd()) };
        if ret < 0 {
            Err(io::Error::last_os_error())
        } else {
            // This is safe because we checked ret for success and know the kernel gave us an fd that we
            // own.
            Ok(EventFd {
                eventfd: unsafe { File::from_raw_fd(ret) },
            })
        }
    }
}

impl AsRawFd for EventFd {
    fn as_raw_fd(&self) -> RawFd {
        self.eventfd.as_raw_fd()
    }
}

#[derive(Debug)]
/// Describes the errors that may occur while trying to work with EventFds
pub enum Error {
    /// The EventFd was not yet ready for the attempted operation
    NotReady,
    /// The read attempt on EventFd failed; the underlying IO error is contained within.
    ReadFailed(io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new() {
        EventFd::new().unwrap();
    }

    #[test]
    fn read_write() {
        let evt = EventFd::new().unwrap();
        evt.write(55).unwrap();
        assert_eq!(evt.read().unwrap(), 55);
    }

    #[test]
    fn try_read_nothing() {
        let evt = EventFd::new().unwrap();
        let r = evt.try_read();
        match r {
            Err(Error::NotReady) => (),
            _ => panic!("invalid state"),
        }
    }

    #[test]
    fn try_read_something() {
        let evt = EventFd::new().unwrap();
        evt.write(1189998819999197253).unwrap();
        let r = evt.try_read().unwrap();
        assert_eq!(r, 1189998819999197253);
    }

    #[test]
    fn clone() {
        let evt = EventFd::new().unwrap();
        let evt_clone = evt.try_clone().unwrap();
        evt.write(923).unwrap();
        assert_eq!(evt_clone.read().unwrap(), 923);
    }
}
