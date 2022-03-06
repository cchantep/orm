use std::error::Error;
use std::str;

use std::path::Path;

use log::{debug, info, warn};

mod err;
mod update;

use update::manifest::Version;
use update::ExecutionStatus as UpdateStatus;

/// The type of IoT object; Must correspond to the object type on IoT Core.
const OBJECT_TYPE: &'static str = env!("OBJECT_TYPE");

/// The URL to fetch/GET the YAML manifest.
const YAML_MANIFEST_URL: &'static str = env!("YAML_MANIFEST_URL");

/// The name of the managed application.
const APPLICATION_NAME: &'static str = env!("APPLICATION_NAME");

/// The local prefix path.
const LOCAL_PREFIX: &'static str = env!("LOCAL_PREFIX");

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    setup_logging();

    info!("Software management for {}.", OBJECT_TYPE);

    let local_prefix = Path::new(LOCAL_PREFIX);

    if !local_prefix.is_dir() {
        return boxed_error!("Local prefix is not a valid directory: {}", LOCAL_PREFIX);
    }

    // ---

    let app_dir = local_prefix.join(APPLICATION_NAME);

    debug!("Application directory = {:?}", app_dir);

    if !app_dir.is_dir() {
        return boxed_error!("Application directory is not a valid one: {:?}", app_dir);
    }

    // ---

    let version_path = app_dir.join(".orm_version");

    if !version_path.is_file() {
        return boxed_error!("Missing ORM version marker: {:?}", version_path);
    }

    let version_content = std::fs::read_to_string(version_path)?;

    // TODO: Regex to validate?
    let current_version = Version(version_content.trim().to_string());

    info!("Current version is {}", current_version);

    // ---

    let update_status = update::execute(
        YAML_MANIFEST_URL,
        OBJECT_TYPE,
        APPLICATION_NAME,
        &local_prefix,
        &app_dir,
        current_version,
    )
    .await;

    debug!("Update status: {:?}", update_status);

    let run = || -> Result<(), Box<dyn Error + Send + Sync>> {
        run_app(&app_dir)
            .or_else(|run_err| Err(Box::new(run_err))?)
            .map(|run_status| info!("Exited with status: {:?}", run_status))
    };

    let update_result = update_status.and_then(|status| match status {
        UpdateStatus::NoUpdate(msg) => {
            info!("No update: {}", msg);
            info!("Executing the current version ...");

            run()
        },
        UpdateStatus::AppTerminated(status) => {
            Ok(info!("Updated application successfully terminated: {}", status))
        }
    });

    update_result.or_else(|up_err| {
        warn!("Fails to update software for {}: {}", OBJECT_TYPE, up_err);

        run()
    })
}

/// Set up logging.
fn setup_logging() {
    // TODO: Check datalog logging

    if cfg!(debug_assertions) {
        env_logger::Builder::new()
            .filter_level(log::LevelFilter::Debug)
            .init();
    } else if !log::log_enabled!(log::Level::Info) {
        env_logger::Builder::new()
            .filter_level(log::LevelFilter::Info)
            .init();
    } else {
        env_logger::init();
    }
}

use std::process::{Command, ExitStatus};

/// Runs current version of the application
fn run_app(app_dir: &Path) -> Result<ExitStatus, Box<err::Error>> {
    let run_script = app_dir.join("run.sh");

    debug!("Run script: {:?}", run_script);

    Command::new(run_script)
        .spawn()
        .and_then(|mut child| {
            info!("Successfully started {:?} ...", app_dir);

            child.wait()
        })
        .or_else(|err| Err(Box::new(err::Error::from(err)))?)
}
