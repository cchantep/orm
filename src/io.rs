use std::io::{BufRead, BufReader, Error};

use std::path::Path;

/// List the file names in the specified directory,
/// using the given filter.
pub fn list_file_names<'x, F>(dir_path: &'x Path, filter: F) -> Result<Vec<String>, Error>
where
    F: for<'r> std::ops::Fn(&'r String) -> bool,
{
    let ls = std::fs::read_dir(dir_path)?;

    let names = ls
        .filter_map(|f| f.ok())
        .filter_map(|file| {
            let nme = file.file_name();
            let osn = nme.to_str().map(|s| s.to_string());

            osn.filter(|n| filter(&n))
        })
        .collect::<Vec<String>>();

    Ok(names)
}

/// Finds a text line in file at given path.
pub fn find_line<'x, F>(path: &'x Path, accepts: F) -> Result<Option<String>, Error>
where
    F: Fn(&String) -> bool,
{
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let lines = reader.lines();

    for ln in lines {
        let line = ln?;

        if accepts(&line) {
            return Ok(Some(line));
        }
    }

    Ok(None)
}
