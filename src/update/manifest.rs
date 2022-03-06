use std::fmt::{Display, Formatter};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Pattern(pub String);

#[derive(Debug, Deserialize, Eq)]
pub struct Version(pub String);

impl Display for Version {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let Version(v) = self;

        write!(formatter, "{}", v)
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        let Version(a) = self;
        let Version(b) = other;

        a == b
    }
}

#[derive(Debug, Deserialize)]
pub struct Path(String);

#[derive(Debug, Deserialize)]
pub struct Device {
    pub pattern: Pattern,
    pub version: Version,
}

#[derive(Deserialize)]
pub struct Manifest {
    pub object_type: String,
    pub devices: Vec<Device>,
}

impl Display for Manifest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let devices: Vec<String> = self
            .devices
            .iter()
            .map(|d| {
                let Pattern(p) = &d.pattern;

                format!("{} = {}", p, d.version)
            })
            .collect();

        write!(
            formatter,
            r#"[meta]
object_type = {}

[devices]
{}"#,
            self.object_type,
            devices.join("\n")
        )
    }
}
