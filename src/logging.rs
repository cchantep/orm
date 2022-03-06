use std::env::var;

use datadog_logs::config::{DataDogConfig, DataDogHttpConfig};
use datadog_logs::error::DataDogLoggerError;
use datadog_logs::logger::DataDogLogger;

use crate::error::Error;

/// Compile-time DataDog API URL
const DATADOG_API_URL: Option<&'static str> = option_env!("DATADOG_API_URL");

/// Compile-time DataDog API key
const DATADOG_API_KEY: Option<&'static str> = option_env!("DATADOG_API_KEY");

/// Compile-time DataDog tags
const DATADOG_TAGS: Option<&'static str> = option_env!("DATADOG_TAGS");

/// Compile-time DataDog service
const DATADOG_SERVICE: Option<&'static str> = option_env!("DATADOG_SERVICE");

/// Compile-time DataDog source
const DATADOG_SOURCE: Option<&'static str> = option_env!("DATADOG_SOURCE");

/// Set up logging.
pub fn setup() -> Result<(), Error> {
    let datadog_api_url = DATADOG_API_URL
        .map(|s| s.to_string())
        .or_else(|| var("DATADOG_API_URL").ok());

    let datadog_api_key = DATADOG_API_KEY
        .map(|s| s.to_string())
        .or_else(|| var("DATADOG_API_KEY").ok());

    match datadog_api_url.zip(datadog_api_key) {
        Some((url, api_key)) => {
            let http_config = DataDogHttpConfig { url: url };
            let tags = DATADOG_TAGS
                .map(|s| s.to_string())
                .or_else(|| var("DATADOG_TAGS").ok());
            let service = DATADOG_SERVICE
                .map(|s| s.to_string())
                .or_else(|| var("DATADOG_SERVICE").ok());

            let source = DATADOG_SOURCE
                .map(|s| s.to_string())
                .unwrap_or_else(|| var("DATADOG_SOURCE").unwrap_or_else(|_| "orm".to_string()));

            let config: DataDogConfig = DataDogConfig {
                apikey: api_key,
                tags: tags,
                service: service,
                source: source,
                hostname: var("HOSTNAME").ok(),
                http_config: http_config,
                ..DataDogConfig::default()
            };

            println!("DataDog config = {:#?}", config);

            let client = datadog_logs::client::HttpDataDogClient::new(&config)?;
            let nonblocking =
                DataDogLogger::set_nonblocking_logger(client, config, log::LevelFilter::Info)?;

            tokio::spawn(nonblocking);

            Ok(())
        }

        None => {
            if var("RUST_LOG").map_or_else(|_| false, |_| true) {
                env_logger::init()
            } else if cfg!(debug_assertions) {
                env_logger::Builder::new()
                    .filter_level(log::LevelFilter::Debug)
                    .init();
            } else {
                env_logger::Builder::new()
                    .filter_level(log::LevelFilter::Info)
                    .init();
            }

            Ok(())
        }
    }
}

impl From<DataDogLoggerError> for Error {
    fn from(dderr: DataDogLoggerError) -> Error {
        Error::new(format!("Datadog error: {}", dderr))
    }
}
