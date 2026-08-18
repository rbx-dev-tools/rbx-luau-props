//! Compiles React-Luau prop types for Roblox UI classes from Roblox's API dump.
//!
//! The dump is the engine's own description of itself, published per Studio
//! deployment, so the generated types describe what Roblox actually accepts
//! rather than what someone remembered to write down. Nothing here enumerates
//! classes or properties: a UI class Roblox ships appears on the next refresh,
//! and a property it removes takes its field with it.

pub mod dump;
pub mod emit;
pub mod ir;
pub mod ty;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

pub const OUTPUT: &str = "generated";
pub const TYPES: &str = "UiProps.luau";
pub const MANIFEST: &str = "manifest.json";
pub const VENDORED_DUMP: &str = "vendor/API-Dump.json";

/// What produced the current output, so a regenerated file can be traced back
/// to an engine release and an unnoticed dump edit fails `check`.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    /// Human-readable Studio version, such as `0.735.0.7351131`.
    pub roblox_version: String,
    /// The deployment id the dump was fetched under.
    pub roblox_upload: String,
    pub dump_sha256: String,
    pub types_sha256: String,
    pub classes: usize,
    pub properties: usize,
    pub skipped_deprecated: usize,
}

pub struct Artifacts {
    pub types: String,
    pub manifest: Manifest,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub style: emit::Style,
    pub include_deprecated: bool,
}

fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut acc, byte| {
            let _ = write!(acc, "{byte:02x}");
            acc
        })
}

/// Compile the types from the dump vendored in the repository.
pub fn generate(root: &Path, config: &Config) -> Result<Artifacts> {
    let dump_path = root.join(VENDORED_DUMP);
    let bytes = fs::read(&dump_path).with_context(|| {
        format!(
            "reading {}; run `react-luau-props fetch` first",
            dump_path.display()
        )
    })?;

    let previous = read_manifest(root).ok();
    let parsed = dump::parse(&bytes)?;
    let surface = ir::build(
        &parsed,
        ir::Options {
            include_deprecated: config.include_deprecated,
        },
    )?;
    let emitted = emit::emit(&surface, &config.style);

    // The version is not in the dump itself, so it is carried by the manifest
    // written when the dump was fetched.
    let (roblox_version, roblox_upload) = match previous {
        Some(manifest) => (manifest.roblox_version, manifest.roblox_upload),
        None => ("unknown".to_owned(), "unknown".to_owned()),
    };

    Ok(Artifacts {
        manifest: Manifest {
            roblox_version,
            roblox_upload,
            dump_sha256: digest(&bytes),
            types_sha256: digest(emitted.source.as_bytes()),
            classes: emitted.classes,
            properties: emitted.properties,
            skipped_deprecated: surface.skipped_deprecated,
        },
        types: emitted.source,
    })
}

pub fn write(root: &Path, artifacts: &Artifacts) -> Result<()> {
    let out = root.join(OUTPUT);
    fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;

    fs::write(out.join(TYPES), &artifacts.types)?;
    fs::write(
        out.join(MANIFEST),
        serde_json::to_string_pretty(&artifacts.manifest)? + "\n",
    )?;
    Ok(())
}

pub fn read_manifest(root: &Path) -> Result<Manifest> {
    let path = root.join(OUTPUT).join(MANIFEST);
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Recompile and compare against what is committed.
///
/// This is the guard that makes the committed file trustworthy: an edited
/// output, a half-refreshed dump, or a generator change nobody regenerated for
/// all fail here rather than reaching a consumer.
pub fn check(root: &Path, config: &Config) -> Result<()> {
    let fresh = generate(root, config)?;
    let committed_types =
        fs::read_to_string(root.join(OUTPUT).join(TYPES)).context("reading the committed types")?;

    if committed_types != fresh.types {
        bail!("{OUTPUT}/{TYPES} is out of date; run `react-luau-props generate`");
    }

    let committed_manifest = read_manifest(root)?;
    if committed_manifest.types_sha256 != fresh.manifest.types_sha256
        || committed_manifest.dump_sha256 != fresh.manifest.dump_sha256
    {
        bail!("{OUTPUT}/{MANIFEST} does not describe the committed files");
    }

    Ok(())
}

/// Whether the vendored dump still describes the deployed Studio.
///
/// `check` on its own proves the committed file matches the vendored dump. It
/// cannot know Roblox has moved on, because nothing local changes when it does;
/// this is the question that needs the network.
pub fn behind_upstream(root: &Path) -> Result<Option<(String, String)>> {
    let vendored = read_manifest(root)?.roblox_version;
    let agent = ureq::Agent::new_with_defaults();
    let deployed = dump::current_version(&agent)?.version;

    if vendored == deployed {
        Ok(None)
    } else {
        Ok(Some((vendored, deployed)))
    }
}

/// Download the dump for the currently deployed Studio and vendor it.
pub fn fetch(root: &Path) -> Result<(String, String)> {
    let agent = ureq::Agent::new_with_defaults();
    let version = dump::current_version(&agent)?;
    let bytes = dump::fetch_dump(&agent, &version.upload)?;

    // Parsing before writing means a truncated or wrong-shaped download never
    // replaces a good vendored dump.
    let parsed = dump::parse(&bytes)?;
    if parsed.classes.is_empty() {
        bail!("the downloaded dump has no classes");
    }

    let path = root.join(VENDORED_DUMP);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, &bytes).with_context(|| format!("writing {}", path.display()))?;

    // Recorded now, because the dump alone does not say which release it is.
    let out = root.join(OUTPUT);
    fs::create_dir_all(&out)?;
    let manifest = Manifest {
        roblox_version: version.version.clone(),
        roblox_upload: version.upload.clone(),
        dump_sha256: digest(&bytes),
        types_sha256: String::new(),
        classes: 0,
        properties: 0,
        skipped_deprecated: 0,
    };
    fs::write(
        out.join(MANIFEST),
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;

    Ok((version.version, version.upload))
}

/// The nearest ancestor holding a `Cargo.toml` for this crate.
pub fn find_root(start: &Path) -> Result<PathBuf> {
    for candidate in start.ancestors() {
        if candidate.join("Cargo.toml").is_file() {
            return Ok(candidate.to_path_buf());
        }
    }
    bail!("no crate root above {}", start.display())
}
