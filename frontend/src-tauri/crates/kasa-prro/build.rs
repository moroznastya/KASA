// build.rs — генерація Rust-типів з prro.proto (tonic + prost).
// Джерело proto — Python-еталон backend/app/infrastructure/services/prro/prro.proto
// (НЕ копіюється в crate: читається з backend-каталогу проєкту Kasa).

use std::env;
use std::path::PathBuf;

fn main() {
    // protoc: vendored-бінарник (без залежності від системного protobuf-compiler)
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");
    std::env::set_var("PROTOC", protoc);

    // Шлях до prro.proto — відносно кореня репозиторію Kasa.
    // CARGO_MANIFEST_DIR = frontend/src-tauri/crates/kasa-prro
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    // піднімаємось: kasa-prro -> crates -> src-tauri -> frontend -> kasa
    let manifest = PathBuf::from(&manifest_dir);
    let repo_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("repo root (kasa/)");
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
