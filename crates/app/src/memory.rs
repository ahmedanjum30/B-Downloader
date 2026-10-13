//! Gives memory back to Windows while the app has nothing to do.

/// Removes the app's pages from RAM. A download's buffers and the drawing
/// caches stay in the working set after use; Windows pages back in whatever
/// is touched again, so this is only ever a small cost on the next use.
#[cfg(windows)]
pub fn trim() {
    use windows_sys::Win32::System::Memory::SetProcessWorkingSetSizeEx;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: the pseudo handle of the current process is always valid, and
    // `usize::MAX` for both sizes is the documented way to trim the working set.
    unsafe {
        SetProcessWorkingSetSizeEx(GetCurrentProcess(), usize::MAX, usize::MAX, 0);
    }
}

#[cfg(not(windows))]
pub fn trim() {}
