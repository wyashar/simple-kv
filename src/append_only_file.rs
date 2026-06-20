use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};

pub struct AppendOnlyFile {
    buf_writer: BufWriter<File>,
}

impl AppendOnlyFile {
    pub fn open_file(file_path: &str) -> Result<AppendOnlyFile, std::io::Error> {
        let file: File = OpenOptions::new().append(true).open(file_path)?;

        Ok(AppendOnlyFile {
            buf_writer: BufWriter::new(file),
        })
    }

    pub fn create_file(file_path: &str) -> Result<AppendOnlyFile, std::io::Error> {
        let file: File = OpenOptions::new()
            .append(true)
            .create_new(true)
            .open(file_path)?;

        Ok(AppendOnlyFile {
            buf_writer: BufWriter::new(file),
        })
    }

    // TODO: implement file sync policy!
    // TODO: Flushing on every input is kinda bad. Also, we never sync, which is also bad.
    pub fn append(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        self.buf_writer.write_all(data)?;
        self.buf_writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    /// Path to a file named `name` inside `dir`, as a `String` so it can be
    /// passed to the `&str` APIs under test.
    fn file_path(dir: &TempDir, name: &str) -> String {
        dir.path()
            .join(name)
            .to_str()
            .expect("temp path should be valid utf-8")
            .to_string()
    }

    #[test]
    fn create_file_and_write_to_it() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let file_path = file_path(&dir, "tmp.log");
        let append_file_res: Result<AppendOnlyFile, std::io::Error> =
            AppendOnlyFile::create_file(&file_path);
        assert!(matches!(append_file_res, Ok(_)));
        let mut append_file: AppendOnlyFile = append_file_res.unwrap();

        let path: &Path = Path::new(&file_path);
        assert!(path.exists());
        append_file
            .append(b"some data")
            .expect("write should work here");

        let contents = std::fs::read(&file_path).expect("read should work");
        assert_eq!(contents, b"some data");
    }

    #[test]
    fn create_file_fails_when_it_already_exists() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let file_path = file_path(&dir, "tmp.log");
        File::create(&file_path).expect("file creation works");
        let append_file_res: Result<AppendOnlyFile, std::io::Error> =
            AppendOnlyFile::create_file(&file_path);
        assert!(matches!(append_file_res, Err(_)));
    }

    #[test]
    fn open_file_and_write_to_it() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let file_path = file_path(&dir, "tmp.log");
        File::create(&file_path).expect("file creationg works");
        let append_file_res: Result<AppendOnlyFile, std::io::Error> =
            AppendOnlyFile::open_file(&file_path);
        assert!(matches!(append_file_res, Ok(_)));
        let mut append_file: AppendOnlyFile = append_file_res.unwrap();

        let path: &Path = Path::new(&file_path);
        assert!(path.exists());
        append_file
            .append(b"some data")
            .expect("write should work here");

        let contents = std::fs::read(&file_path).expect("read should work");
        assert_eq!(contents, b"some data");
    }

    #[test]
    fn open_file_fails_when_it_doesnt_exist() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let file_path = file_path(&dir, "tmp.log");
        let append_file_res: Result<AppendOnlyFile, std::io::Error> =
            AppendOnlyFile::open_file(&file_path);
        assert!(matches!(append_file_res, Err(_)));
    }
}
