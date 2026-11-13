//! Erzeugt eine isolierte Demo-Sammlung für die Zeitreise (ADR 0069): vier Personen mit
//! gezeichneten Gesichtern über mehrere Jahre, mit Lücken, kleinen Gesichtern und Fotos ohne
//! EXIF-Datum. Rein synthetisch; es wird nie eine private Sammlung gelesen. Die Gesichter
//! sind einfache Zeichnungen, keine Erkennungsergebnisse (Rahmen und Landmarks werden direkt
//! in die Datenbank geschrieben).
//!
//! `cargo run --release --example timeline_demo`, dann
//! `FOTO_IMPORTER_SETTINGS=tmp/zeitreise-demo/settings.json FOTO_IMPORTER_VIEW=time cargo run --release`.
use foto_importer::{
    config::Settings,
    core::{
        collection::Collection,
        faces::{self, NewPerson},
        relations::{self, Role},
        timeline,
    },
};
use image::{Rgb, RgbImage};
use rusqlite::params;
use std::path::Path;

const W: u32 = 1000;
const H: u32 = 750;

struct Look {
    name: &'static str,
    gender: &'static str,
    born: (i32, u32),
    skin: [u8; 3],
    hair: [u8; 3],
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32) / (1u64 << 31) as f32
    }
}

/// Zeichnet ein Gesicht und liefert Rahmen (Pixel) und Landmarks (Pixel).
fn draw(
    image: &mut RgbImage,
    center: (f32, f32),
    radius: f32,
    look: &Look,
    mood: f32,
) -> ([f32; 4], [[f32; 2]; 5]) {
    let (cx, cy) = center;
    let hair = look
        .hair
        .map(|c| (c as f32 + mood * 60.0).clamp(0.0, 255.0) as u8);
    for y in (cy - radius * 1.2) as u32..(cy + radius * 1.2).min(H as f32 - 1.0) as u32 {
        for x in (cx - radius).max(0.0) as u32..(cx + radius).min(W as f32 - 1.0) as u32 {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            if (dx / (radius * 0.82)).powi(2) + (dy / radius).powi(2) <= 1.0 {
                image.put_pixel(x, y, Rgb(if dy < -radius * 0.5 { hair } else { look.skin }));
            }
        }
    }
    let eye = (radius * 0.34, radius * 0.08);
    for side in [-1.0f32, 1.0] {
        let (ex, ey) = (cx + side * eye.0, cy - eye.1);
        let r = (radius * 0.09).max(3.0);
        for y in (ey - r) as u32..=(ey + r) as u32 {
            for x in (ex - r * 1.4) as u32..=(ex + r * 1.4) as u32 {
                image.put_pixel(x, y, Rgb([40, 40, 52]));
            }
        }
    }
    let mouth_y = cy + radius * 0.5;
    for x in (cx - radius * 0.26) as u32..=(cx + radius * 0.26) as u32 {
        for t in 0..(radius * 0.05).max(2.0) as u32 {
            image.put_pixel(x, mouth_y as u32 + t, Rgb([160, 70, 76]));
        }
    }
    let width = radius * 0.82;
    (
        [cx - width, cy - radius, cx + width, cy + radius],
        [
            [cx - eye.0, cy - eye.1],
            [cx + eye.0, cy - eye.1],
            [cx, cy + radius * 0.18],
            [cx - radius * 0.26, mouth_y],
            [cx + radius * 0.26, mouth_y],
        ],
    )
}

fn main() -> anyhow::Result<()> {
    let root = Path::new("tmp/zeitreise-demo");
    let source = root.join("Sammlung");
    std::fs::create_dir_all(&source)?;
    let source = source.canonicalize()?;
    let mut collection = Collection::open(&source, true)?;
    let looks = [
        Look {
            name: "Mia",
            gender: "weiblich",
            born: (2016, 3),
            skin: [244, 208, 180],
            hair: [150, 100, 60],
        },
        Look {
            name: "Ben",
            gender: "männlich",
            born: (2019, 7),
            skin: [236, 196, 164],
            hair: [90, 62, 40],
        },
        Look {
            name: "Anna",
            gender: "weiblich",
            born: (1985, 5),
            skin: [240, 204, 176],
            hair: [110, 70, 40],
        },
        Look {
            name: "Jonas",
            gender: "männlich",
            born: (1983, 9),
            skin: [222, 176, 140],
            hair: [40, 36, 36],
        },
    ];
    let mut ids = Vec::new();
    for look in &looks {
        let id = faces::add_person(
            &mut collection,
            &NewPerson {
                name: look.name.into(),
                gender: look.gender.into(),
                birth_year: Some(look.born.0),
                note: String::new(),
            },
        )?;
        // Geburtsmonat nur für die Kinder: zeigt Alter auf Monate und den Rückfall auf Jahre.
        if look.born.0 > 2000 {
            timeline::set_birth_month(&collection, id, Some(look.born.1 as u8))?;
        }
        ids.push(id);
    }
    for parent in [2, 3] {
        for child in [0, 1] {
            relations::add(&mut collection, ids[parent], Role::Parent, ids[child])?;
        }
    }
    relations::add(&mut collection, ids[2], Role::Partner, ids[3])?;

    let mut rng = Rng(7);
    let mut next_id = 1i64;
    let tx = collection.db.transaction()?;
    // (Person, Jahr, Monat, Datum im EXIF?, Gesichtsradius-Faktor)
    let mut shots: Vec<(usize, i32, u32, bool, f32)> = Vec::new();
    for (index, look) in looks.iter().enumerate() {
        let (born_year, born_month) = look.born;
        let first = if born_year > 2000 { born_year } else { 2012 };
        for year in first..=2026 {
            let months: Vec<u32> = if born_year > 2000 && year <= born_year + 2 {
                (1..=12)
                    .filter(|m| (year > born_year || *m >= born_month) && m % 2 == 1)
                    .collect()
            } else if born_year > 2000 {
                vec![3, 8]
            } else {
                vec![6]
            };
            for month in months {
                // Lücke: Mia hat 2021 nur kleine Gesichter, Ben 2023 gar keine Fotos.
                if index == 1 && year == 2023 {
                    continue;
                }
                let exif = !(year == 2018 && index == 0);
                let small = index == 0 && year == 2021;
                shots.push((index, year, month, exif, if small { 0.3 } else { 1.0 }));
            }
        }
    }
    for (index, year, month, exif, factor) in shots {
        let look = &looks[index];
        let age = (year - look.born.0) as f32 + (month as f32 - look.born.1 as f32) / 12.0;
        // Köpfe wachsen bis etwa 16 Jahre; nie unter 155 px Rahmenbreite außer „klein“.
        let radius = (118.0 + 8.5 * age.clamp(0.0, 16.0)) * factor;
        let folder = if exif {
            year.to_string()
        } else {
            format!("{year} Urlaub")
        };
        let dir = source.join(&folder);
        std::fs::create_dir_all(&dir)?;
        let name = format!("{}_{year}-{month:02}.jpg", look.name);
        let relpath = format!("{folder}/{name}");
        let mut image = RgbImage::from_fn(W, H, |x, y| {
            let t = y as f32 / H as f32;
            let bump = (x / 40 % 2) as u8 * 6;
            Rgb([
                ((60.0 + 90.0 * t + (month * 7) as f32) as u8).saturating_add(bump),
                ((110.0 + 70.0 * t) as u8).saturating_add(bump),
                ((150.0 + 50.0 * (1.0 - t) + (year % 5) as f32 * 8.0) as u8).saturating_add(bump),
            ])
        });
        let center = (
            W as f32 / 2.0 + (rng.next() - 0.5) * 80.0,
            H as f32 / 2.0 + (rng.next() - 0.5) * 40.0,
        );
        let (bbox, landmarks) = draw(&mut image, center, radius, look, rng.next());
        image.save(dir.join(&name))?;
        let id = next_id;
        next_id += 1;
        let date = format!("{year}:{month:02}:{:02} 15:00:00", 3 + (id % 24));
        tx.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [id])?;
        tx.execute(
            "INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status,meta_state) VALUES(?,?,?,?,?,?,'jpg',1,?,?,'present','done')",
            params![id, relpath, relpath.to_lowercase(), folder, name, name.trim_end_matches(".jpg"), 1_700_000_000_000_000_000i64, id],
        )?;
        tx.execute(
            "INSERT INTO meta(file_id,dt_original,width,height,orientation) VALUES(?,?,?,?,1)",
            params![id, exif.then_some(date.as_str()), W, H],
        )?;
        let norm = [
            bbox[0] / W as f32,
            bbox[1] / H as f32,
            bbox[2] / W as f32,
            bbox[3] / H as f32,
        ];
        tx.execute(
            "INSERT INTO faces(photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,det_score,quality,person_id,assignment,captured_at) VALUES(?,?,?,'demo-detector',?,?,?,?,?,0.95,?,?,'user',?)",
            params![
                id, id, relpath, norm[0], norm[1], norm[2], norm[3],
                serde_json::to_string(&landmarks)?,
                0.7 + rng.next() * 0.25,
                ids[index],
                exif.then_some(date.as_str()),
            ],
        )?;
    }
    tx.commit()?;
    let settings = Settings {
        collection: source.to_string_lossy().into_owned(),
        central_enabled: false,
        ..Default::default()
    };
    let settings_path = root.join("settings.json");
    std::fs::write(&settings_path, serde_json::to_vec_pretty(&settings)?)?;
    println!("{}", settings_path.canonicalize()?.display());
    Ok(())
}
