extern crate regex;
extern crate rustc_serialize;
extern crate getopts;
extern crate threadpool;
extern crate csv;
extern crate walkdir;

use std::fs::File;
use std::io::{Read, Error, Write};
use std::iter::FromIterator;
use std::collections::{HashSet, HashMap};
use std::fmt::Debug;
use std::{fs, fmt, env};
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::hash::Hash;
use std::process::Command;

use getopts::Options;
use rustc_serialize::json;
use threadpool::ThreadPool;
use regex::Regex;
use walkdir::{DirEntry, WalkDir, WalkDirIterator};

pub mod pathex;
pub mod ngram;
pub mod ssdeep;
use ngram::NGram;
use pathex::AbsolutePath;

static SSDEEP_HASHES: &'static str = "hashes.ssdeep";
static NGRAMS_FILE: &'static str = "ngrams.json";

/// Insert multiple values under a single key
trait OneToMany<K, V> {
    fn insert_one(&mut self, key: K, value: V);
}

impl<K: Eq + Hash, V> OneToMany<K, V> for HashMap<K, Vec<V>> {
    /// If `key` is not present insert the key with a vector containing the `value`
    /// whereas if `key` is already present then push the `value` into the vector
    fn insert_one(&mut self, key: K, value: V) {
        if self.contains_key(&key) {
            self.get_mut(&key).unwrap().push(value);
        } else {
            self.insert(key, vec![value]);
        }
    }
}

type IoResult<T> = Result<T, Error>;

/// Generic container for `leveled ngrams`
#[derive(RustcDecodable, RustcEncodable, Debug)]
struct Data<T> {
    ngrams: Vec<T>,
    level: u64,
}

/// Generic string ngram
type NG = NGram<String>;

/// Generic vector owning it's ngrams
type NGramVec = Vec<NG>;

/// Raw output and input data for processing
type OutData<'a> = Data<&'a NG>;
type InData = Data<NG>;

/// Primitive representation of the output JSON document
type VecOutData<'a> = Data<&'a Vec<String>>;
type JsonOutMap<'a> = HashMap<&'a str, VecOutData<'a>>;

/// Primitive representation of the input JSON document
type VecInData = Data<Vec<String>>;
type JsonInMap = HashMap<String, VecInData>;

/// Input corpus structure holding basic information
struct InputCorpus {
    file: String,
    data: InData,
}

/// All input corpuses
type InputVector = Vec<InputCorpus>;

/// Store information about licenses found per file
struct SearchResult {
    file: String,
    found: String,
}

/// Maps corpus name to vector of ngrams.
#[derive(Eq, PartialEq, Hash)]
struct LicenseCorpus {
    file: String,
    ngrams: NGramVec,
}

impl Debug for LicenseCorpus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
               "LicenseCorpus: {} ({} ngrams)",
               self.file,
               self.ngrams.len())
    }
}

fn read_file(file: &str) -> IoResult<String> {
    let mut f = try!(File::open(file));
    let mut s = String::new();
    try!(f.read_to_string(&mut s));
    Ok(s)
}

fn write_file(file: &str, data: &str) -> IoResult<usize> {
    let mut f = try!(File::create(file));
    let written = try!(f.write(data.as_bytes()));
    // println!(" {} bytes written", written);
    Ok(written)
}

const NGRAM_SIZE: usize = 7;

/// Get ngrams of size `n` from input string `from`.
fn get_ngrams(from: &str, n: usize) -> NGramVec {
    let minus_newlines = from.replace("\n", " ");

    // Collapse 2+ consecutive whitespace characters into one
    let whitespace_collapser = Regex::new(r"[\r\t\v ]{1,}").unwrap();
    // Remove <text_in_angle_brackets>
    let angle_remover = Regex::new(r"<[\w_]*>").unwrap();
    // Remove ____...
    let underscore_filter = Regex::new(r"_{2,}").unwrap();
    let sanitized = whitespace_collapser.replace_all(
        &angle_remover.replace_all(
            &underscore_filter.replace_all(&minus_newlines, ""), 
            ""), 
        " ");
    // Get rid of all empty chars
    let filtered = sanitized.split(" ")
                            .filter(|x| *x != "")
                            .collect::<Vec<&str>>()
                            .join(" ");
    // Create `n` iterators splitting the string by the space character
    let mut iterators = vec![filtered.split(" "); n];

    // Skew the iterators by their position such as:
    //
    // I.   | ABCDEFGHIJKL
    // II.  |  BCDEFGHIJKL
    // III. |   CDEFGHIJKL
    //  ...
    //
    // Reading from each iterator will then yield next n-gram
    let mut cnt = 0;
    for iterator in iterators.iter_mut() {
        for _ in 0..cnt {
            let n = iterator.next()
                            .map(|_| true)
                            .unwrap_or(false);
            if !n {
                return Vec::new();
            }
        }

        cnt += 1;
    }

    let mut grams: NGramVec = Vec::new();
    let mut ng: Vec<&str> = Vec::new();
    'main: loop {
        for iterator in iterators.iter_mut() {
            if let Some(item) = iterator.next() {
                ng.push(item);
            } else {
                // one of the iterators has reached the end, break out of the main loop
                break 'main;
            }
        }

        let values = ng.iter()
                       .map(|x| String::from(*x))
                       .collect::<Vec<_>>();
        let ngram = NGram::new(&values);
        grams.push(ngram);
        ng.clear();
    }

    grams
}

/// Save data from the input map into a `JsonOutMap` for JSON serialization.
fn save_data<'a>(data: &'a HashMap<&'a LicenseCorpus, OutData>) -> JsonOutMap<'a> {
    let mut out: JsonOutMap = HashMap::new();

    for (corpus, ngrams) in data {
        let name = Path::new(&corpus.file)
                       .file_stem()
                       .unwrap()
                       .to_str()
                       .unwrap();

        let mut data_grams: Vec<&Vec<String>> = Vec::new();
        for g in &ngrams.ngrams {
            data_grams.push(&g.elements.obj);
        }

        let data = VecOutData {
            level: ngrams.level,
            ngrams: data_grams,
        };

        out.insert(name, data);
    }

    out
}

/// Creates `Arc<InputVector>` sorted by data level from
/// input license corpus.
fn load_data(input: &JsonInMap) -> Arc<InputVector> {
    let mut licenses: InputVector = Vec::new();

    for (k, v) in input {
        let mut ngrams: NGramVec = Vec::new();
        for ngram in &v.ngrams {
            ngrams.push(NGram::new(&ngram));
        }

        let data = InData {
            level: v.level,
            ngrams: ngrams,
        };

        let item = InputCorpus {
            file: k.clone(),
            data: data,
        };

        licenses.push(item);
    }

    licenses.sort_by_key(|x| x.data.level);

    Arc::new(licenses)
}

/// Generate ngram corpuses from all files in `data_dir` and return them
/// as a string serialized JSON.
fn generate_corpuses(data_dir: &str, verbose: bool) -> String {
    // Load corpus files
    let mut corpuses: Vec<LicenseCorpus> = Vec::new();
    let paths = fs::read_dir(data_dir).unwrap();
    for path in paths {
        let p = path.unwrap().path();
        if verbose {
            println!("{:?}", p);
        }

        let data = read_file(p.to_str().unwrap()).unwrap();
        let ngrams = get_ngrams(&data, NGRAM_SIZE);

        corpuses.push(LicenseCorpus {
            file: String::from(p.to_str().unwrap()),
            ngrams: ngrams,
        });
    }

    let mut ngrammap: HashMap<&NGram<String>, Vec<&LicenseCorpus>> = HashMap::new();
    if verbose {
        println!("[+] Generating n-gram map for {} corpuses", corpuses.len());
    }

    {
        let z = &corpuses;
        for corpus in z {
            for ngram in &corpus.ngrams {
                ngrammap.insert_one(ngram, corpus);
            }
        }
    }
    if verbose {
        println!("[!] Done generating n-gram map: {} items", ngrammap.len());
    }

    // Map corpuses to unique ngrams
    let mut fm: HashMap<&LicenseCorpus, OutData> = HashMap::new();

    // Store finished corpuses
    let mut finished: HashSet<&LicenseCorpus> = HashSet::new();

    // We'll mutate this hashmap at the end of each loop to remove
    // garbage ngrams (ngrams unique to already finished corpus)
    let mut allngrams = ngrammap.clone();

    let pgbar: [&str; 4] = ["-", "\\", "|", "/"];
    let (mut loops, mut prints) = (1, 0);

    loop {
        if finished.len() == corpuses.len() {
            break;
        }

        let last = finished.len();
        let count = allngrams.len();
        let mut cleanup: Vec<&NGram<String>> = Vec::new();
        for (i, (ngram, mut occurences)) in allngrams.iter_mut().enumerate() {
            if i % 100 == 0 {
                print!("\r[{}] Processing .. {}/{}", pgbar[prints % 4], i, count);
                std::io::stdout().flush().ok();
                prints += 1;
            }

            // We have an ngram with only a single edge
            if occurences.len() == 1 {
                let key = occurences.iter().next().unwrap();
                if finished.contains(key) {
                    cleanup.push(ngram);
                    continue;
                }

                if fm.contains_key(key) {
                    let mut ngrams = fm.get_mut(key).unwrap();
                    ngrams.ngrams.push(ngram);
                    ngrams.level = loops;
                    // 3 unique ngrams
                    if ngrams.ngrams.len() > 2 {
                        if verbose {
                            println!("\r finished: {:?}", key.file);
                        }
                        finished.insert(key);
                        cleanup.push(ngram);
                    }
                } else {
                    fm.insert(key,
                              OutData {
                                  ngrams: vec![ngram],
                                  level: loops,
                              });
                    cleanup.push(ngram);
                }
            } else {
                for fin in &finished {
                    if let Some(x) = occurences.iter().position(|x| x == fin) {
                        occurences.remove(x);
                    };
                }
            }
        }

        for ng in cleanup {
            allngrams.remove(ng);
        }

        if last == finished.len() {
            println!("\nRemaining: {}", corpuses.len() - finished.len());
            for key in &corpuses {
                if !finished.contains(key) {
                    println!("\n {}", key.file);
                }
            }
            // We didn't find a single solution during this iteration
            // so we bail out, some further graph magic will be needed
            // to find the solution (if there is one).
            panic!("[E] Solution not found! ¯\\_(ツ)_/¯ ");
        }

        loops += 1;
    }

    println!("{} corpuses created!", finished.len());

    let result: JsonOutMap = save_data(&fm);
    // Format as pretty JSON and write to file
    let pj = json::as_pretty_json(&result).indent(3);
    format!("{}", &pj)
}

/// Predicate determining whether the path is hidden.
fn is_hidden(entry: &DirEntry) -> bool {
    entry.path()
         .to_str()
         .map(|s| s.contains("/."))
         .unwrap_or(false)
}

/// Predicate determining whether the path is a text file or directory
/// entry. Due to the recursive nature of the walker we can't filter out
/// directories here.
fn is_text_or_dirent(entry: &DirEntry) -> bool {
    let result = match Command::new("file")
                           .arg("-bi")
                           .arg(entry.path().to_str().unwrap())
                           .output() {
        Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
        Err(_) => String::new(),
    };

    result.starts_with("text/") || result.starts_with("inode/directory;")
}

/// Search `path` using data from `data` for both ngrams and ssdeep hashes.
/// Returns a string serialized JSON.
fn search_path(data: &str, path: &str) -> String {
    let pool = ThreadPool::new(16);
    let mut paths: Vec<String> = Vec::new();

    let walker = WalkDir::new(path).into_iter();
    for p in walker.filter_entry(|e| !is_hidden(e) && is_text_or_dirent(e)) {
        let file_entry = p.unwrap();
        let file = file_entry.path().to_str().unwrap();

        if file_entry.file_type().is_file() {
            paths.push(String::from(file));
        }
    }

    let rx = {
        let (tx, rx) = mpsc::channel();
        let ngrams_path = Path::new(data).join(NGRAMS_FILE);
        let d = read_file(ngrams_path.to_str().unwrap()).unwrap();
        let decoded: JsonInMap = json::decode(&d).unwrap();
        let licenses = load_data(&decoded);

        for p in paths.iter().cloned() {
            let (tx, licenses) = (tx.clone(), licenses.clone());

            pool.execute(move || {
                let ng = get_ngrams(&read_file(&p).unwrap(), NGRAM_SIZE);
                let ngrams: HashSet<&NGram<String>> = HashSet::from_iter(ng.iter());

                for ref ic in licenses.iter() {
                    let check = ic.data.ngrams.iter().all(|x| ngrams.contains(x));
                    if check {
                        tx.send(Arc::new(SearchResult {
                              file: p.clone(),
                              found: ic.file.clone(),
                          }))
                          .unwrap();
                    }
                }
            });
        }

        rx
    };

    // file: vec![found_licenses]
    let mut results: HashMap<String, Vec<String>> = HashMap::new();
    while let Ok(item) = rx.recv() {
        let p = Path::new(&item.file).canonical_path();
        results.insert_one(String::from(p.as_path().to_str().unwrap()),
                           item.found.clone());
    }

    // ssdeep search
    let hashes_path = Path::new(data).join(SSDEEP_HASHES);
    // TODO: Make sensitivity tunable
    let hashed = ssdeep::compare(hashes_path.to_str().unwrap(), path, 75);
    for h in &hashed {
        results.insert_one(h.file_a.clone(), h.file_b.clone());
    }

    let pj = json::as_pretty_json(&results).indent(3);
    format!("{}", pj)
}

fn print_usage(code: i32, program: &str, opts: &Options) {
    let banner = format!("Usage: {} [options] ...", program);
    println!("{} - {}", program, "0.1.0");
    print!("{}", opts.usage(&banner));
    std::process::exit(code);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args[0].split('/').last().unwrap();
    let mut opts = Options::new();
    opts.optflag("h", "help", "display usage information");
    opts.optopt("g",
                "generate",
                "generate data from target directory",
                "DIR");
    opts.optopt("c", "check", "check using this data corpus", "FILE");
    opts.optflag("v", "verbose", "verbose mode");
    opts.optflag("", "version", "display version information");
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => panic!(e.to_string()),
    };

    // ssdeep::compute_directory("data/");
    // ssdeep::compare("license_hashes", "../docker-hica/", 75);

    if matches.opt_present("h") {
        print_usage(0, &program, &opts);
    }

    let verbose = matches.opt_present("v");
    let is_check = matches.opt_present("c");
    let is_generate = matches.opt_present("g");
    if is_generate && is_check {
        panic!("Options -g and -c are mutually exclusive");
    } else if !is_generate && !is_check {
        panic!("Provide either -g or -c argument");
    }

    let check_data = match matches.opt_str("c") {
        Some(x) => x,
        None => String::new(),
    };
    let gen_data = match matches.opt_str("g") {
        Some(x) => x,
        None => String::new(),
    };

    if is_check {
        if check_data == "" {
            panic!("Empty check data");
        }

        if matches.free.len() == 0 {
            panic!("Nothing to check");
        }

        let output = search_path(&check_data, &matches.free[0]);
        println!("{}", output);
    } else {
        if gen_data == "" {
            panic!("No target directory from which to generate data");
        }

        fs::create_dir("cache/").ok();

        let output = generate_corpuses(&gen_data, verbose);
        let ngrams = Path::new("cache/").join(NGRAMS_FILE);
        write_file(&ngrams.to_str().unwrap(), &output).ok();

        let ssdeep = ssdeep::compute_directory(&gen_data);
        let hashes = Path::new("cache/").join(SSDEEP_HASHES);
        write_file(hashes.to_str().unwrap(), &ssdeep).ok();
    }
}
