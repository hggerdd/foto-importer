//! Create isolated synthetic review data, never touch private collections.
use foto_importer::{
    config::Settings,
    core::{collection::Collection, jobs::Control},
};
use rusqlite::params;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let root = Path::new("tmp/demo");
    std::fs::create_dir_all(root.join("Quelle"))?;
    std::fs::create_dir_all(root.join("Importziel"))?;
    for i in 0u32..30 {
        let folder = root
            .join("Quelle")
            .join(if i % 2 == 0 { "Herbst" } else { "Berge" });
        std::fs::create_dir_all(&folder)?;
        let path = folder.join(format!("IMG_{:04}.jpg", i + 1));
        if path.exists() {
            continue;
        }
        let image = image::RgbImage::from_fn(800, 520, |x, y| {
            let horizon = 230.0 + 55.0 * ((x as f32 / 130.0 + i as f32).sin());
            let ridge = 330.0 + 65.0 * ((x as f32 / 190.0 + i as f32 * 0.4).sin());
            let t = y as f32 / 520.0;
            let color = if y as f32 > ridge {
                [22.0 + 15.0 * t, 53.0 + 30.0 * t, 55.0 + 20.0 * t]
            } else if y as f32 > horizon {
                [50.0 + i as f32 * 1.5, 85.0, 102.0]
            } else if (x as f32 - 590.0).powi(2) + (y as f32 - 135.0).powi(2) < 2300.0 {
                [251.0, 217.0, 151.0]
            } else {
                [
                    55.0 + 120.0 * t + (i % 4) as f32 * 12.0,
                    91.0 + 95.0 * t,
                    136.0 + 50.0 * t,
                ]
            };
            image::Rgb(color.map(|v| v.clamp(0.0, 255.0) as u8))
        });
        image.save(path)?;
    }
    let source = root.join("Quelle").canonicalize()?;
    let mut collection = Collection::open(&source, true)?;
    collection.quick_scan(false, &Control::default())?;
    let tx = collection.db.transaction()?;
    let files = tx
        .prepare("SELECT id FROM files ORDER BY id")?
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, file) in files.iter().enumerate() {
        tx.execute("INSERT OR REPLACE INTO meta(file_id,make,model,dt_original,lens,iso,width,height,orientation,json) VALUES(?,'Nikon','Nikon Z6',?,'24–70mm f/4',?,800,520,1,?)",params![file,format!("2026:10:{:02} 15:30:00",20+id%6),100+id%5*100,serde_json::json!({"Kamera":"Nikon Z6","Demo":"Synthetische Landschaft, kein privates Foto"}).to_string()])?;
        tx.execute("UPDATE files SET meta_state='done' WHERE id=?", [file])?;
    }
    tx.commit()?;
    let settings = Settings {
        source: source.to_string_lossy().into_owned(),
        collection: source.to_string_lossy().into_owned(),
        target: root
            .join("Importziel")
            .canonicalize()?
            .to_string_lossy()
            .into_owned(),
        model_dir: Path::new("models")
            .canonicalize()?
            .to_string_lossy()
            .into_owned(),
        onnx_runtime: Path::new("runtime/onnxruntime.dll")
            .canonicalize()?
            .to_string_lossy()
            .into_owned(),
        ..Default::default()
    };
    let settings_path = root.join("settings.json");
    std::fs::write(&settings_path, serde_json::to_vec_pretty(&settings)?)?;
    println!("{}", settings_path.canonicalize()?.display());
    Ok(())
}
