use foto_importer::core::{
    face_models,
    inference::{Detection, Models},
    jobs::Control,
};
use std::path::Path;

#[test]
#[ignore = "Requires locally installed models and ONNX Runtime; no downloads"]
fn real_yunet_and_sface_execute_on_synthetic_rgb() -> anyhow::Result<()> {
    let destination = tempfile::tempdir()?;
    let (folder, _) =
        face_models::install(destination.path(), &["models".into()], &Control::default())?;
    let mut models = Models::load(
        &folder,
        Path::new("runtime/onnxruntime.dll"),
        "yunet",
        "sface",
    )?;
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(112, 112, |x, y| {
        image::Rgb([(x * 2) as u8, (y * 2) as u8, ((x + y) % 256) as u8])
    }));
    let detection = Detection {
        bbox: [0.0, 0.0, 112.0, 112.0],
        landmarks: [
            [38.2946, 51.6963],
            [73.5318, 51.5014],
            [56.0252, 71.7366],
            [41.5493, 92.3655],
            [70.7299, 92.2041],
        ],
        score: 1.0,
    };
    let vector = models.embed(&image, &detection)?;
    assert_eq!(vector.len(), 128);
    assert!((vector.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 0.0001);
    let blank = image::DynamicImage::new_rgb8(960, 640);
    assert!(models.detect(&blank)?.is_empty());
    Ok(())
}

#[test]
#[ignore = "Downloads YuNet and SFace from OpenCV Zoo"]
fn downloads_and_installs_standard_models() -> anyhow::Result<()> {
    let destination = tempfile::tempdir()?;
    let (folder, models) = face_models::install(destination.path(), &[], &Control::default())?;
    assert!(face_models::weights_present(
        &models, &folder, "yunet", "sface"
    ));
    Ok(())
}
/// Genaueres Vergleichsmodell allein aus Landmarks (wie `strong::compute`); die Übereinstimmung
/// mit InsightFace steht in docs/validierung.md.
#[test]
#[ignore = "Requires models/weights/w600k_r50.onnx and ONNX Runtime (FOTO_IMPORTER_ORT); no downloads"]
fn real_strong_embedder_runs_from_landmarks() -> anyhow::Result<()> {
    let dll =
        std::env::var("FOTO_IMPORTER_ORT").unwrap_or_else(|_| "runtime/onnxruntime.dll".into());
    let mut embedder = foto_importer::core::inference::Embedder::load(
        Path::new("models"),
        Path::new(&dll),
        foto_importer::config::STRONG_EMBEDDER,
        2,
    )?;
    let image = image::RgbImage::from_fn(200, 160, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    let landmarks = [
        [78.0, 61.0],
        [113.0, 61.0],
        [96.0, 81.0],
        [81.0, 102.0],
        [110.0, 102.0],
    ];
    let vector = embedder.embed(&image, landmarks)?;
    assert_eq!(vector.len(), 512);
    assert!((vector.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 0.0001);
    // Dieselbe Eingabe ergibt denselben Vektor.
    assert_eq!(embedder.embed(&image, landmarks)?, vector);
    Ok(())
}

#[test]
#[ignore = "Requires locally installed optional models and ONNX Runtime; no downloads"]
fn real_scrfd_and_arcface_execute_on_synthetic_rgb() -> anyhow::Result<()> {
    let mut models = Models::load(
        Path::new("models"),
        Path::new("runtime/onnxruntime.dll"),
        "scrfd_10g",
        "arcface_buffalo_l",
    )?;
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(112, 112, |x, y| {
        image::Rgb([(x * 2) as u8, (y * 2) as u8, ((x + y) % 256) as u8])
    }));
    let detection = Detection {
        bbox: [0.0, 0.0, 112.0, 112.0],
        landmarks: [
            [38.2946, 51.6963],
            [73.5318, 51.5014],
            [56.0252, 71.7366],
            [41.5493, 92.3655],
            [70.7299, 92.2041],
        ],
        score: 1.0,
    };
    let vector = models.embed(&image, &detection)?;
    assert_eq!(vector.len(), 512);
    assert!((vector.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 0.0001);
    assert!(
        models
            .detect(&image::DynamicImage::new_rgb8(640, 640))?
            .is_empty()
    );
    Ok(())
}
