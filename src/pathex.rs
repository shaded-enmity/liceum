use std;
use std::path::{Path, PathBuf};

/// Return a new path buffer containing the absolute, possibly cannonicalized, version
/// of this path buffer.
pub trait AbsolutePath {
    fn absolute_path(&self, canonicalize: bool) -> PathBuf;
    fn canonical_path(&self) -> PathBuf {
        self.absolute_path(true)
    }
}

impl AbsolutePath for Path {
    fn absolute_path(&self, canonicalize: bool) -> PathBuf {
        let mut absolute_path = std::env::current_dir().unwrap();
        absolute_path.push(self);

        if canonicalize {
            let mut buf = PathBuf::new();

            for c in absolute_path.components() {
                let strref = c.as_ref();

                if strref == "." {
                    continue;
                } else if strref == ".." {
                    buf.pop();
                } else {
                    buf.push(c.as_ref());
                }
            }

            buf
        } else {
            absolute_path
        }
    }
}
