#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
#[test]
#[ignore = "Lädt das offizielle Microsoft-Paket über das Netzwerk"]
fn downloads_and_loads_official_windows_runtime() -> anyhow::Result<()> {
    use foto_importer::core::{jobs::Control, onnx_runtime};

    let destination = tempfile::tempdir()?;
    let path = onnx_runtime::download(destination.path(), &Control::default())?;
    assert!(
        path.with_file_name("onnxruntime_providers_shared.dll")
            .is_file()
    );
    ort::init_from(path.to_string_lossy()).commit()?;
    let _session = ort::session::Session::builder()?;
    Ok(())
}
