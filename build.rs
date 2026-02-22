fn git_info() -> (String, String) {
    let describe = std::process::Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    (describe, hash)
}

fn main() {
    // git HEAD が変わったら再ビルド
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");

    let (describe, hash) = git_info();
    println!("cargo:rustc-env=GIT_DESCRIBE={}", describe);
    println!("cargo:rustc-env=GIT_HASH={}", hash);

    cc::Build::new()
        .include("rtklib_c")
        .files(&[
            "rtklib_c/rtcm.c",
            "rtklib_c/rtcm2.c",
            "rtklib_c/rtcm3.c",
            "rtklib_c/rtcm3e.c",
            "rtklib_c/rtkcmn.c",
            "rtklib_c/solution.c",
            "rtklib_c/trace.c",
            "rtklib_c/rtklib_wrapper.c",
        ])
        .define("ENAGLO", None)
        .define("ENAGAL", None)
        .define("ENACMP", None)
        .define("ENAQZS", None)
        .define("ENAIRN", None)
        .define("TRACE", None)
        .define("WIN32", None)
        .warnings(false)
        .compile("rtklib");
}
