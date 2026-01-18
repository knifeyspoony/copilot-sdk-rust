// Copyright (c) 2026 Elias Bachaalany
// SPDX-License-Identifier: MIT

use std::fs;
use std::path::{Path, PathBuf};

const EXPECTED_COPYRIGHT: &str = "Copyright (c) 2026 Elias Bachaalany";
const EXPECTED_SPDX: &str = "SPDX-License-Identifier: MIT";

fn list_rs_files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(list_rs_files_under(&path));
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }

    out
}

fn read_first_lines(path: &Path, n: usize) -> String {
    let Ok(contents) = fs::read_to_string(path) else {
        return String::new();
    };
    contents.lines().take(n).collect::<Vec<_>>().join("\n")
}

#[test]
fn rust_files_have_attribution_header() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let mut files = Vec::new();
    files.extend(list_rs_files_under(&crate_root.join("src")));
    files.extend(list_rs_files_under(&crate_root.join("examples")));
    files.extend(list_rs_files_under(&crate_root.join("tests")));

    let mut missing = Vec::new();
    for file in files {
        let first = read_first_lines(&file, 3);
        if !first.contains(EXPECTED_COPYRIGHT) || !first.contains(EXPECTED_SPDX) {
            missing.push(file);
        }
    }

    assert!(
        missing.is_empty(),
        "Missing/incorrect attribution header in: {missing:?}"
    );
}

#[test]
fn license_has_attribution_year_2026() {
    let license = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("LICENSE");
    let text = fs::read_to_string(&license).expect("read LICENSE");
    assert!(
        text.contains(EXPECTED_COPYRIGHT),
        "LICENSE missing expected attribution line: {EXPECTED_COPYRIGHT}"
    );
}
