use std::{env, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-changed=native/bridge.cpp");
    for key in ["OCCT_INCLUDE_DIR", "OCCT_LIB_DIR"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let include = env::var_os("OCCT_INCLUDE_DIR").map(PathBuf::from).or_else(|| {
        ["/opt/homebrew/include/opencascade", "/usr/local/include/opencascade", "/usr/include/opencascade"].into_iter().map(PathBuf::from).find(|p| p.join("Standard_Version.hxx").exists())
    }).expect("sim-cad requires OCCT 7.7.x/7.8.x headers; set OCCT_INCLUDE_DIR (no runtime server fallback)");
    let lib = env::var_os("OCCT_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            include
                .parent()
                .and_then(|p| p.parent())
                .unwrap()
                .join("lib")
        });
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .include(include)
        .file("native/bridge.cpp")
        .compile("sim_cad_occt");
    println!("cargo:rustc-link-search=native={}", lib.display());
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-lib=dl");
    }
    for name in [
        "TKMesh",
        "TKPrim",
        "TKTopAlgo",
        "TKBRep",
        "TKGeomBase",
        "TKG3d",
        "TKG2d",
        "TKMath",
        "TKernel",
    ] {
        println!("cargo:rustc-link-lib={name}");
    }
}
