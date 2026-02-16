fn main() {
    cc::Build::new()
        .include("rtklib_c")
        .files(&[
            "rtklib_c/rtcm.c",
            "rtklib_c/rtcm3.c",
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
        .warnings(false)
        .compile("rtklib");
}
