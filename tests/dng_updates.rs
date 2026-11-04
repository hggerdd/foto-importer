use foto_importer::core::dng_updates::{self, UpdateCache};
#[test]
fn versions_compare_numerically() -> anyhow::Result<()> {
    assert!(dng_updates::version("17.10")? > dng_updates::version("17.9.9")?);
    assert_eq!(
        dng_updates::version("17.1")?,
        dng_updates::version("17.1.0.0")?
    );
    assert!(dng_updates::version("17.beta").is_err());
    Ok(())
}
#[test]
fn only_expected_adobe_installer_links_are_accepted() -> anyhow::Result<()> {
    assert_eq!(
        dng_updates::from_download_url(
            "https://download.adobe.com/pub/adobe/dng/win/AdobeDNGConverter_x64_17_4_1.exe"
        )?,
        "17.4.1"
    );
    assert!(
        dng_updates::from_download_url(
            "https://download.adobe.com.evil.test/pub/adobe/dng/win/AdobeDNGConverter_x64_17_4.exe"
        )
        .is_err()
    );
    assert!(
        dng_updates::from_download_url(
            "http://download.adobe.com/pub/adobe/dng/win/AdobeDNGConverter_x64_17_4.exe"
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn same_day_failure_is_cached_without_network() {
    let cache = UpdateCache {
        date: chrono::Local::now().date_naive().to_string(),
        latest: None,
        error: Some("offline".into()),
    };
    let result = dng_updates::check(cache);
    assert_eq!(result.error.as_deref(), Some("offline"));
}
