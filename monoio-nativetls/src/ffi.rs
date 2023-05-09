#[cfg(feature = "qat")]
use openssl_sys::{c_char, c_int, ENGINE};

#[cfg(feature = "qat")]
extern "C" {
    pub fn ENGINE_init(engine: *mut ENGINE) -> c_int;
    pub fn ENGINE_by_id(id: *const c_char) -> *mut ENGINE;
    pub fn ENGINE_set_default(engine: *mut ENGINE, flags: c_int) -> c_int;
}

#[cfg(feature = "qat")]
pub fn init_openssl_engine(name: &std::ffi::CStr) {
    openssl_sys::init();

    unsafe {
        let engine = ENGINE_by_id(name.as_ptr());
        if engine as usize == 0 {
            tracing::info!("engine: unknown");
            return;
        }
        let rc = ENGINE_init(engine);
        if rc == 0 {
            tracing::error!("engine: initialize failed");
            return;
        }
        tracing::info!("engine: {}", engine as usize);
        let rc = ENGINE_set_default(engine, 0xffff);
        if rc == 0 {
            tracing::error!("engine: initialize failed");
        }
        tracing::info!("engine: register successfully");
    }
}
