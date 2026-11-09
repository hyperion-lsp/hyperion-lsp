// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! glibc allocator tuning for the long-running server.
//!
//! The server allocates from dozens of tokio blocking, rayon and runtime
//! threads. glibc gives each thread group its own malloc arena (up to 8 x the
//! core count) and never returns freed memory of one arena to the others, so
//! bursts of parse/index/type-check work left the resident set at several GB
//! for the rest of the session: 4.0 GB after 150 s of edits in a 5,400-line
//! file versus 0.6 GB with `MALLOC_ARENA_MAX=2`. Capping the arenas and
//! trimming the free lists while the server idles keeps RSS close to the live
//! working set. `HYPERION_MALLOC_ARENA_MAX` overrides the cap; `MALLOC_ARENA_MAX`
//! set by the user is respected.

/// Arenas allowed when the user has not chosen a number.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const DEFAULT_ARENA_MAX: i32 = 4;

/// How often free heap memory is handed back to the operating system.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const TRIM_INTERVAL: std::time::Duration = std::time::Duration::from_secs(20);

/// Cap the malloc arenas and start the periodic trim. Call before any other
/// thread is spawned (arenas are assigned as threads first allocate).
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn tune() {
    if let Some(arenas) = arena_limit(
        std::env::var_os("MALLOC_ARENA_MAX").is_some(),
        std::env::var("HYPERION_MALLOC_ARENA_MAX").ok().as_deref(),
    ) {
        // SAFETY: `mallopt` only changes allocator parameters.
        unsafe {
            libc::mallopt(libc::M_ARENA_MAX, arenas);
        }
    }
    if let Err(error) = std::thread::Builder::new()
        .name("malloc-trim".to_owned())
        .spawn(|| loop {
            std::thread::sleep(TRIM_INTERVAL);
            // SAFETY: `malloc_trim` is safe to call from any thread.
            unsafe {
                libc::malloc_trim(0);
            }
        })
    {
        // Without the trim thread, freed memory is returned only when glibc decides to.
        eprintln!("hyperion: cannot start the malloc-trim thread: {error}");
    }
}

/// The arena cap to apply: none when the user chose one through glibc's own
/// variable, otherwise `HYPERION_MALLOC_ARENA_MAX` or the default.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn arena_limit(user_chose_glibc_variable: bool, hyperion_override: Option<&str>) -> Option<i32> {
    if user_chose_glibc_variable {
        return None;
    }
    Some(
        hyperion_override
            .and_then(|value| value.parse::<i32>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_ARENA_MAX),
    )
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub const fn tune() {}

#[cfg(all(test, target_os = "linux", target_env = "gnu"))]
mod tests {
    use super::*;

    #[test]
    fn arena_cap_defaults_honours_overrides_and_respects_the_glibc_variable() {
        assert_eq!(arena_limit(false, None), Some(DEFAULT_ARENA_MAX));
        assert_eq!(arena_limit(false, Some("2")), Some(2));
        assert_eq!(arena_limit(false, Some("0")), Some(DEFAULT_ARENA_MAX));
        assert_eq!(arena_limit(false, Some("many")), Some(DEFAULT_ARENA_MAX));
        assert_eq!(arena_limit(true, Some("2")), None);
    }
}
