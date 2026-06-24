use cmake::Config;
use std::{env, fs, path::PathBuf};

fn copy_dir(src: &PathBuf, dst: &PathBuf) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dst_path);
        } else {
            fs::copy(entry.path(), &dst_path).unwrap();
        }
    }
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // EPANET's CMake writes generated headers back into the source tree.
    // Copy it to OUT_DIR first so the build works even when vendor/ is read-only.
    let epanet_out = out_dir.join("epanet_src");
    copy_dir(&PathBuf::from("vendor/epanet"), &epanet_out);

    let dst = Config::new(&epanet_out)
        .define("CMAKE_BUILD_TYPE", "Release")
        .define("BUILD_SHARED_LIBS", "OFF")
        .cflag("-DDLLEXPORT=")
        .build();

    // EPANET's CMakeLists.txt uses `install(TARGETS epanet2 DESTINATION .)`,
    // so the static lib lands directly in the install prefix, not in a lib/ subdir.
    println!("cargo:rustc-link-search=native={}", dst.display());
    println!("cargo:rustc-link-lib=static=epanet2");

    // Tell Cargo to re-run this script only when vendor/ changes.
    println!("cargo:rerun-if-changed=vendor/epanet");
}
