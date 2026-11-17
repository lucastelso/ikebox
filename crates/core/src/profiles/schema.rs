//! Profile file formats (ADR-0022): reading schema 1 and 2, writing the
//! lowest schema that expresses the profile.
//!
//! In memory a profile has the shape of schema 2 ([`Profile`]). Schema 1 had
//! one `auth.method` (`psk` or `psk-xauth`) with `local_id`/`remote_id`;
//! every schema 1 profile converts to schema 2 and back without loss. A
//! profile that uses something schema 1 cannot say (today: a one-time XAuth
//! password) is written as schema 2, so the profiles that colleagues on an
//! older version receive keep opening for them.

use serde::{Deserialize, Serialize};

use super::ProfileError;
use super::model::{
    AuthSettings, ChildSettings, IkeSettings, LocalMethod, NetworkSettings, Profile, RemoteMethod,
    SCHEMA_VERSION,
};

/// Oldest schema still read.
pub const OLDEST_SCHEMA_VERSION: u32 = 1;

/// Schema 1, as written by IPsec Client up to 0.5.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileV1 {
    schema_version: u32,
    id: String,
    name: String,
    gateway: String,
    ike: IkeSettings,
    auth: AuthV1,
    child: ChildSettings,
    #[serde(default)]
    network: NetworkSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum MethodV1 {
    Psk,
    PskXauth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthV1 {
    method: MethodV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    local_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_id: Option<String>,
}

impl From<ProfileV1> for Profile {
    fn from(v1: ProfileV1) -> Self {
        Self {
            id: v1.id,
            name: v1.name,
            gateway: v1.gateway,
            ike: v1.ike,
            auth: AuthSettings::psk(
                v1.auth.local_id,
                v1.auth.method == MethodV1::PskXauth,
                v1.auth.remote_id,
            ),
            child: v1.child,
            network: v1.network,
        }
    }
}

/// The profile in schema 1, when schema 1 can say it.
fn as_v1(p: &Profile) -> Option<ProfileV1> {
    let rounds: Vec<_> = p.auth.local.iter().map(|r| (r.method, r.otp)).collect();
    let method = match rounds.as_slice() {
        [(LocalMethod::Psk, false)] => MethodV1::Psk,
        [(LocalMethod::Psk, false), (LocalMethod::Xauth, false)] => MethodV1::PskXauth,
        _ => return None,
    };
    if p.auth.remote.method != RemoteMethod::Psk
        || p.auth.local.get(1).is_some_and(|r| r.id.is_some())
    {
        return None;
    }
    Some(ProfileV1 {
        schema_version: 1,
        id: p.id.clone(),
        name: p.name.clone(),
        gateway: p.gateway.clone(),
        ike: p.ike.clone(),
        auth: AuthV1 {
            method,
            local_id: p.auth.psk_id().map(str::to_owned),
            remote_id: p.auth.remote_id().map(str::to_owned),
        },
        child: p.child.clone(),
        network: p.network.clone(),
    })
}

/// Schema 2 on disk: the version first, then the profile.
#[derive(Serialize)]
struct ProfileV2<'a> {
    schema_version: u32,
    #[serde(flatten)]
    profile: &'a Profile,
}

/// Schema a profile is written in: the lowest one that expresses it.
#[must_use]
pub fn schema_of(profile: &Profile) -> u32 {
    if as_v1(profile).is_some() {
        1
    } else {
        SCHEMA_VERSION
    }
}

/// Reads a profile of any supported schema (without validating it) and
/// returns it with the schema it came in.
pub(super) fn from_json(json: &str) -> Result<(Profile, u32), ProfileError> {
    let mut value: serde_json::Value = serde_json::from_str(json)?;
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(ProfileError::MissingSchemaVersion)?;
    match version {
        1 => {
            let v1: ProfileV1 = serde_json::from_value(value)?;
            Ok((v1.into(), 1))
        }
        2 => {
            if let Some(map) = value.as_object_mut() {
                map.remove("schema_version");
            }
            Ok((serde_json::from_value(value)?, 2))
        }
        other => Err(ProfileError::UnsupportedVersion(other)),
    }
}

/// Validation paths of schema 2 as the fields of a schema 1 file, so that a
/// problem in a schema 1 file names a field that exists in it.
pub(super) fn v1_field(field: &str) -> String {
    if field == "auth.local[0].id" {
        "auth.local_id".to_owned()
    } else if field == "auth.remote.id" {
        "auth.remote_id".to_owned()
    } else if field.starts_with("auth.local") || field.starts_with("auth.remote") {
        "auth.method".to_owned()
    } else {
        field.to_owned()
    }
}

/// Serializes the profile in readable JSON, in the lowest schema that
/// expresses it.
///
/// # Errors
/// Only in pathological serde cases.
pub fn to_json(profile: &Profile) -> Result<String, ProfileError> {
    let mut json = match as_v1(profile) {
        Some(v1) => serde_json::to_string_pretty(&v1)?,
        None => serde_json::to_string_pretty(&ProfileV2 {
            schema_version: SCHEMA_VERSION,
            profile,
        })?,
    };
    json.push('\n');
    Ok(json)
}
