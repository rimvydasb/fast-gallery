//! Thread pools. Decode work never runs on the UI thread (PERF-11), and the number of decode
//! threads does not exceed the number of performance CPU cores (THM-12).

use std::ffi::CStr;
use std::sync::OnceLock;

use rayon::{ThreadPool, ThreadPoolBuilder};

fn sysctl_u32(name: &CStr) -> Option<u32> {
    let mut v: u32 = 0;
    let mut len = std::mem::size_of::<u32>();
    let r = unsafe {
        libc::sysctlbyname(name.as_ptr(), (&mut v as *mut u32).cast(), &mut len, std::ptr::null_mut(), 0)
    };
    (r == 0 && v > 0).then_some(v)
}

/// Number of performance cores (Apple Silicon: perflevel0; Intel: all physical cores).
pub fn perf_cores() -> usize {
    sysctl_u32(c"hw.perflevel0.physicalcpu")
        .or_else(|| sysctl_u32(c"hw.physicalcpu"))
        .map(|v| v as usize)
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4))
}

/// macOS major version, for example 14 for Sonoma.
pub fn macos_major() -> u32 {
    static V: OnceLock<u32> = OnceLock::new();
    *V.get_or_init(|| {
        let mut buf = [0u8; 32];
        let mut len = buf.len();
        let r = unsafe {
            libc::sysctlbyname(
                c"kern.osproductversion".as_ptr(),
                buf.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if r != 0 {
            return 12;
        }
        let s = std::str::from_utf8(&buf[..len.saturating_sub(1)]).unwrap_or("12");
        s.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(12)
    })
}

fn build(name: &'static str, n: usize) -> ThreadPool {
    ThreadPoolBuilder::new()
        .num_threads(n.max(1))
        .thread_name(move |i| format!("{name}-{i}"))
        .build()
        .expect("thread pool")
}

/// Image decode and encode.
pub fn decode() -> &'static ThreadPool {
    static P: OnceLock<ThreadPool> = OnceLock::new();
    P.get_or_init(|| build("decode", perf_cores()))
}

/// Directory scans and folder preview resolution (I/O bound).
pub fn scan() -> &'static ThreadPool {
    static P: OnceLock<ThreadPool> = OnceLock::new();
    P.get_or_init(|| build("scan", 4))
}

/// Cache reads and file reads for the protocols.
pub fn io() -> &'static ThreadPool {
    static P: OnceLock<ThreadPool> = OnceLock::new();
    P.get_or_init(|| build("io", 4))
}
