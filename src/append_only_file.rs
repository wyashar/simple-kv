use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};

pub struct AppendOnlyFile {
    buf_writer: BufWriter<File>,
    fsync_policy: FsyncPolcy,
}

#[derive(Debug, PartialEq, strum::VariantNames, strum::EnumString)]
#[strum(serialize_all = "PascalCase")]
pub enum FsyncPolcy {
    Always,
    EverySec,
    No,
}

impl Write for AppendOnlyFile {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
        self.buf_writer.write(buf)
    }

    // TODO: implement file sync policy! flush() only pushes the BufWriter's
    // contents to the OS; it does not fsync, so a crash can still lose data.
    fn flush(&mut self) -> Result<(), std::io::Error> {
        self.buf_writer.flush()
    }
}

impl AppendOnlyFile {
    pub fn open_or_create(
        file_path: &str,
        fsync_policy: FsyncPolcy,
    ) -> Result<AppendOnlyFile, std::io::Error> {
        let file: File = OpenOptions::new()
            .append(true)
            .create(true)
            .open(file_path)?;

        Ok(AppendOnlyFile {
            buf_writer: BufWriter::new(file),
            fsync_policy: fsync_policy,
        })
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
    fn creates_file_when_it_doesnt_exist_and_writes_to_it() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let file_path = file_path(&dir, "tmp.log");
        assert!(!Path::new(&file_path).exists());

        // The fsync policy is irrelevant to this test, so any variant works.
        let mut append_file: AppendOnlyFile =
            AppendOnlyFile::open_or_create(&file_path, FsyncPolcy::No)
                .expect("open_or_create should work");

        assert!(Path::new(&file_path).exists());
        append_file
            .write_all(b"some data")
            .expect("write should work here");
        append_file.flush().expect("flush should work here");

        let contents = std::fs::read(&file_path).expect("read should work");
        assert_eq!(contents, b"some data");
    }

    #[test]
    fn opens_existing_file_and_appends_without_truncating() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let file_path = file_path(&dir, "tmp.log");
        std::fs::write(&file_path, b"existing ").expect("seeding the file should work");

        // The fsync policy is irrelevant to this test, so any variant works.
        let mut append_file: AppendOnlyFile =
            AppendOnlyFile::open_or_create(&file_path, FsyncPolcy::No)
                .expect("open_or_create should work");

        append_file
            .write_all(b"data")
            .expect("write should work here");
        append_file.flush().expect("flush should work here");

        let contents = std::fs::read(&file_path).expect("read should work");
        assert_eq!(contents, b"existing data");
    }
}
