//! Temporary file paths for the EPANET C API (file-based I/O only).

use pgrx::prelude::*;
use std::ffi::CStr;
use std::path::{Path, PathBuf};

static TEMP_DIR: GucSetting<Option<&'static CStr>> = GucSetting::<Option<&'static CStr>>::new(None);

/// Registers `pg_epanet.temp_dir` GUC at extension load.
pub fn register_gucs() {
    GucRegistry::define_string_guc(
        c"pg_epanet.temp_dir",
        c"Directory for EPANET temporary .inp/.rpt/.out files during simulation",
        c"Defaults to TMPDIR environment variable, then /tmp. Set this on managed Postgres when /tmp is not writable.",
        &TEMP_DIR,
        GucContext::Userset,
        GucFlags::default(),
    );
}

pub fn temp_directory() -> PathBuf {
    if let Some(dir) = TEMP_DIR.get() {
        let s = dir.to_string_lossy();
        if !s.is_empty() {
            return PathBuf::from(s.as_ref());
        }
    }
    std::env::var("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

pub struct TempProjectFiles {
    pub inp_path: PathBuf,
    pub rpt_path: PathBuf,
    pub out_path: PathBuf,
}

impl TempProjectFiles {
    pub fn new(prefix: &str) -> Self {
        let dir = temp_directory();
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| {
            error!("Cannot create temp directory {}: {e}", dir.display())
        });
        Self {
            inp_path: dir.join(format!("{prefix}.inp")),
            rpt_path: dir.join(format!("{prefix}.rpt")),
            out_path: dir.join(format!("{prefix}.out")),
        }
    }

    pub fn write_inp(&self, inp_text: &str) {
        std::fs::write(&self.inp_path, inp_text).unwrap_or_else(|e| {
            error!(
                "Cannot write temp INP file {}: {e}",
                self.inp_path.display()
            )
        });
    }
}

impl Drop for TempProjectFiles {
    fn drop(&mut self) {
        for p in [&self.inp_path, &self.rpt_path, &self.out_path] {
            let _ = std::fs::remove_file(p);
        }
    }
}

pub fn path_to_cstring(path: &Path) -> std::ffi::CString {
    std::ffi::CString::new(path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| error!("Temp path contains interior null byte"))
}
