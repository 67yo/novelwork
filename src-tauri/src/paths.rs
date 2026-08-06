use std::fs;
use std::path::PathBuf;

pub fn app_root() -> PathBuf {
    let root = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("novework");
    fs::create_dir_all(&root).ok();
    root
}

pub fn db_path() -> PathBuf {
    app_root().join("nove.db")
}

pub fn lancedb_dir() -> PathBuf {
    let p = app_root().join("lancedb");
    fs::create_dir_all(&p).ok();
    p
}

pub fn novels_dir() -> PathBuf {
    let p = app_root().join("novels");
    fs::create_dir_all(&p).ok();
    p
}

pub fn novel_dir(novel_id: &str) -> PathBuf {
    let p = novels_dir().join(novel_id);
    fs::create_dir_all(&p).ok();
    fs::create_dir_all(p.join("chapters")).ok();
    p
}

pub fn novel_meta_path(novel_id: &str) -> PathBuf {
    novel_dir(novel_id).join("meta.json")
}

pub fn novel_tree_path(novel_id: &str) -> PathBuf {
    novel_dir(novel_id).join("tree.json")
}

pub fn chapter_path(novel_id: &str, node_id: &str) -> PathBuf {
    novel_dir(novel_id)
        .join("chapters")
        .join(format!("{node_id}.md"))
}
