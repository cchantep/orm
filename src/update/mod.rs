use std::fs;
use std::fs::File;
use std::str;

use std::io::Write;
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};

use std::process::{Command, ExitStatus};

use chrono::{DateTime, Utc};

use log::{debug, info, warn};

use hyper::body::Buf;
use hyper::{Client, Uri};
use hyper_tls::HttpsConnector;

use http::uri::{Parts, PathAndQuery};

use flate2::read::{GzDecoder, GzEncoder};
use flate2::Compression;
use tar::Archive;

pub mod manifest;

use super::error;
use super::io::{find_line, list_file_names};
use error::Error;

use crate::format_error;

#[derive(Debug)]
pub enum ExecutionStatus {
    NoUpdate(String),
    AppTerminated(ExitStatus),
}

/// Try to update the software.
pub async fn execute<'x>(
    manifest_url: &'static str,
    object_type: &'static str,
    app_name: &'static str,
    local_prefix: &'x Path,
    app_dir: &'x Path,
    current_version: semver::Version,
) -> Result<ExecutionStatus, Error> {
    let thing_id = resolve_id(app_dir)?;

    debug!("Thing ID = {}", thing_id);

    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    let update_settings = device_settings(object_type, manifest_url, &thing_id, &client).await?;

    debug!("Update settings = {:?}", update_settings);

    if update_settings.is_none() {
        return Err(format_error!("No device matching {}", thing_id));
    }

    let device = update_settings.unwrap();

    debug!(
        "Check update version {} against current {}",
        device.version, current_version
    );

    let new_version = semver::Version::parse(&device.version.0)?;

    if new_version <= current_version {
        return Ok(ExecutionStatus::NoUpdate(format!(
            "Application version is already up-to-date: {} < {}",
            new_version, current_version
        )));
    }

    let failed_versions_path = local_prefix.join(".orm_failed");
    let failed_version = find_line(&failed_versions_path, |line| {
        match semver::Version::parse(line) {
            Ok(ver) => ver == new_version,
            Err(_) => false,
        }
    })?;

    debug!("Failed version = {:?}", failed_version);

    if failed_version.is_some() {
        return Ok(ExecutionStatus::NoUpdate(format!(
            "Application version is a failed one: {}",
            new_version
        )));
    }

    // --- Archive

    let mut ar_file: File = tempfile::tempfile()?;

    let ar_size = download_archive_to(
        manifest_url,
        app_name,
        &device.version,
        &client,
        &mut ar_file,
    )
    .await?;

    debug!("Application archive size = {}", ar_size);

    ar_file.seek(SeekFrom::Start(0))?; // Rewind

    let extracted_dir = tempfile::tempdir()?;
    let extracted_path = extracted_dir.path();

    debug!("Checking archive & extracting to {:?}", extracted_path);

    let app_prefix = Path::new(app_name);

    extract_archive(&app_prefix, &ar_file, &extracted_path)?;

    let status = run_updated(
        app_name,
        local_prefix,
        app_dir,
        &failed_versions_path,
        &device.version,
        &extracted_path,
        &app_prefix,
    )
    .map_err(|err| {
        if !extracted_path.is_dir() {
            err
        } else {
            warn!(
                "Cleaning temporary directory {} on error: {}",
                extracted_path.display(),
                err
            );

            match fs::remove_dir_all(extracted_path) {
                Err(cause) => Error::from(cause),
                _ => err,
            }
        }
    })?;

    Ok(status)
}

/// Resolve the device/thing ID from the `id.sh` command,
/// that must be provided inside the application.
fn resolve_id<'x>(app_dir: &'x Path) -> Result<String, Error> {
    let cmd_path = app_dir.join("id.sh");
    let cmd_res = Command::new(&cmd_path).output();

    if cmd_res.is_err() {
        return Err(format_error!(
            "Fails to execute command {:?}: {}",
            &cmd_path,
            cmd_res.unwrap_err()
        ));
    }

    let cmd_out = cmd_res?;
    let id_res = str::from_utf8(cmd_out.stdout.as_slice())?;
    let thing_id = id_res.trim().to_string(); // Trim as CLI can output EOL

    let id_regex = regex::Regex::new("[A-Za-z]+[A-Za-z0-9-]*")?;

    if !id_regex.is_match(thing_id.as_str()) {
        return Err(format_error!("Invalid thing ID: {}", thing_id));
    }

    Ok(thing_id)
}

/// Finds settings for the specified device/thing.
async fn device_settings<'x>(
    object_type: &'static str,
    manifest_url: &'static str,
    thing_id: &'x String,
    client: &'x Client<HttpsConnector<hyper::client::HttpConnector>>,
) -> Result<Option<manifest::Device>, Error> {
    // --- Manifest
    info!("Fetching manifest from '{}' ...", manifest_url);

    let body = client.get(Uri::from_static(manifest_url)).await?;

    let status = body.status();

    debug!("Manifest request status: {}", status);

    if status != 200 {
        return Err(format_error!(
            "Fails to fetch manifest: status = {} != 200",
            status
        ));
    }

    // ---

    let buf = hyper::body::to_bytes(body).await?;
    let bytes = buf.to_vec();
    let utf = bytes.as_slice();
    let yml = str::from_utf8(utf)?;

    debug!("YAML\n{}\n---", yml);

    let manifest = serde_yaml::from_str::<manifest::Manifest>(yml)?;

    debug!("Manifest\n---\n{}\n---", manifest);

    if manifest.object_type != object_type {
        return Err(format_error!(
            "Unexpected object_type: {} != {}",
            manifest.object_type,
            object_type
        ));
    }

    let found = manifest.devices.iter().find(|dev| {
        let manifest::Pattern(p) = &dev.pattern;

        match regex::Regex::new(&p) {
            Ok(re) => re.is_match(thing_id),
            _ => {
                warn!("Invalid pattern {}", p);
                false
            }
        }
    });

    Ok(found.map(|dev| dev.clone()))
}

/// Returns the parent URI.
fn parent_uri(url: &str) -> Result<Uri, Error> {
    let uri = url.parse::<Uri>().unwrap();
    let uri_parts = uri.into_parts();

    if uri_parts.path_and_query.is_none() {
        return Err(format_error!("Invalid manifest URL: {}", url));
    }

    let path_and_query = uri_parts.path_and_query.unwrap();
    let path_segments: Vec<&str> = path_and_query.path().split("/").collect();

    let path_count = path_segments.len();

    if path_count == 0 {
        return Err(format_error!("Invalid manifest path: {:?}", path_segments));
    }

    let parent_path: PathAndQuery = path_segments
        .iter()
        .take(path_count - 1)
        .fold("".to_string(), |out, seg| match seg {
            &"" => out,
            _ => out + "/" + seg,
        })
        .parse()
        .unwrap();

    let mut parent_parts = Parts::default();

    parent_parts.scheme = uri_parts.scheme;
    parent_parts.authority = uri_parts.authority;
    parent_parts.path_and_query = Some(parent_path);

    Uri::from_parts(parent_parts).map_err(Error::from)
}

/// Download the application archive to
async fn download_archive_to<'x>(
    manifest_url: &'static str,
    app_name: &'static str,
    version: &'x manifest::Version,
    client: &'x Client<HttpsConnector<hyper::client::HttpConnector>>,
    target: &'x mut File,
) -> Result<u64, Error> {
    let parent_uri = parent_uri(manifest_url).unwrap();

    debug!("Parent URL = {:?}", parent_uri);

    let archive_uri = Uri::builder()
        .scheme(parent_uri.scheme_str().unwrap())
        .authority(parent_uri.authority().unwrap().as_str())
        .path_and_query(format!(
            "{}/{}-{}.tar.gz",
            parent_uri.path(),
            app_name,
            version
        ))
        .build()
        .unwrap();

    debug!("Archive URL = {:?}", archive_uri);

    let body = client.get(archive_uri).await?;
    let buf = hyper::body::to_bytes(body).await?;

    debug!(
        "Downloading application archive to temporary file = {:?}",
        target
    );

    let size = std::io::copy(&mut buf.reader(), target)?;

    Ok(size)
}

/// Extracts the application archive.
fn extract_archive<'x>(
    prefix: &'x Path,
    ar_file: &'x File,
    extracted_path: &'x Path,
) -> Result<usize, Error> {
    let tar = GzDecoder::new(ar_file);
    let mut app_archive = Archive::new(tar);

    let entries: Vec<PathBuf> = app_archive
        .entries()?
        .filter_map(|e| e.ok())
        .map(|mut entry| -> Result<PathBuf, std::io::Error> {
            let path = entry.path()?.to_path_buf().to_owned();
            let extracted_entry = extracted_path.join(&path);

            debug!("Extracted entry = {:?}", extracted_entry);

            entry.unpack(extracted_entry).map(|_| path)
        })
        .filter_map(|p| p.ok())
        .filter(|p| match p.parent() {
            Some(parent) => parent == prefix && (p.ends_with("run.sh") || p.ends_with("id.sh")),
            None => false,
        })
        .collect();

    let size = entries.len();

    if size != 2 {
        return Err(format_error!(
            "Invalid archive; Missing script(s): {:?}",
            entries
        ));
    }

    Ok(size)
}

/// Try to run the updated application.
fn run_updated<'x>(
    app_name: &'static str,
    local_prefix: &'x Path,
    app_dir: &'x Path,
    failed_versions_path: &'x Path,
    version: &'x manifest::Version,
    extracted_path: &'x Path,
    app_prefix: &'x Path,
) -> Result<ExecutionStatus, Error> {
    let archived_path: PathBuf = {
        let now: DateTime<Utc> = Utc::now();
        let ts = now.format("%Y%m%d%H%M%S").to_string();

        local_prefix.join(format!("{}-{}", app_name, ts))
    };
    let archived_dir = (match archived_path.to_str() {
        Some(dir) => Ok(dir),
        None => Err(format_error!(
            "Fails to prepare archive directory path: {:?}",
            archived_path
        )),
    })?;

    info!(
        "Renaming previous application directory to {:?}",
        archived_dir
    );

    fs::rename(app_dir, archived_dir)?;

    let status = fs::rename(extracted_path.join(app_prefix), app_dir)
        .and_then(|_| {
            let run_script = app_dir.join("run.sh");

            debug!("Updated run script: {:?}", run_script);

            Command::new(run_script).spawn().and_then(|mut child| {
                info!("Successfully started updated {:?} ...", app_dir);

                // List previous archive
                let previous_archives = list_file_names(local_prefix, |n| {
                    n.starts_with(app_name) && n.ends_with(".tar.gz")
                })?;

                // Create archive of the previous application directory
                let archived_tar = File::create(archived_path.with_extension("tar.gz"))?;

                let enc = GzEncoder::new(&archived_tar, Compression::best());
                let mut tar = tar::Builder::new(enc);

                tar.append_dir_all(app_prefix, &archived_dir)?;

                fs::remove_dir_all(archived_dir)?;

                debug!(
                    "Previous application directory archived as {:?}",
                    archived_tar
                );

                // Clean archives
                for ar in previous_archives.iter() {
                    debug!("Cleaning previous archive: {}", ar);

                    fs::remove_file(local_prefix.join(ar))?
                }

                // Add version marker and wait termination
                let mut version_marker = File::create(app_dir.join(".orm_version"))?;

                write!(&mut version_marker, "{}", version)?;
                debug!("Current version marker = {}", version);

                child.wait().map(ExecutionStatus::AppTerminated)
            })
        })
        .or_else(|err| {
            let msg = format!(
                "Reverts due to failed execution of application from update archive: {}",
                err
            );

            warn!("{}", msg);

            // Mark as failed version
            let mut failed_versions = fs::OpenOptions::new()
                .write(true)
                .append(true)
                .create(true)
                .open(failed_versions_path)?;

            debug!("Failed version: {:?}", failed_versions);
            writeln!(failed_versions, "{}", version)?;

            // Revert
            let before_revert = {
                if app_dir.is_dir() {
                    fs::remove_dir_all(app_dir)
                } else {
                    Ok(())
                }
            };

            before_revert
                .and_then(|_| fs::rename(archived_dir, app_dir))
                .map(|_| ExecutionStatus::NoUpdate(msg))
        })?;

    Ok(status)
}

// --- Tests

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_parent_uri() {
        // File at root
        let parent1 = parent_uri("http://foo/manifest.yaml").unwrap();

        assert_eq!(parent1.to_string(), "http://foo/".to_string());

        // File in sub path
        let parent2 = parent_uri("https://foo/bar/manifest.yaml").unwrap();

        assert_eq!(parent2.to_string(), "https://foo/bar".to_string());
    }
}
