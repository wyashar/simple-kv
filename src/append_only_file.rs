use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;

pub struct AppendOnlyFile {
    buf_writer: BufWriter<File>,
}

impl AppendOnlyFile {
    pub fn open<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?;

        Ok(Self {
            buf_writer: BufWriter::new(file),
        })
    }

    pub fn append(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.buf_writer.write_all(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn file_path(dir: &TempDir, name: &str) -> PathBuf {
        dir.path().join(name)
    }

    #[test]
    fn creates_file_when_it_does_not_exist() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let path = file_path(&dir, "tmp.log");
        assert!(!path.exists());

        AppendOnlyFile::open(&path).expect("open should create the file");

        assert!(path.exists());
        let contents = std::fs::read(&path).expect("read should work");
        assert!(contents.is_empty());
    }

    #[test]
    fn opens_existing_file_without_truncating() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let path = file_path(&dir, "tmp.log");
        std::fs::write(&path, b"existing").expect("seeding the file should work");

        AppendOnlyFile::open(&path).expect("open should succeed for an existing file");

        let contents = std::fs::read(&path).expect("read should work");
        assert_eq!(contents, b"existing");
    }

    #[test]
    fn append_writes_bytes_to_the_file() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let path = file_path(&dir, "tmp.log");

        {
            let mut aof =
                AppendOnlyFile::open(&path).expect("open should create the file");
            aof.append(b"some data").expect("append should work");
        }

        let contents = std::fs::read(&path).expect("read should work");
        assert_eq!(contents, b"some data");
    }

    #[test]
    fn append_does_not_truncate_existing_contents() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let path = file_path(&dir, "tmp.log");
        std::fs::write(&path, b"existing ").expect("seeding the file should work");

        {
            let mut aof = AppendOnlyFile::open(&path)
                .expect("open should succeed for an existing file");
            aof.append(b"data").expect("append should work");
        }

        let contents = std::fs::read(&path).expect("read should work");
        assert_eq!(contents, b"existing data");
    }
}
