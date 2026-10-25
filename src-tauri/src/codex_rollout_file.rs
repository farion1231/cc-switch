//! Codex rollout 文件的两种形态：`rollout-*.jsonl` 与压缩后的 `rollout-*.jsonl.zst`。
//!
//! Codex 的 `[features] local_thread_store_compression`（以及不受该开关控制的
//! app-server `rollout/compress`）会把 7 天没动过的 rollout 压成同名 `.jsonl.zst`
//! （单帧 zstd、保留 mtime）并删掉原文件；在压缩过的会话上继续对话时，Codex
//! 先解压回 `.jsonl` 再追加。会话管理、用量导入、历史分桶迁移都经这里读写，
//! 两种形态对它们透明。

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use crate::config::atomic_write;
use crate::error::AppError;

const COMPRESSED_SUFFIX: &str = ".zst";
const PLAIN_SUFFIX: &str = ".jsonl";
const READER_CAPACITY: usize = 1 << 20;
/// 与 Codex 压缩时用的级别无关，只用于 CC Switch 原地改写压缩文件（历史分桶迁移）。
const WRITE_LEVEL: i32 = 3;

fn file_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(|name| name.to_str())
}

/// 是否为压缩形态（`*.jsonl.zst`）。
pub fn is_compressed(path: &Path) -> bool {
    file_name(path).is_some_and(|name| {
        name.strip_suffix(COMPRESSED_SUFFIX)
            .is_some_and(|plain| plain.ends_with(PLAIN_SUFFIX))
    })
}

/// 是否为 rollout 文件（`*.jsonl` 或 `*.jsonl.zst`），只看扩展名。
pub fn is_rollout_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") || is_compressed(path)
}

/// 去掉 `.zst` 后的普通形态路径；普通路径原样返回。
pub fn logical_path(path: &Path) -> PathBuf {
    match file_name(path)
        .filter(|_| is_compressed(path))
        .and_then(|name| name.strip_suffix(COMPRESSED_SUFFIX))
    {
        Some(plain) => path.with_file_name(plain),
        None => path.to_path_buf(),
    }
}

/// 另一种形态的路径：`x.jsonl` ↔ `x.jsonl.zst`。不是 rollout 文件时返回 `None`。
pub fn sibling(path: &Path) -> Option<PathBuf> {
    if is_compressed(path) {
        return Some(logical_path(path));
    }
    if !is_rollout_file(path) {
        return None;
    }
    let name = file_name(path)?;
    Some(path.with_file_name(format!("{name}{COMPRESSED_SUFFIX}")))
}

/// 按行读取，压缩文件透明解压。
pub fn open_reader(path: &Path) -> io::Result<Box<dyn BufRead + Send>> {
    reader_from_file(path, File::open(path)?)
}

/// 同 [`open_reader`]，用调用方已打开的文件（调用方还要从同一句柄取元数据时用）。
pub fn reader_from_file(path: &Path, file: File) -> io::Result<Box<dyn BufRead + Send>> {
    if is_compressed(path) {
        let decoder = zstd::stream::read::Decoder::new(file)?;
        Ok(Box::new(BufReader::with_capacity(READER_CAPACITY, decoder)))
    } else {
        Ok(Box::new(BufReader::with_capacity(READER_CAPACITY, file)))
    }
}

/// 读出全文（UTF-8），压缩文件先解压。
pub fn read_to_string(path: &Path) -> io::Result<String> {
    if !is_compressed(path) {
        return std::fs::read_to_string(path);
    }
    let mut text = String::new();
    open_reader(path)?.read_to_string(&mut text)?;
    Ok(text)
}

/// 原子写回；压缩路径先编码成 zstd，保持文件原有形态。
///
/// 压缩形态还保留原 mtime：会话列表不解压结尾，压缩会话的最后活跃时间就是 mtime
/// （Codex 压缩时也保留了它），改写元数据不该让老会话显示成刚刚活跃。
pub fn write_rollout(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    if !is_compressed(path) {
        return atomic_write(path, bytes);
    }
    let modified = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok();
    let encoded =
        zstd::stream::encode_all(bytes, WRITE_LEVEL).map_err(|error| AppError::io(path, error))?;
    atomic_write(path, &encoded)?;
    if let Some(modified) = modified {
        File::options()
            .write(true)
            .open(path)
            .and_then(|file| file.set_times(std::fs::FileTimes::new().set_modified(modified)))
            .map_err(|error| AppError::io(path, error))?;
    }
    Ok(())
}

/// 逻辑文件名（去掉 `.zst`）。`path` 可以是任意平台的路径字符串（游标里存的原样）。
pub fn logical_file_name(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.strip_suffix(COMPRESSED_SUFFIX) {
        Some(plain) if plain.ends_with(PLAIN_SUFFIX) => plain,
        _ => name,
    }
}

/// 同一逻辑名的两种形态同时存在时（压缩或解压的瞬间）只留普通形态：
/// 它是 Codex 正在写的那份，内容不少于压缩那份。保持原有顺序。
pub fn dedupe_siblings(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let plain: HashSet<PathBuf> = paths
        .iter()
        .filter(|path| !is_compressed(path))
        .cloned()
        .collect();
    paths
        .into_iter()
        .filter(|path| !is_compressed(path) || !plain.contains(&logical_path(path)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_both_forms() {
        let plain = Path::new("/x/rollout-a.jsonl");
        let zst = Path::new("/x/rollout-a.jsonl.zst");
        assert!(is_rollout_file(plain));
        assert!(is_rollout_file(zst));
        assert!(!is_compressed(plain));
        assert!(is_compressed(zst));
        assert!(!is_rollout_file(Path::new("/x/rollout-a.zst")));
        assert!(!is_rollout_file(Path::new("/x/notes.txt")));
        assert!(!is_compressed(Path::new("/x/rollout-a.zst")));
    }

    #[test]
    fn logical_and_sibling_paths() {
        let plain = PathBuf::from("/x/rollout-a.jsonl");
        let zst = PathBuf::from("/x/rollout-a.jsonl.zst");
        assert_eq!(logical_path(&zst), plain);
        assert_eq!(logical_path(&plain), plain);
        assert_eq!(sibling(&plain), Some(zst.clone()));
        assert_eq!(sibling(&zst), Some(plain));
        assert_eq!(sibling(Path::new("/x/a.txt")), None);
    }

    #[test]
    fn logical_file_name_handles_both_separators() {
        assert_eq!(
            logical_file_name("/a/b/rollout-x.jsonl.zst"),
            "rollout-x.jsonl"
        );
        assert_eq!(
            logical_file_name("C:\\a\\rollout-x.jsonl"),
            "rollout-x.jsonl"
        );
        assert_eq!(logical_file_name("rollout-x.zst"), "rollout-x.zst");
    }

    #[test]
    fn dedupe_keeps_plain_and_order() {
        let paths = vec![
            PathBuf::from("/x/b.jsonl.zst"),
            PathBuf::from("/x/a.jsonl.zst"),
            PathBuf::from("/x/a.jsonl"),
            PathBuf::from("/y/a.jsonl.zst"),
        ];
        assert_eq!(
            dedupe_siblings(paths),
            vec![
                PathBuf::from("/x/b.jsonl.zst"),
                PathBuf::from("/x/a.jsonl"),
                PathBuf::from("/y/a.jsonl.zst"),
            ]
        );
    }

    #[test]
    fn rewriting_compressed_rollout_keeps_mtime() {
        let temp = tempfile::tempdir().expect("tempdir");
        let zst = temp.path().join("rollout-a.jsonl.zst");
        write_rollout(&zst, b"{\"a\":1}\n").expect("write");
        let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        File::options()
            .write(true)
            .open(&zst)
            .expect("open")
            .set_times(std::fs::FileTimes::new().set_modified(old))
            .expect("set mtime");

        write_rollout(&zst, b"{\"a\":2}\n").expect("rewrite");

        let modified = std::fs::metadata(&zst)
            .expect("meta")
            .modified()
            .expect("mtime");
        assert_eq!(modified, old);
        assert_eq!(read_to_string(&zst).expect("read"), "{\"a\":2}\n");
    }

    #[test]
    fn read_and_write_round_trip() {
        let temp = tempfile::tempdir().expect("tempdir");
        let zst = temp.path().join("rollout-a.jsonl.zst");
        let plain = temp.path().join("rollout-b.jsonl");
        let text = "{\"a\":1}\n{\"b\":2}\n";

        write_rollout(&zst, text.as_bytes()).expect("write zst");
        write_rollout(&plain, text.as_bytes()).expect("write plain");

        let raw = std::fs::read(&zst).expect("raw");
        assert_eq!(&raw[..4], &[0x28, 0xB5, 0x2F, 0xFD], "zstd magic");
        assert_eq!(read_to_string(&zst).expect("read zst"), text);
        assert_eq!(read_to_string(&plain).expect("read plain"), text);
        let lines: Vec<String> = open_reader(&zst)
            .expect("reader")
            .lines()
            .collect::<Result<_, _>>()
            .expect("lines");
        assert_eq!(lines, vec!["{\"a\":1}", "{\"b\":2}"]);
    }
}
