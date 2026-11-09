//! Erzeugt unter `tmp/central-demo` zwei synthetische Sammlungen A und B, eine temporäre
//! Zentraldatei und Einstellungen für Sichtprüfungen des zentralen Beziehungsabgleichs (ADR 0065).
//! Die Zentraldatei liegt im Demo-Ordner, nie im Benutzerprofil. Nur erfundene Namen.
//!
//! `cargo run -j 1 --example central_demo`, dann zum Beispiel
//! `FOTO_IMPORTER_SETTINGS=tmp/central-demo/settings.json FOTO_IMPORTER_VIEW=people
//! FOTO_IMPORTER_FACE_MODE=persons FOTO_IMPORTER_ACTION=central_sync cargo run`.
use foto_importer::{
    config::Settings,
    core::{
        central::{self, Central, CentralConfig},
        collection::Collection,
        faces::{self, NewPerson},
        relations::{self, Role},
    },
};
use std::path::Path;

fn person(name: &str, gender: &str) -> NewPerson {
    NewPerson {
        name: name.into(),
        gender: gender.into(),
        ..Default::default()
    }
}
fn id(c: &Collection, name: &str) -> anyhow::Result<i64> {
    faces::people(c)?
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.id)
        .ok_or_else(|| anyhow::anyhow!("{name} fehlt"))
}

fn main() -> anyhow::Result<()> {
    let root = Path::new("tmp/central-demo");
    if root.exists() {
        std::fs::remove_dir_all(root)?;
    }
    let (pa, pb) = (root.join("a"), root.join("b"));
    std::fs::create_dir_all(&pa)?;
    std::fs::create_dir_all(&pb)?;
    let config = CentralConfig {
        enabled: true,
        path: std::env::current_dir()?.join(root).join("central.sqlite"),
        images: false,
        relations: true,
    };
    let (mut a, mut b) = (Collection::open(&pa, true)?, Collection::open(&pb, true)?);
    for (name, gender) in [
        ("Anna", "weiblich"),
        ("Ben", "männlich"),
        ("Clara", "weiblich"),
        ("Dora", "weiblich"),
    ] {
        central::add_person_synced(&mut a, &config, &person(name, gender))?;
    }
    let report = central::sync(&mut b, &mut Central::open_config(&config)?)?;
    let gids: Vec<String> = report.missing.iter().map(|p| p.gid.clone()).collect();
    central::adopt(&mut b, &Central::open_config(&config)?, &gids)?;
    central::sync(&mut a, &mut Central::open_config(&config)?)?;
    central::sync(&mut b, &mut Central::open_config(&config)?)?;
    // Zentral (über A): Anna und Ben sind Partner, Clara ist Kind von Anna, Dora Freundin von Anna.
    let (anna, ben, clara, dora) = (
        id(&a, "Anna")?,
        id(&a, "Ben")?,
        id(&a, "Clara")?,
        id(&a, "Dora")?,
    );
    central::add_relation_synced(&mut a, &config, anna, Role::Partner, ben)?;
    central::add_relation_synced(&mut a, &config, anna, Role::Parent, clara)?;
    central::add_relation_synced(&mut a, &config, anna, Role::Friend, dora)?;
    // Dora/Anna wird in A wieder gelöscht (Grabstein); B hat sie noch lokal.
    central::sync(&mut b, &mut Central::open_config(&config)?)?;
    central::remove_relation_synced(&a, &config, anna, Role::Friend, dora)?;
    // B widerspricht lokal: Anna ist Mutter von Ben (zentral: Partner); Clara Elternteil von Anna.
    let (banna, bben, bclara) = (id(&b, "Anna")?, id(&b, "Ben")?, id(&b, "Clara")?);
    relations::remove(&b, banna, Role::Partner, bben)?;
    relations::add(&mut b, banna, Role::Parent, bben)?;
    relations::remove(&b, banna, Role::Parent, bclara)?;
    relations::add(&mut b, bclara, Role::Parent, banna)?;
    let settings = Settings {
        collection: std::env::current_dir()?
            .join(&pb)
            .to_string_lossy()
            .into_owned(),
        central_enabled: true,
        central_path: config.path.to_string_lossy().into_owned(),
        central_relations: true,
        ..Settings::default()
    };
    std::fs::write(
        root.join("settings.json"),
        serde_json::to_vec_pretty(&settings)?,
    )?;
    println!("Demo bereit: {}", root.join("settings.json").display());
    Ok(())
}
