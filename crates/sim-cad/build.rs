use std::{env, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-changed=native/bridge.cpp");
    println!("cargo:rerun-if-changed=native/ops.cpp");
    for key in ["OCCT_INCLUDE_DIR", "OCCT_LIB_DIR"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    // OCCT_INCLUDE_DIR, else a local OCCT 7.7.2 build (~/.local/occt-7.7.2, see
    // README "Installing OCCT"), else the usual system prefixes.
    let local = env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/occt-7.7.2/include/opencascade"));
    let include = env::var_os("OCCT_INCLUDE_DIR").map(PathBuf::from).or_else(|| {
        local
            .into_iter()
            .chain(["/opt/homebrew/include/opencascade", "/usr/local/include/opencascade", "/usr/include/opencascade"].into_iter().map(PathBuf::from))
            .find(|p| p.join("Standard_Version.hxx").exists())
    }).expect("sim-cad requires OCCT 7.7.x/7.8.x headers: build OCCT 7.7.2 into ~/.local/occt-7.7.2 (README) or set OCCT_INCLUDE_DIR (no runtime server fallback)");
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
        .file("native/ops.cpp")
        .compile("sim_cad_occt");
    println!("cargo:rustc-link-search=native={}", lib.display());
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-lib=dl");
    }
    // Building (booleans, fillets, sections) needs TKFillet/TKBO/TKBool and
    // their healing and algorithm toolkits; reading and meshing need the rest.
    for name in [
        "TKSTEP",
        "TKSTEP209",
        "TKSTEPAttr",
        "TKSTEPBase",
        "TKIGES",
        "TKXSBase",
        "TKOffset",
        "TKFeat",
        "TKHLR",
        "TKFillet",
        "TKBO",
        "TKBool",
        "TKShHealing",
        "TKGeomAlgo",
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
