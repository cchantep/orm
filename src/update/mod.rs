use std::boxed::Box;
use std::error::Error;
use std::str;
use std::fs;

use std::io::{Seek, SeekFrom};
use std::io::Write;
use std::path::{Path, PathBuf};

use std::process::{Command, ExitStatus};

use chrono::{DateTime, Utc};

use log::{debug, info, warn};

use hyper::body::Buf;
use hyper::{Client, Uri};
use hyper_tls::HttpsConnector;

use http::uri::{Parts, PathAndQuery};

use flate2::read::GzDecoder;
use tar::Archive;

pub mod manifest;

use super::err;
use crate::{boxed_error, format_error};

#[derive(Debug)]
pub enum ExecutionStatus {
    NoUpdate(String),
    AppTerminated(ExitStatus)
}

/// Try to update the software.
pub async fn execute<'x>(
    manifest_url: &'static str,
    object_type: &'static str,
    app_name: &'static str,
    local_prefix: &'x Path,
    app_dir: &'x Path,
    current_version: manifest::Version,
) -> Result<ExecutionStatus, Box<dyn 'x + Error + Send + Sync>> {
    // --- Identifier

    let id_path = app_dir.join("id.sh");
    let id_res = Command::new(&id_path).output();

    if id_res.is_err() {
        return boxed_error!(
            "Fails to execute command {:?}: {}",
            &id_path,
            id_res.unwrap_err()
        );
    }

    let id_out = id_res?;
    let id_bytes = str::from_utf8(id_out.stdout.as_slice());

    if id_bytes.is_err() {
        return Err(id_bytes.unwrap_err())?;
    }

    // ---

    let thing_id = id_bytes?.trim(); // Trim as CLI can output EOL

    debug!("Thing ID = {}", thing_id);

    // TODO: Check the thing_id is valid [A-Za-z]+[A-Za-z0-9-]*

    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    // --- Manifest
    info!("Fetching manifest from '{}' ...", manifest_url);

    let mf_body = client.get(Uri::from_static(manifest_url)).await?;

    let mf_status = mf_body.status();

    debug!("Manifest request status: {}", mf_status);

    if mf_status != 200 {
        return boxed_error!("Fails to fetch manifest: status = {} != 200", mf_status);
    }

    // ---

    let mf_buf = hyper::body::to_bytes(mf_body).await?;
    let mf_bytes = mf_buf.to_vec();
    let utf = mf_bytes.as_slice();
    let yml = str::from_utf8(utf)?;

    debug!("YAML\n{}\n---", yml);

    let manifest = serde_yaml::from_str::<manifest::Manifest>(yml)?;

    debug!("Manifest\n---\n{}\n---", manifest);

    if manifest.object_type != object_type {
        return boxed_error!(
            "Unexpected object_type: {} != {}",
            manifest.object_type,
            object_type
        );
    }

    let matching_device = manifest.devices.iter().find(|dev| {
        let manifest::Pattern(p) = &dev.pattern;

        match regex::Regex::new(&p) {
            Ok(re) => re.is_match(thing_id),
            _ => {
                warn!("Invalid pattern {}", p);
                false
            }
        }
    });

    let update_settings = matching_device; // TODO: Separate function? device_settings(object_type, manifest_url, thing_id, client).await?;

    debug!("Update settings = {:?}", update_settings);

    if update_settings.is_none() {
        return boxed_error!("No device matching {}", thing_id);
    }

    let device = update_settings.unwrap();

    debug!(
        "Check update version {} against current {}",
        device.version, current_version
    );

    if device.version == current_version {
        return Ok(ExecutionStatus::NoUpdate(format!(
            "Application version is already up-to-date: {}",
            current_version
        )));
    }

    // TODO: Check the version from the matching device
    // is not the current one (tracked in local file?), otherwise skip update

    // --- Archive

    let parent_uri = parent_uri(manifest_url).unwrap();

    debug!("Parent URL = {:?}", parent_uri);

    let archive_uri = Uri::builder()
        .scheme(parent_uri.scheme_str().unwrap())
        .authority(parent_uri.authority().unwrap().as_str())
        .path_and_query(format!(
            "{}/{}-{}.tar.gz",
            parent_uri.path(),
            app_name,
            device.version
        ))
        .build()
        .unwrap();

    debug!("Archive URL = {:?}", archive_uri);

    let ar_body = client.get(archive_uri).await?;
    let ar_buf = hyper::body::to_bytes(ar_body).await?;
    let mut ar_file = tempfile::tempfile()?;

    debug!("Downloading archive to temporary file = {:?}", ar_file);

    let ar_size = std::io::copy(&mut ar_buf.reader(), &mut ar_file)?;

    debug!("Archive size = {}", ar_size);

    ar_file.seek(SeekFrom::Start(0))?; // Rewind

    let ar_tar = GzDecoder::new(ar_file);
    let mut app_archive = Archive::new(ar_tar);

    let entry_prefix = Path::new(app_name);

    let ar_entries: Vec<PathBuf> = app_archive
        .entries()?
        .filter_map(|e| e.ok())
        .map(|mut entry| -> Result<PathBuf, std::io::Error> {
            let path = entry.path()?.to_path_buf().to_owned();

            entry.unpack(&path)?;

            Ok(path)
        })
        .filter_map(|p| p.ok())
        .filter(|p| match p.parent() {
            Some(parent) => {
                parent == entry_prefix && (p.ends_with("run.sh") || p.ends_with("id.sh"))
            }
            None => false,
        })
        .collect();

    if ar_entries.len() != 2 {
        return boxed_error!("Invalid archive; Missing script(s): {:?}", ar_entries);
    }

    let app_dir_archived_path = || -> PathBuf {
        let now: DateTime<Utc> = Utc::now();
        let ts = now.format("%Y%m%d%H%M%S").to_string();

        local_prefix.join(format!("{}-{}", app_name, ts))
    };
    let archived_dir = app_dir_archived_path();

    info!("Renaming previous application directory to {:?}", archived_dir);

    fs::rename(app_dir, archived_dir)?;

    app_archive.unpack(app_dir).and_then(|unpacked| {
        debug!("Unpacked = {:?}", unpacked);

        let run_script = app_dir.join("run.sh");
        
        debug!("Run script: {:?}", run_script);
        
        Command::new(run_script)
            .spawn()
            .and_then(|mut child| {
                info!("Successfully started updated {:?} ...", app_dir);

                let mut version_marker = fs::File::create(
                    app_dir.join(".orm_version"))?;
                
            write!(&mut version_marker, "{}", current_version)?;
                debug!("Current version marker = {}", current_version);
                
                child.wait().map(ExecutionStatus::AppTerminated)
            })
    }).or_else(|err| {
        let msg = format!("Reverts due to failed execution of application from update archive: {}", err);

        warn!("{}", msg);

        // TODO: Write version to avoid trying again this update?

        let before_revert = {
            if app_dir.is_dir() {
                // TODO: Rather rename or archive?
                fs::remove_dir_all(app_dir)
            } else {
                Ok(())
            }
        };

        before_revert
            .and_then(|_| fs::rename(app_dir_archived_path(), app_dir))
            .map(|_| ExecutionStatus::NoUpdate(msg))
            .map_err(|e| Box::new(err::Error::from(e)) as Box<dyn 'x + Error + Send + Sync>)
    })
}

/// Returns the parent URI.
fn parent_uri(url: &str) -> Result<Uri, err::Error> {
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

    Uri::from_parts(parent_parts).map_err(err::Error::from)
}
