use std::ffi::{c_char, c_int, c_void};
use std::fmt;

/// Opaque pointer to C rtcm_t
pub type RtcmPtr = *mut c_void;

/// Flat observation record from C wrapper
#[repr(C)]
#[derive(Debug, Clone)]
pub struct RtcmObs {
    pub time_sec: f64,
    pub frac_sec: f64,
    pub sat: u8,
    pub sys: u8,
    pub prn: u8,
    pub code: [u8; 3],
    pub p: [f64; 3],
    pub l: [f64; 3],
    pub d: [f32; 3],
    pub snr: [f32; 3],
    pub lli: [u8; 3],
}

/// Flat station info from C wrapper
#[repr(C)]
#[derive(Clone)]
pub struct RtcmSta {
    pub staid: c_int,
    pub pos: [f64; 3],
    pub hgt: f64,
    pub antdes: [c_char; 64],
    pub antsno: [c_char; 64],
    pub rectype: [c_char; 64],
    pub recver: [c_char; 64],
}

/// Flat ephemeris summary from C wrapper
#[repr(C)]
#[derive(Debug, Clone)]
pub struct RtcmEphSummary {
    pub sat: u8,
    pub sys: u8,
    pub prn: u8,
    pub iode: c_int,
    pub svh: c_int,
    pub toe_sec: f64,
}

/// Decode result from input_rtcm3
#[repr(C)]
#[derive(Clone)]
pub struct RtcmDecodeResult {
    pub ret: c_int,
    pub msg_type: c_int,
    pub msg_type_str: [c_char; 256],
}

unsafe extern "C" {
    fn rtklib_alloc_rtcm() -> RtcmPtr;
    fn rtklib_free_rtcm(rtcm: RtcmPtr);
    fn rtklib_input_rtcm3(rtcm: RtcmPtr, data: u8, out: *mut RtcmDecodeResult);
    fn rtklib_get_obs_count(rtcm: RtcmPtr) -> c_int;
    fn rtklib_get_obs(rtcm: RtcmPtr, index: c_int, out: *mut RtcmObs) -> c_int;
    fn rtklib_get_sta(rtcm: RtcmPtr, out: *mut RtcmSta) -> c_int;
    fn rtklib_get_eph_summary(rtcm: RtcmPtr, out: *mut RtcmEphSummary) -> c_int;
    fn rtklib_get_msg_counts(
        rtcm: RtcmPtr,
        types: *mut c_int,
        counts: *mut u32,
        max_entries: c_int,
    ) -> c_int;
}

/// Safe wrapper around the RTKLIB RTCM3 decoder
pub struct RtcmDecoder {
    ptr: RtcmPtr,
}

// RtcmDecoder is Send because the C rtcm_t is only accessed from one thread
unsafe impl Send for RtcmDecoder {}

/// Decoded RTCM3 message event
#[derive(Debug, Clone)]
pub enum RtcmEvent {
    /// Observation data decoded
    Observation {
        msg_type: i32,
        msg_desc: String,
        observations: Vec<RtcmObs>,
    },
    /// Ephemeris decoded
    Ephemeris {
        msg_type: i32,
        msg_desc: String,
        summary: RtcmEphSummary,
    },
    /// Station info decoded
    Station {
        msg_type: i32,
        msg_desc: String,
        staid: i32,
        pos: [f64; 3],
        hgt: f64,
        antdes: String,
        rectype: String,
    },
    /// SSR correction decoded
    Ssr {
        msg_type: i32,
        msg_desc: String,
    },
    /// Other message decoded (non-zero return)
    Other {
        msg_type: i32,
        msg_desc: String,
        ret: i32,
    },
}

/// Satellite system name from sys code
pub fn sys_name(sys: u8) -> &'static str {
    match sys {
        0x01 => "GPS",
        0x04 => "GLO",
        0x08 => "GAL",
        0x10 => "QZS",
        0x20 => "BDS",
        0x40 => "IRN",
        0x02 => "SBS",
        _ => "???",
    }
}

impl fmt::Display for RtcmObs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{:3} P1={:14.3} L1={:14.3} SNR={:5.1}",
            sys_name(self.sys),
            self.prn,
            self.p[0],
            self.l[0],
            self.snr[0]
        )
    }
}

fn c_char_to_string(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).to_string()
}

impl RtcmDecoder {
    /// Create a new RTCM3 decoder
    pub fn new() -> Option<Self> {
        let ptr = unsafe { rtklib_alloc_rtcm() };
        if ptr.is_null() {
            None
        } else {
            Some(Self { ptr })
        }
    }

    /// Feed one byte into the decoder. Returns Some(event) when a message is decoded.
    pub fn input(&mut self, byte: u8) -> Option<RtcmEvent> {
        let mut result = std::mem::MaybeUninit::<RtcmDecodeResult>::uninit();
        unsafe {
            rtklib_input_rtcm3(self.ptr, byte, result.as_mut_ptr());
        }
        let result = unsafe { result.assume_init() };
        let ret = result.ret as i32;

        if ret <= 0 {
            return None;
        }

        let msg_type = result.msg_type as i32;
        let msg_desc = c_char_to_string(&result.msg_type_str);

        match ret {
            1 => {
                // Observation data
                let count = unsafe { rtklib_get_obs_count(self.ptr) } as i32;
                let mut observations = Vec::with_capacity(count as usize);
                for i in 0..count {
                    let mut obs = std::mem::MaybeUninit::<RtcmObs>::uninit();
                    let ok =
                        unsafe { rtklib_get_obs(self.ptr, i as c_int, obs.as_mut_ptr()) };
                    if ok != 0 {
                        observations.push(unsafe { obs.assume_init() });
                    }
                }
                Some(RtcmEvent::Observation {
                    msg_type,
                    msg_desc,
                    observations,
                })
            }
            2 => {
                // Ephemeris
                let mut summary = std::mem::MaybeUninit::<RtcmEphSummary>::uninit();
                let ok =
                    unsafe { rtklib_get_eph_summary(self.ptr, summary.as_mut_ptr()) };
                if ok != 0 {
                    Some(RtcmEvent::Ephemeris {
                        msg_type,
                        msg_desc,
                        summary: unsafe { summary.assume_init() },
                    })
                } else {
                    Some(RtcmEvent::Other {
                        msg_type,
                        msg_desc,
                        ret,
                    })
                }
            }
            5 => {
                // Station info
                let mut sta = std::mem::MaybeUninit::<RtcmSta>::uninit();
                let ok = unsafe { rtklib_get_sta(self.ptr, sta.as_mut_ptr()) };
                if ok != 0 {
                    let sta = unsafe { sta.assume_init() };
                    Some(RtcmEvent::Station {
                        msg_type,
                        msg_desc,
                        staid: sta.staid as i32,
                        pos: sta.pos,
                        hgt: sta.hgt,
                        antdes: c_char_to_string(&sta.antdes),
                        rectype: c_char_to_string(&sta.rectype),
                    })
                } else {
                    Some(RtcmEvent::Other {
                        msg_type,
                        msg_desc,
                        ret,
                    })
                }
            }
            10 | 20 => {
                // SSR corrections
                Some(RtcmEvent::Ssr {
                    msg_type,
                    msg_desc,
                })
            }
            _ => Some(RtcmEvent::Other {
                msg_type,
                msg_desc,
                ret,
            }),
        }
    }

    /// Get message type counts (type_number, count)
    pub fn msg_counts(&self) -> Vec<(i32, u32)> {
        let mut types = [0i32; 128];
        let mut counts = [0u32; 128];
        let n = unsafe {
            rtklib_get_msg_counts(
                self.ptr,
                types.as_mut_ptr(),
                counts.as_mut_ptr(),
                128,
            )
        } as usize;
        types[..n]
            .iter()
            .zip(counts[..n].iter())
            .map(|(&t, &c)| (t, c))
            .collect()
    }
}

impl Drop for RtcmDecoder {
    fn drop(&mut self) {
        unsafe {
            rtklib_free_rtcm(self.ptr);
        }
    }
}
