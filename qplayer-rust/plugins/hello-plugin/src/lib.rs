//! Example QPlayer plugin.
//!
//! Compiles to `wasm32-unknown-unknown` and exports lifecycle hooks.
//! Uses `no_std` because the target has no OS support.

#![no_std]

// Host import: log a message.
// level: 0=info, 1=warn, 2=error
#[link(wasm_import_module = "env")]
unsafe extern "C" {
    fn host_log(level: i32, ptr: *const u8, len: i32);
}

/// Helper to log a static byte string.
fn log_str(level: i32, msg: &[u8]) {
    unsafe { host_log(level, msg.as_ptr(), msg.len() as i32) };
}

#[unsafe(no_mangle)]
pub extern "C" fn qplayer_plugin_on_load() {
    log_str(0, b"hello-plugin: loaded!");
}

#[unsafe(no_mangle)]
pub extern "C" fn qplayer_plugin_on_unload() {
    log_str(0, b"hello-plugin: unloading...");
}

#[unsafe(no_mangle)]
pub extern "C" fn qplayer_plugin_on_go(qid: i32) {
    // We can't format the qid into a string without alloc, so just log a static message.
    // A real plugin would use host_alloc / host_free to build dynamic strings.
    let _ = qid;
    log_str(0, b"hello-plugin: on_go called");
}

#[unsafe(no_mangle)]
pub extern "C" fn qplayer_plugin_on_save() {
    log_str(0, b"hello-plugin: on_save called");
}

#[unsafe(no_mangle)]
pub extern "C" fn qplayer_plugin_on_slow_update() {
    // Kept quiet to avoid log spam
}

// Panic handler required for no_std on wasm32
#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
