use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

pub struct AppendOnlyFile {
    file: File,
}

impl AppendOnlyFile {
    pub fn open<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(path)?;

        Ok(Self { file })
    }

    pub fn append(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.file.write_all(bytes)
    }

    pub fn get_file_content_from_start(&mut self) -> std::io::Result<&File> {
        self.file.seek(SeekFrom::Start(0))?;
        Ok(&self.file)
    }

    pub fn try_clone(&self) -> std::io::Result<Self> {
        Ok(Self {
            file: self.file.try_clone()?,
        })
    }

    // TODO: in the future, we should handle a more robust sync mechanism
    // TODO: we need to decide what the server will do in the event that the sync fails
    // TODO: for now, we will just panic if the sync fails
    pub fn sync(&self) {
        self.file.sync_all().expect("file sync should work");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
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
            let mut aof = AppendOnlyFile::open(&path).expect("open should create the file");
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
            let mut aof =
                AppendOnlyFile::open(&path).expect("open should succeed for an existing file");
            aof.append(b"data").expect("append should work");
        }

        let contents = std::fs::read(&path).expect("read should work");
        assert_eq!(contents, b"existing data");
    }

    #[test]
    fn reader_reads_appended_data_from_start() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let path = file_path(&dir, "tmp.log");
        let mut aof = AppendOnlyFile::open(path).expect("open should work");
        aof.append(b"pending data").expect("append should work");

        let mut contents = Vec::new();
        aof.get_file_content_from_start()
            .expect("reader creation should work")
            .read_to_end(&mut contents)
            .expect("read should work");

        assert_eq!(contents, b"pending data");
    }
}
