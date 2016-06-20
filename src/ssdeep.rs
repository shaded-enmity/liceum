use std::process::Command;
use csv::Reader;
use std::path::Path;

/// Holds result of ssdeep comparison.
#[derive(Debug)]
pub struct CompareResult {
    pub similarity: u32,
    pub file_a: String,
    pub file_b: String,
}

/// Compute ssdeep hash, recursively, for all files in `dir` directory
/// and return a single string with one hash per line.
pub fn compute_directory(dir: &str) -> String {
    let result = match Command::new("ssdeep").arg("-br").arg(dir).output() {
        Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
        Err(_) => String::new(), 
    };

    result
}

/// Compare ssdeep hashes from `data_file`, recursively, against all files
/// in `dir` directory and return only those above `threshold` similarity.
pub fn compare(data_file: &str, dir: &str, threshold: u32) -> Vec<CompareResult> {
    let result = match Command::new("ssdeep").arg("-rcm").arg(data_file).arg(dir).output() {
        Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
        Err(_) => String::new(), 
    };

    let mut reader = Reader::from_string(result).has_headers(false);
    let mut res: Vec<CompareResult> = Vec::new();
    for line in reader.decode() {
        let (src, template, score): (String, String, u32) = line.unwrap();

        if score > threshold {
            res.push(CompareResult {
                similarity: score,
                file_a: src,
                file_b: String::from(Path::new(&template)
                                         .file_stem()
                                         .unwrap()
                                         .to_str()
                                         .unwrap()),
            });
        }
    }

    res
}
