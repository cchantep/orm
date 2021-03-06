# Orm

The [Great God of the Strict Authorized Ormits](https://discworld.fandom.com/wiki/Gods#Orm), usually found residing in one of the Nether Hells, or an simple application update utility.

## Build

    cargo build

[![Rust](https://github.com/cchantep/orm/actions/workflows/ci.yml/badge.svg)](https://github.com/cchantep/orm/actions/workflows/ci.yml)

The following environment variables must be defined at compile-time.

- `OBJECT_TYPE` (`string`) - The object type (corresponding to IoT core).
- `YAML_MANIFEST_URL` (`string`) - The URL to [YAML manifest](#yaml-manifest).
- `APPLICATION_NAME` (`string`) - The name of managed application.
- `LOCAL_PREFIX` (`string`) - The prefix path.

*Example:*

If,

- `YAML_MANIFEST_URL` is `http://bar/manifest.yaml`,
- `APPLICATION_NAME` is `foo`,
- `LOCAL_PREFIX` is '/tmp`,

then the following must be satisfied.

- The application archives must be at `http://bar`; e.g. `http://bar/foo-1.2.3.tar.gz` if version is `1.2.3`.
- The all the entries inside an application archive must be prefixed the `APPLICATION_NAME`; e.g. `foo/run.sh` must be found in such archive.
  - A `{APPLICATION_NAME}/run.sh` is required as start script.
  - A `{APPLICATION_NAME}/id.sh` is required to resolve the device (thing) ID.
- The `LOCAL_PREFIX` must be a local directory, and must be writable.
- The local application directory will be `/tmp/foo`.

## Usage

No runtime configuration or setting is required.

    /path/to/orm

Either execute the current version if up-to-date, or update before as bellow.

![Update workflow](https://cchantep.github.io/orm/update.png)

### YAML manifest

The update manifest must be a valid YAML file, accessible by HTTP GET.

Example:

```yaml
---
object_type: 'FOO'

devices:
  - pattern: foo.*
    version: 1.2.3
  - pattern: strict_id
    version: "0.1"
```

- `object_type` (`string`) - Must be the same as `OBJECT_TYPE`.
- `devices` - List of device settings, orderly checked against the local device.
  - `pattern` (`string`) - Regular expression to match against local thing ID.
  - `version` (`string`) - Application version.

### Settings

**`RUST_LOG`:**

The [Rust logging](https://rust-lang-nursery.github.io/rust-cookbook/development_tools/debugging/config_log.html) is used and can enabled at runtime by setting `RUST_LOG` environment variables.

    export RUST_LOG=info

**[DataDog logging](https://docs.datadoghq.com/logs/):**

The following environment variables can be set to enable logging to DataDog.

- `DATADOG_API_URL` & `DATADOG_API_KEY` (`string`) - Required API URL (`.com` or `.eu` according the associated API key), and the API key.
- `DATADOG_TAGS` (`string`) - Optional comma separated list of DataDog tags.
- `DATADOG_SERVICE` (`string`) - Optional service name.
- `DATADOG_SOURCE` (`string`) - Optional source name (default: `orm`).
- `HOSTNAME` (`string`) - Optional unique hostname.

> Except `HOSTNAME` that is only resolved at runtime, the DataDog settings can be set at compile-time.