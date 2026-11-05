use foto_importer::core::{
    jobs::Control,
    previews::{self, PreviewRequest},
};
#[test]
fn all_exif_orientations_preserve_expected_pixel_order() {
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(2, 3, |x, y| {
        image::Rgb([(1 + y * 2 + x) as u8, 0, 0])
    }));
    let expected = [
        vec![1, 2, 3, 4, 5, 6],
        vec![2, 1, 4, 3, 6, 5],
        vec![6, 5, 4, 3, 2, 1],
        vec![5, 6, 3, 4, 1, 2],
        vec![1, 3, 5, 2, 4, 6],
        vec![5, 3, 1, 6, 4, 2],
        vec![6, 4, 2, 5, 3, 1],
        vec![2, 4, 6, 1, 3, 5],
    ];
    for (orientation, expected) in expected.into_iter().enumerate() {
        let actual = previews::orient(image.clone(), orientation as u32 + 1);
        assert_eq!(
            actual.to_rgb8().pixels().map(|p| p[0]).collect::<Vec<_>>(),
            expected
        );
    }
}
#[test]
fn border_face_is_square_and_padded_instead_of_shifted() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("red.png");
    image::RgbImage::from_pixel(100, 60, image::Rgb([255, 0, 0])).save(&path)?;
    let p = previews::preview(
        PreviewRequest {
            root: Some(root.path().into()),
            path,
            orientation: 1,
            crop: Some([0.0, 0.0, 0.2, 0.3]),
            edge: 256,
            generation: 0,
        },
        "missing-exiftool",
        &Control::default(),
    )?;
    assert_eq!((p.width, p.height), (256, 256));
    assert_eq!(&p.rgba[..3], &[0, 0, 0]);
    Ok(())
}
#[test]
fn preview_cannot_escape_selected_root() -> anyhow::Result<()> {
    let a = tempfile::tempdir()?;
    let b = tempfile::tempdir()?;
    let path = b.path().join("outside.png");
    image::RgbImage::new(10, 10).save(&path)?;
    assert!(
        previews::preview(
            PreviewRequest {
                root: Some(a.path().into()),
                path,
                orientation: 1,
                crop: None,
                edge: 100,
                generation: 0
            },
            "unused",
            &Control::default()
        )
        .is_err()
    );
    Ok(())
}
