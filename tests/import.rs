use foto_importer::core::{files::{self,SourceFile},jobs::Control};
use std::{fs,path::Path};

#[test]
fn reject_windows_names_and_traversal(){for name in ["",".","..","CON","nul.jpg","LPT9","foo/bar","a\\b","test.","test ","bad:name"]{assert!(files::validate_folder(name).is_err(),"{name}");}assert!(files::validate_folder("2026-11 Herbst & Familie").is_ok());}
#[test]
fn parse_capture_dates_without_timezone_conversion(){assert_eq!(files::parse_date("2026:11:04 23:59:00+09:00").as_deref(),Some("2026-11-04"));assert_eq!(files::parse_date("20261104").as_deref(),Some("2026-11-04"));assert!(files::parse_date("2026:02:30").is_none());}
#[test]
fn repeated_import_preserves_sources_and_destinations()->anyhow::Result<()>{
    let temp=tempfile::tempdir()?;let source=temp.path().join("A.JPG");fs::write(&source,b"original-photo")?;let target=temp.path().join("archive");fs::create_dir(&target)?;let files=vec![SourceFile{path:source.clone(),day:"2026-11-04".into(),size:14}];
    for _ in 0..2{let report=files::import(&files,&target,"Herbst",None,&Control::default())?;assert_eq!(report.copied.len(),1);assert!(report.errors.is_empty());}
    assert_eq!(fs::read(&source)?,b"original-photo");assert_eq!(fs::read(target.join("Herbst/jpg/A.JPG"))?,b"original-photo");assert_eq!(fs::read(target.join("Herbst/jpg/A_1.JPG"))?,b"original-photo");Ok(())
}
#[test]
fn cancelled_copy_does_not_publish_output()->anyhow::Result<()>{let temp=tempfile::tempdir()?;let source=temp.path().join("test.jpg");fs::write(&source,b"data")?;let control=Control::default();control.stop();assert!(files::copy_one(&source,&temp.path().join("dest"),&control).is_err());assert!(!temp.path().join("dest/test.jpg").exists());Ok(())}
#[test]
fn scan_excludes_unsupported_files_and_groups_case_insensitively()->anyhow::Result<()>{let temp=tempfile::tempdir()?;fs::write(temp.path().join("photo.JPG"),b"not-a-jpeg")?;fs::write(temp.path().join("notes.txt"),b"ignore")?;let scan=files::scan(temp.path(),true,&Control::default())?;assert_eq!(scan.files.len(),1);assert_eq!(scan.files[0].path.file_name(),Path::new("photo.JPG").file_name());assert!(!scan.files[0].day.is_empty());Ok(())}
