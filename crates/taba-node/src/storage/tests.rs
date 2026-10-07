use super::*;

#[test]
fn creates_the_directory_and_leaves_no_probe_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("a").join("taba");
    prepare_dir(&dir).unwrap();
    assert!(dir.is_dir());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    prepare_dir(&dir).expect("repetir é inofensivo");
}

#[test]
fn fails_when_the_path_is_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("taba");
    std::fs::write(&file, "x").unwrap();
    assert!(prepare_dir(&file).is_err());
}

#[test]
fn wipe_removes_the_contents_but_keeps_the_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("taba");
    std::fs::create_dir_all(dir.join("pasta")).unwrap();
    std::fs::write(dir.join("pasta").join("a.json"), "{}").unwrap();
    std::fs::write(dir.join("solto"), "x").unwrap();

    wipe_dir_contents(&dir).unwrap();

    assert!(dir.is_dir());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    wipe_dir_contents(&tmp.path().join("nao-existe")).unwrap();
}
