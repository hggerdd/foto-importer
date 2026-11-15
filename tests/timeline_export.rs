#![allow(clippy::unwrap_used)]
//! Export der Zeitreise (ADR 0069): schreibt nur neue Dateien und ersetzt nie eine bestehende.
use foto_importer::core::{
    collection::Collection,
    jobs::Control,
    timeline::{self, Axis, Grain, Params},
    timeline_export::{self, Tile},
};
use rusqlite::params;

fn seed(c: &Collection, root: &std::path::Path) -> anyhow::Result<()> {
    c.db.execute("INSERT INTO persons(id) VALUES(1)", [])?;
    c.db.execute(
        "INSERT INTO person_attrs VALUES(1,'name','Anna Beispiel')",
        [],
    )?;
    for (id, year) in [(1, 2015), (2, 2017), (3, 2020)] {
        let rel = format!("{year}/{id}.jpg");
        std::fs::create_dir_all(root.join(year.to_string()))?;
        let image = image::RgbImage::from_fn(800, 600, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 120])
        });
        image.save(root.join(&rel))?;
        c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [id])?;
        c.db.execute("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status,meta_state) VALUES(?,?,?,?,?,?,'jpg',1,0,?,'present','done')",
            params![id, rel, rel, year.to_string(), format!("{id}.jpg"), id.to_string(), id])?;
        c.db.execute(
            "INSERT INTO meta(file_id,dt_original,width,height,orientation) VALUES(?,?,800,600,1)",
            params![id, format!("{year}:06:01 12:00:00")],
        )?;
        c.db.execute("INSERT INTO faces(id,photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,det_score,quality,person_id,assignment) VALUES(?,?,?,?,'d',0.3,0.2,0.7,0.8,'[[300.0,200.0],[400.0,200.0],[350.0,260.0],[310.0,320.0],[390.0,320.0]]',0.95,0.9,1,'user')",
            params![id, id, id, rel])?;
    }
    Ok(())
}

#[test]
fn exports_are_new_files_and_never_replace_existing_ones() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    seed(&c, temp.path())?;
    let faces = timeline::load(&c, 1)?;
    let computed = timeline::compute(
        &faces,
        &Params {
            axis: Axis::Date,
            grain: Some(Grain::Year),
            thresholds: timeline::Thresholds::with_min_px(96),
            ..Default::default()
        },
    );
    let tiles: Vec<Tile> = computed
        .sections
        .iter()
        .map(|s| Tile {
            label: timeline_export::plain_label(computed.axis, computed.grain, s.ord),
            face: computed.face(s.best.unwrap()).clone(),
        })
        .collect();
    assert_eq!(tiles.len(), 3);
    let control = Control::default();
    let out = tempfile::tempdir()?;
    let image = timeline_export::collage(temp.path(), &tiles, 2, 96, true, "", &control)?;
    assert_eq!(image.width(), 8 + 2 * (96 + 8));
    let first = timeline_export::save_png(&image, out.path(), "Collage")?;
    let second = timeline_export::save_png(&image, out.path(), "Collage")?;
    assert_ne!(first, second);
    assert!(first.ends_with("Collage.png") && second.ends_with("Collage-2.png"));
    let frames: Vec<_> = tiles
        .iter()
        .map(|t| timeline_export::render_face(temp.path(), &t.face, 64, true, "", &control))
        .collect::<anyhow::Result<_>>()?;
    let gif = timeline_export::timelapse_gif(&frames, 4.0, out.path(), "Raffer")?;
    assert!(std::fs::metadata(&gif)?.len() > 0);
    let folder = timeline_export::timelapse_sequence(&frames, out.path(), "Raffer")?;
    assert_eq!(std::fs::read_dir(&folder)?.count(), 3);
    let again = timeline_export::timelapse_sequence(&frames, out.path(), "Raffer")?;
    assert_ne!(folder, again);
    let rows = vec![(
        "2015".to_owned(),
        vec![tiles[0].face.clone(), tiles[1].face.clone()],
    )];
    let sheet = timeline_export::contact_sheet(temp.path(), &rows, 64, "", &control)?;
    assert!(sheet.width() > 64);
    assert_eq!(timeline_export::safe_stem("Anna / B: ä"), "Anna___B__ä");
    assert!(timeline_export::collage(temp.path(), &[], 2, 64, false, "", &control).is_err());
    Ok(())
}
