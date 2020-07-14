use std::convert::TryFrom;
use std::fs;
use tempdir::TempDir;
use ya_agreement_utils::AgreementView;

#[test]
fn test_parsing() -> anyhow::Result<()> {
    let dir = TempDir::new("test1")?;
    let agrement_json = include_str!("agreement-9ce65424.json");
    let file = dir.path().join("agreement-9ce65424.json");
    fs::write(&file, agrement_json)?;
    eprintln!("file = {}", file.display());

    let _view = AgreementView::try_from(&file)?;
    Ok(())
}
