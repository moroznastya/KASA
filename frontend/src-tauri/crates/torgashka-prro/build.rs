// build.rs — генерація Rust-типів з prro.proto (tonic + prost).
// Джерело proto — Python-еталон backend/app/infrastructure/services/prro/prro.proto
// (НЕ копіюється в crate: читається з backend-каталогу проєкту Torgashka).

use std::env;
use std::path::PathBuf;

fn main() {
    // C-обгортки для багнутого SDK EUSignCP (rbx=0 перед викликом):
    // ffi/euscp_wrappers.c — див. коментарі там (SIGSEGV-фікс етапу 7.3+).
    // Це GNU x86-64 asm (AT&T) — ТІЛЬКИ unix/Linux. На Windows SDK — DLL,
    // хак не потрібен (викликаємо EUReadPrivateKeyBinary напряму), а MSVC
    // цей C-файл не компілює (error C2143). Тому збираємо його лише не-windows.
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        cc::Build::new()
            .file(format!("{manifest_dir}/ffi/euscp_wrappers.c"))
            .warnings(false)
            .compile("euscp_wrappers");
        println!("cargo:rerun-if-changed={manifest_dir}/ffi/euscp_wrappers.c");
    }

    // protoc: vendored-бінарник (без залежності від системного protobuf-compiler)
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");
    std::env::set_var("PROTOC", protoc);

    // Шлях до prro.proto — відносно кореня репозиторію Torgashka.
    // CARGO_MANIFEST_DIR = frontend/src-tauri/crates/torgashka-prro
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    // піднімаємось: torgashka-prro -> crates -> src-tauri -> frontend -> torgashka
    let manifest = PathBuf::from(&manifest_dir);
    let repo_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("repo root (torgashka/)");
    let proto_file = repo_root.join("backend/app/infrastructure/services/prro/prro.proto");
    let proto_dir = proto_file.parent().expect("proto dir");

    println!("cargo:rerun-if-changed={}", proto_file.display());

    // Транзитивні залежності proto немає — single file.
    let proto_paths = [proto_file.clone()];

    tonic_build::configure()
        .build_client(true)
        .build_server(false)
        .compile_protos(&proto_paths, &[proto_dir.to_path_buf()])
        .unwrap_or_else(|e| panic!("tonic_build::compile_protos failed: {e}"));

    // prost_build як fallback (якщо tonic-build недостатньо) — не використовується,
    // tonic_build включає prost. Файл тримаємо для сумісності з CI-перевірками.
    let _ = prost_build::Config::new();
}
