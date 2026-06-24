// Hand-written FFI bindings to OWA-EPANET 2.3 C toolkit.
// Only the functions used by pg_epanet are declared here.
// Values match epanet2_enums.h from OWA-EPANET 2.3.
use std::ffi::c_char;

/// Opaque handle to an EPANET project (void* in C).
#[allow(non_camel_case_types)]
pub type EN_Project = *mut std::ffi::c_void;

// EN_InitHydOption
pub const EN_NOSAVE: i32 = 0;

// EN_CountType
pub const EN_NODECOUNT: i32 = 0;
pub const EN_LINKCOUNT: i32 = 2;

// EN_NodeProperty
pub const EN_DEMAND:   i32 = 9;
pub const EN_HEAD:     i32 = 10;
pub const EN_PRESSURE: i32 = 11;

// EN_LinkProperty
pub const EN_FLOW:     i32 = 8;
pub const EN_VELOCITY: i32 = 9;
pub const EN_HEADLOSS: i32 = 10;

extern "C" {
    pub fn EN_createproject(ph: *mut EN_Project) -> i32;
    pub fn EN_deleteproject(ph: EN_Project) -> i32;
    pub fn EN_open(
        ph: EN_Project,
        inp_file: *const c_char,
        rpt_file: *const c_char,
        bin_out_file: *const c_char,
    ) -> i32;
    pub fn EN_close(ph: EN_Project) -> i32;

    // Single-shot hydraulic solve (not used for EPS, kept for reference).
    pub fn EN_solveH(ph: EN_Project) -> i32;

    // Extended Period Simulation (EPS) hydraulic loop.
    // `current_time` and `t_step` are in seconds; C type is `long` (i64 on macOS/Linux 64-bit).
    pub fn EN_openH(ph: EN_Project) -> i32;
    pub fn EN_initH(ph: EN_Project, init_flag: i32) -> i32;
    pub fn EN_runH(ph: EN_Project, current_time: *mut i64) -> i32;
    pub fn EN_nextH(ph: EN_Project, t_step: *mut i64) -> i32;
    pub fn EN_closeH(ph: EN_Project) -> i32;

    pub fn EN_getcount(ph: EN_Project, obj: i32, count: *mut i32) -> i32;
    pub fn EN_geterror(errcode: i32, errmsg: *mut c_char, max_len: i32) -> i32;
    pub fn EN_getnodeid(ph: EN_Project, index: i32, id: *mut c_char) -> i32;
    pub fn EN_getnodevalue(ph: EN_Project, index: i32, property: i32, value: *mut f64) -> i32;
    pub fn EN_getlinkid(ph: EN_Project, index: i32, id: *mut c_char) -> i32;
    pub fn EN_getlinkvalue(ph: EN_Project, index: i32, property: i32, value: *mut f64) -> i32;
}
