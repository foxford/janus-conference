#![allow(non_camel_case_types)]

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_long, c_uint};

use anyhow::{bail, format_err, Context, Result};
use janus_plugin_sys::janus_refcount;
use libc::{pthread_mutex_t, FILE};

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Codec {
    VP8,
    OPUS,
    H264,
    G711,
    VP9,
}

impl Codec {
    fn as_str(self) -> &'static str {
        match self {
            Self::VP8 => "vp8",
            Self::OPUS => "opus",
            Self::H264 => "h264",
            Self::G711 => "g711",
            Self::VP9 => "vp9",
        }
    }
}

pub struct JanusRecorder<'a> {
    recorder: &'a mut janus_recorder,
}

impl<'a> JanusRecorder<'a> {
    pub fn create(dir: &str, filename: &str, codec: Codec) -> Result<Self> {
        let dir = CString::new(dir).context("Failed to cast `dir` to CString")?;
        let filename = CString::new(filename).context("Failed to cast `filename` to CString")?;
        let codec = CString::new(codec.as_str()).context("Failed to cast `codec` to CString")?;

        unsafe { janus_recorder_create(dir.as_ptr(), codec.as_ptr(), filename.as_ptr()).as_mut() }
            .ok_or_else(|| format_err!("Failed to create recorder"))
            .map(|recorder| Self { recorder })
    }

    pub fn save_frame(&mut self, buffer: &[i8]) -> Result<()> {
        let res = unsafe {
            janus_recorder_save_frame(self.recorder, buffer.as_ptr(), buffer.len() as u32)
        };

        if res == 0 {
            Ok(())
        } else {
            bail!("Failed to save frame: {}", res)
        }
    }

    pub fn close(&mut self) -> Result<()> {
        let res = unsafe { janus_recorder_close(self.recorder) };

        if res == 0 {
            Ok(())
        } else {
            bail!("Failed to close recorder: {}", res)
        }
    }
}

impl Drop for JanusRecorder<'_> {
    fn drop(&mut self) {
        unsafe { janus_recorder_destroy(self.recorder) };
    }
}

///////////////////////////////////////////////////////////////////////////////

type gboolean = c_int;
type gint = c_int;
type gint64 = c_long;
type janus_mutex = pthread_mutex_t;

#[allow(dead_code)]
#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum janus_recorder_medium {
    JANUS_RECORDER_AUDIO = 0,
    JANUS_RECORDER_VIDEO = 1,
    JANUS_RECORDER_DATA = 2,
}

#[repr(C)]
struct janus_recorder {
    dir: *mut c_char,
    filename: *mut c_char,
    file: *mut FILE,
    codec: *mut c_char,
    fmtp: *mut c_char,
    created: gint64,
    started: gint64,
    type_: janus_recorder_medium,
    encrypted: gboolean,
    header: c_int,
    writable: c_int,
    mutex: janus_mutex,
    destroyed: gint,
    ref_: janus_refcount,
}

#[cfg(not(test))]
extern "C" {
    fn janus_recorder_create(
        dir: *const c_char,
        codec: *const c_char,
        filename: *const c_char,
    ) -> *mut janus_recorder;

    fn janus_recorder_save_frame(
        recorder: *mut janus_recorder,
        buffer: *const c_char,
        length: c_uint,
    ) -> c_int;

    fn janus_recorder_close(recorder: *mut janus_recorder) -> c_int;
    fn janus_recorder_destroy(recorder: *mut janus_recorder);
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[no_mangle]
unsafe extern "C" fn janus_recorder_create(
    _dir: *const c_char,
    _codec: *const c_char,
    _filename: *const c_char,
) -> *mut janus_recorder {
    std::ptr::null_mut()
}

#[cfg(test)]
#[no_mangle]
unsafe extern "C" fn janus_recorder_save_frame(
    _recorder: *mut janus_recorder,
    _buffer: *const c_char,
    _length: c_uint,
) -> c_int {
    0
}

#[cfg(test)]
#[no_mangle]
unsafe extern "C" fn janus_recorder_close(_recorder: *mut janus_recorder) -> c_int {
    0
}

#[cfg(test)]
#[no_mangle]
unsafe extern "C" fn janus_recorder_destroy(_recorder: *mut janus_recorder) {}
