use foto_importer::core::inference::{Detection, Models};
use std::path::Path;

#[test]
#[ignore = "Requires locally installed models and ONNX Runtime; no downloads"]
fn real_yunet_and_sface_execute_on_synthetic_rgb() -> anyhow::Result<()> {
    let mut models = Models::load(
        Path::new("models"),
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
    let blank = image::DynamicImage::new_rgb8(640, 640);
    assert!(models.detect(&blank)?.is_empty());
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
