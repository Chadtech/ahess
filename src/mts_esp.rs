use std::{
    error::Error,
    ffi::{c_char, c_void, CStr, CString},
    fmt,
    ptr::NonNull,
};

use libc::{dlclose, dlerror, dlopen, dlsym, RTLD_NOW};

const LIBRARY_PATH: &CStr = c"/Library/Application Support/MTS-ESP/libMTS.dylib";

type VoidFunction = unsafe extern "C" fn();
type BoolFunction = unsafe extern "C" fn() -> bool;
type SetNoteTuningFunction = unsafe extern "C" fn(f64, c_char);
type SetMultiChannelFunction = unsafe extern "C" fn(bool, i8);
type SetMultiChannelNoteTuningFunction = unsafe extern "C" fn(f64, c_char, i8);
type SetScaleNameFunction = unsafe extern "C" fn(*const c_char);
#[cfg(test)]
type GetTuningTableFunction = unsafe extern "C" fn() -> *const f64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MtsNoteAddress {
    pub(crate) channel: u8,
    pub(crate) note: u8,
}

pub(crate) struct MtsEspMaster {
    handle: NonNull<c_void>,
    deregister_master: VoidFunction,
    set_note_tuning: SetNoteTuningFunction,
    set_multi_channel_note_tuning: SetMultiChannelNoteTuningFunction,
}

// libMTS is process-global middleware designed to connect audio clients across
// render threads. This value owns one master registration and is only mutated
// through the middleware's thread-safe C entry points.
unsafe impl Send for MtsEspMaster {}
unsafe impl Sync for MtsEspMaster {}

impl MtsEspMaster {
    pub(crate) fn is_available() -> bool {
        // SAFETY: the path is a valid nul-terminated string and a successful
        // handle is closed before returning.
        let handle = unsafe { dlopen(LIBRARY_PATH.as_ptr(), RTLD_NOW) };
        let Some(handle) = NonNull::new(handle) else {
            return false;
        };
        // SAFETY: `handle` was returned by dlopen above.
        unsafe { dlclose(handle.as_ptr()) };
        true
    }

    pub(crate) fn new() -> Result<Self, MtsEspError> {
        // SAFETY: the path is a valid nul-terminated string.
        let handle =
            NonNull::new(unsafe { dlopen(LIBRARY_PATH.as_ptr(), RTLD_NOW) }).ok_or_else(|| {
                MtsEspError::new(format!(
                    "MTS-ESP middleware is not installed at {}: {}",
                    LIBRARY_PATH.to_string_lossy(),
                    dynamic_loader_error()
                ))
            })?;

        let result = (|| {
            let has_master: BoolFunction = load_symbol(handle, c"MTS_HasMaster")?;
            let register_master: VoidFunction = load_symbol(handle, c"MTS_RegisterMaster")?;
            let deregister_master: VoidFunction = load_symbol(handle, c"MTS_DeregisterMaster")?;
            let set_note_tuning = load_symbol(handle, c"MTS_SetNoteTuning")?;
            let set_multi_channel: SetMultiChannelFunction =
                load_symbol(handle, c"MTS_SetMultiChannel")?;
            let set_multi_channel_note_tuning =
                load_symbol(handle, c"MTS_SetMultiChannelNoteTuning")?;
            let set_scale_name: SetScaleNameFunction = load_symbol(handle, c"MTS_SetScaleName")?;

            // SAFETY: all function pointers were resolved from the live libMTS
            // handle and have the signatures published by its master API.
            if unsafe { has_master() } {
                return Err(MtsEspError::new(
                    "another MTS-ESP master is already active; close it before playing Surge XT from Ahess",
                ));
            }
            unsafe {
                register_master();
                for channel in 0..16 {
                    set_multi_channel(true, channel);
                }
                let scale_name = CString::new("Ahess exact frequencies").unwrap();
                set_scale_name(scale_name.as_ptr());
            }

            Ok(Self {
                handle,
                deregister_master,
                set_note_tuning,
                set_multi_channel_note_tuning,
            })
        })();

        if result.is_err() {
            // SAFETY: ownership was not transferred into a successful value.
            unsafe { dlclose(handle.as_ptr()) };
        }
        result
    }

    pub(crate) fn set_frequency(&self, address: MtsNoteAddress, frequency_hz: f64) {
        // Supply both tables: Surge XT uses the channel-specific value, while
        // the general table keeps the MTS-ESP fallback coherent.
        unsafe {
            (self.set_note_tuning)(frequency_hz, address.note as c_char);
            (self.set_multi_channel_note_tuning)(
                frequency_hz,
                address.note as c_char,
                address.channel as i8,
            );
        }
    }
}

impl Drop for MtsEspMaster {
    fn drop(&mut self) {
        // SAFETY: this value owns the sole registration and live library handle.
        unsafe {
            (self.deregister_master)();
            dlclose(self.handle.as_ptr());
        }
    }
}

#[cfg(test)]
pub(crate) struct MtsEspTuningProbe {
    handle: NonNull<c_void>,
    get_tuning_table: GetTuningTableFunction,
}

#[cfg(test)]
impl MtsEspTuningProbe {
    pub(crate) fn new() -> Result<Self, MtsEspError> {
        let handle =
            NonNull::new(unsafe { dlopen(LIBRARY_PATH.as_ptr(), RTLD_NOW) }).ok_or_else(|| {
                MtsEspError::new(format!(
                    "MTS-ESP middleware is not installed at {}: {}",
                    LIBRARY_PATH.to_string_lossy(),
                    dynamic_loader_error()
                ))
            })?;
        let result = (|| {
            let get_tuning_table = load_symbol(handle, c"MTS_GetTuningTable")?;
            Ok(Self {
                handle,
                get_tuning_table,
            })
        })();
        if result.is_err() {
            unsafe { dlclose(handle.as_ptr()) };
        }
        result
    }

    pub(crate) fn frequency(&self, note: u8) -> f64 {
        let table = unsafe { (self.get_tuning_table)() };
        assert!(
            !table.is_null(),
            "MTS-ESP returned a null general tuning table"
        );
        unsafe { *table.add(usize::from(note)) }
    }
}

#[cfg(test)]
impl Drop for MtsEspTuningProbe {
    fn drop(&mut self) {
        unsafe { dlclose(self.handle.as_ptr()) };
    }
}

fn load_symbol<T: Copy>(handle: NonNull<c_void>, name: &CStr) -> Result<T, MtsEspError> {
    // Clear any prior loader error, then query and inspect the new error state.
    unsafe {
        dlerror();
        let symbol = dlsym(handle.as_ptr(), name.as_ptr());
        let error = dlerror();
        if !error.is_null() || symbol.is_null() {
            return Err(MtsEspError::new(format!(
                "MTS-ESP middleware is missing {}: {}",
                name.to_string_lossy(),
                dynamic_loader_error_from(error)
            )));
        }
        Ok(std::mem::transmute_copy(&symbol))
    }
}

fn dynamic_loader_error() -> String {
    // SAFETY: dlerror returns either null or a nul-terminated diagnostic owned
    // by the dynamic loader.
    dynamic_loader_error_from(unsafe { dlerror() })
}

fn dynamic_loader_error_from(error: *const c_char) -> String {
    if error.is_null() {
        "unknown dynamic-loader error".to_string()
    } else {
        // SAFETY: non-null dlerror results are nul-terminated strings.
        unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned()
    }
}

#[derive(Debug)]
pub(crate) struct MtsEspError {
    message: String,
}

impl MtsEspError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for MtsEspError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for MtsEspError {}

#[cfg(test)]
mod tests {
    use super::{MtsEspMaster, MtsNoteAddress};

    #[test]
    #[ignore = "requires the installed MTS-ESP middleware"]
    fn installed_middleware_registers_an_ahess_master() {
        let master = MtsEspMaster::new().unwrap();
        master.set_frequency(
            MtsNoteAddress {
                channel: 0,
                note: 60,
            },
            432.123_456_789,
        );
    }
}
