//! The shape of Roblox's own API dump, and where to get the current one.
//!
//! Only the fields this crate reads are declared. `serde` ignores the rest, so
//! a dump that grows a field does not break the build; a dump that *loses* one
//! this crate depends on does, which is the direction worth failing in.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeSet;

/// Roblox publishes the deployed Studio version here.
const VERSION_ENDPOINT: &str =
    "https://clientsettingscdn.roblox.com/v2/client-version/WindowsStudio64";

/// And the dump for a given deployment here, keyed by its upload id.
const DUMP_HOST: &str = "https://setup.rbxcdn.com";

#[derive(Debug, Deserialize)]
pub struct ClientVersion {
    /// Human-readable, such as `0.735.0.7351131`. Recorded in the manifest so a
    /// regenerated file can be traced back to an engine release.
    pub version: String,
    /// The id the dump is actually addressed by, such as
    /// `version-dcbeee682ce74ee0`. Unrelated in form to `version`.
    #[serde(rename = "clientVersionUpload")]
    pub upload: String,
}

#[derive(Debug, Deserialize)]
pub struct Dump {
    #[serde(rename = "Classes")]
    pub classes: Vec<Class>,
}

#[derive(Debug, Deserialize)]
pub struct Class {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Superclass")]
    pub superclass: Option<String>,
    #[serde(rename = "Members", default)]
    pub members: Vec<Member>,
    #[serde(rename = "Tags", default)]
    pub tags: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
pub struct Member {
    #[serde(rename = "MemberType")]
    pub member_type: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "ValueType")]
    pub value_type: Option<ValueType>,
    #[serde(rename = "Security")]
    pub security: Option<Security>,
    /// Present when writing the property requires a sandbox capability. This is
    /// the same restriction `ReflectionService` reports through `Permits`, and
    /// missing it is how `Instance.Capabilities` slips into a props table.
    #[serde(rename = "Capabilities")]
    pub capabilities: Option<Capabilities>,
    #[serde(rename = "Tags", default)]
    pub tags: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
pub struct ValueType {
    /// `Primitive`, `DataType`, `Enum` or `Class`.
    #[serde(rename = "Category")]
    pub category: String,
    #[serde(rename = "Name")]
    pub name: String,
}

/// `Security` is an object on properties (separate read and write levels) and
/// a bare string on functions, events and callbacks. Only properties matter
/// here, but the type has to accept both or the whole dump fails to parse.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Security {
    Split {
        #[serde(rename = "Write")]
        write: String,
    },
    Uniform(String),
}

impl Security {
    fn write(&self) -> &str {
        match self {
            Security::Split { write } => write,
            Security::Uniform(level) => level,
        }
    }
}

/// Like `Security`, this field changes shape by member kind: an object with
/// separate read and write lists on properties, a bare list on functions.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Capabilities {
    Split {
        /// Absent on most properties, and a list of required capability names
        /// on the few hundred that are gated.
        #[serde(rename = "Write")]
        write: Option<Vec<String>>,
    },
    Uniform(Vec<String>),
}

/// The capability that actually blocks an ordinary script.
///
/// `Capabilities` describes Roblox's *sandboxing* system, not assignability: it
/// names the capability a script must hold, and a script running outside a
/// sandboxed container holds every ordinary one. Treating a non-empty list as a
/// gate was too strict, and `StyleRule.Priority` is the counter-example that
/// proves it: the dump gives it `Write: ["UI"]`, yet ordinary game code assigns
/// it and the rule honours the new priority.
///
/// `CapabilityControl` is the exception, because it governs the sandbox itself:
/// `Instance.Capabilities` and `Instance.Sandboxed` are the two properties that
/// carry it, and neither belongs in a props table.
const SANDBOX_CONTROL: &str = "CapabilityControl";

impl Capabilities {
    fn gates_writing(&self) -> bool {
        let blocks = |list: &Vec<String>| list.iter().any(|c| c == SANDBOX_CONTROL);
        match self {
            Capabilities::Split { write } => write.as_ref().is_some_and(blocks),
            // Only reached for members that are not properties, which are
            // filtered out before this is asked.
            Capabilities::Uniform(list) => blocks(list),
        }
    }
}

impl Member {
    /// Whether ordinary Luau may assign this property.
    ///
    /// Four independent gates, each of which the engine enforces separately:
    /// the member has to be a property at all, writable without elevated
    /// security, writable without a sandbox capability, and not marked
    /// read-only or unscriptable.
    pub fn is_assignable(&self) -> bool {
        if self.member_type != "Property" {
            return false;
        }
        if self.security.as_ref().is_none_or(|s| s.write() != "None") {
            return false;
        }
        if self
            .capabilities
            .as_ref()
            .is_some_and(Capabilities::gates_writing)
        {
            return false;
        }
        !self.tags.contains("ReadOnly") && !self.tags.contains("NotScriptable")
    }

    pub fn is_deprecated(&self) -> bool {
        self.tags.contains("Deprecated")
    }

    /// `Hidden` is reported but never used to exclude: `TextLabel.Font` carries
    /// it while remaining the property most existing code sets.
    pub fn is_hidden(&self) -> bool {
        self.tags.contains("Hidden")
    }
}

/// The deployed Studio version, straight from Roblox.
pub fn current_version(agent: &ureq::Agent) -> Result<ClientVersion> {
    agent
        .get(VERSION_ENDPOINT)
        .call()
        .context("asking Roblox for the deployed Studio version")?
        .body_mut()
        .read_json()
        .context("decoding the client-version response")
}

/// The API dump for one deployment, as raw bytes so it can be hashed and
/// written verbatim before anything parses it.
pub fn fetch_dump(agent: &ureq::Agent, upload: &str) -> Result<Vec<u8>> {
    let url = format!("{DUMP_HOST}/{upload}-API-Dump.json");
    let mut response = agent
        .get(&url)
        .call()
        .with_context(|| format!("downloading {url}"))?;
    response
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .with_context(|| format!("reading {url}"))
}

pub fn parse(bytes: &[u8]) -> Result<Dump> {
    serde_json::from_slice(bytes).context("parsing the API dump")
}
