//! Bundle-tree path values for the product web surface.
//!
//! The frozen product addresses every bundle, collection, asset, resource, failed file and
//! scene through one of the six `AssetRipper.GUI.Web/Paths/` structs. Each value renders
//! itself as a short JSON object (`BundlePath` as `{"P":[...]}`, the bundle-indexed paths
//! as `{"B":{...},"I":n}`, `AssetPath` as `{"C":{...},"D":n}`, `ScenePath` as `{"C":{...}}`)
//! and travels in the `Path` query of the matching `/X/View` URL, percent-encoded with
//! `WebUtility.UrlEncode`. This module ports those shapes, the tree operations (`Parent`,
//! `GetChild`, `GetCollection`, `GetFailedFile`, `GetResource`, `GetAsset`) and the view-URL
//! builders exactly; the bundle-tree projection that resolves them against a loaded game is
//! still open, so resolution stops at the gate sequence the `*API.TryGet*FromQuery` methods
//! share (missing path, malformed JSON, nothing loaded) and reports the pending tree
//! explicitly once those gates pass.
//!
//! Documents are read through the shared [`crate::json`] reader, so the same bounds apply:
//! integral-only numbers (a fraction or exponent form is a parse error, matching the frozen
//! `System.Text.Json` integer handling), bounded strings and bounded nesting. Objects must
//! carry exactly the attributed keys in any order: unknown, missing or duplicated keys are
//! rejected. Like the frozen struct, a `null` backing array and an empty one both decode to
//! the root, so `"P":null` reads as the root path.

use std::fmt::{self, Display, Formatter};

use crate::json::{parse_json, JsonLimits, JsonValue};

/// Bounds for one path document: single-character keys at most, five nesting levels at
/// most (`AssetPath` nests `C`/`B`/`P`/array).
const PATH_JSON_LIMITS: JsonLimits = JsonLimits {
    max_string_bytes: 64,
    max_nesting_depth: 8,
};

/// Failure to read one of the path JSON documents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathParseError {
    message: String,
}

impl PathParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for PathParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PathParseError {}

/// A path through the nested bundles, from the game root to one bundle.
///
/// The empty path is the root: like the frozen struct, an empty constructor array and a
/// missing (`null`) backing array both decode to the root, and both encode as `{"P":[]}`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BundlePath {
    path: Vec<i32>,
}

impl BundlePath {
    /// The game-root path.
    pub fn root() -> Self {
        Self { path: Vec::new() }
    }

    /// The index trail from the game root to this bundle.
    pub fn indices(&self) -> &[i32] {
        &self.path
    }

    /// The number of bundle steps below the game root.
    pub fn depth(&self) -> usize {
        self.path.len()
    }

    /// Whether this is the game-root path.
    pub fn is_root(&self) -> bool {
        self.path.is_empty()
    }

    /// The parent bundle's path; the root (and a single-step path) maps to the root,
    /// matching the frozen `Parent` property (`Depth > 1 ? ... : default`).
    pub fn parent(&self) -> Self {
        if self.path.len() > 1 {
            Self {
                path: self.path[..self.path.len() - 1].to_vec(),
            }
        } else {
            Self::root()
        }
    }

    /// The child bundle reached through `index` below this bundle.
    pub fn child(&self, index: i32) -> Self {
        let mut path = self.path.clone();
        path.push(index);
        Self { path }
    }

    /// The collection with `index` inside this bundle.
    pub fn collection(&self, index: i32) -> CollectionPath {
        CollectionPath {
            bundle: self.clone(),
            index,
        }
    }

    /// The failed file with `index` inside this bundle.
    pub fn failed_file(&self, index: i32) -> FailedFilePath {
        FailedFilePath {
            bundle: self.clone(),
            index,
        }
    }

    /// The resource file with `index` inside this bundle.
    pub fn resource(&self, index: i32) -> ResourcePath {
        ResourcePath {
            bundle: self.clone(),
            index,
        }
    }

    /// Render the frozen `ToJson` document, e.g. `{"P":[2,0]}`.
    pub fn to_json(&self) -> String {
        let mut json = String::from("{\"P\":[");
        for (position, index) in self.path.iter().enumerate() {
            if position > 0 {
                json.push(',');
            }
            json.push_str(&index.to_string());
        }
        json.push_str("]}");
        json
    }

    /// Read a `ToJson` document the way `BundlePath.FromJson` does.
    pub fn from_json(json: &str) -> Result<Self, PathParseError> {
        let document = read_document(json, "BundlePath")?;
        let members = exact_keys(&document, json, "BundlePath", &["P"])?;
        let path = read_indices(members[0], json, "BundlePath")?;
        Ok(Self { path })
    }
}

impl Display for BundlePath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_json())
    }
}

/// A collection inside one bundle: `{"B":{...},"I":n}`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectionPath {
    /// The bundle holding the collection.
    pub bundle: BundlePath,
    /// The collection index inside that bundle.
    pub index: i32,
}

impl CollectionPath {
    /// The asset with `path_id` inside this collection.
    pub fn asset(&self, path_id: i64) -> AssetPath {
        AssetPath {
            collection: self.clone(),
            path_id,
        }
    }

    /// Render the frozen `ToJson` document.
    pub fn to_json(&self) -> String {
        format!("{{\"B\":{},\"I\":{}}}", self.bundle.to_json(), self.index)
    }

    /// Read a `ToJson` document the way `CollectionPath.FromJson` does.
    pub fn from_json(json: &str) -> Result<Self, PathParseError> {
        let document = read_document(json, "CollectionPath")?;
        let members = exact_keys(&document, json, "CollectionPath", &["B", "I"])?;
        let bundle = read_bundle(members[0], json, "CollectionPath")?;
        let index = read_i32(members[1], json, "CollectionPath")?;
        Ok(Self { bundle, index })
    }
}

impl Display for CollectionPath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_json())
    }
}

/// An asset inside one collection: `{"C":{...},"D":n}`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetPath {
    /// The collection holding the asset.
    pub collection: CollectionPath,
    /// The asset path ID inside that collection.
    pub path_id: i64,
}

impl AssetPath {
    /// Render the frozen `ToJson` document.
    pub fn to_json(&self) -> String {
        format!(
            "{{\"C\":{},\"D\":{}}}",
            self.collection.to_json(),
            self.path_id
        )
    }

    /// Read a `ToJson` document the way `AssetPath.FromJson` does.
    pub fn from_json(json: &str) -> Result<Self, PathParseError> {
        let document = read_document(json, "AssetPath")?;
        let members = exact_keys(&document, json, "AssetPath", &["C", "D"])?;
        let collection = read_collection(members[0], json, "AssetPath")?;
        let path_id = read_i64(members[1], json, "AssetPath")?;
        Ok(Self {
            collection,
            path_id,
        })
    }
}

impl Display for AssetPath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_json())
    }
}

/// A resource file inside one bundle: `{"B":{...},"I":n}`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourcePath {
    /// The bundle holding the resource.
    pub bundle: BundlePath,
    /// The resource index inside that bundle.
    pub index: i32,
}

impl ResourcePath {
    /// Render the frozen `ToJson` document.
    pub fn to_json(&self) -> String {
        format!("{{\"B\":{},\"I\":{}}}", self.bundle.to_json(), self.index)
    }

    /// Read a `ToJson` document the way `ResourcePath.FromJson` does.
    pub fn from_json(json: &str) -> Result<Self, PathParseError> {
        let (bundle, index) = indexed_bundle_json(json, "ResourcePath")?;
        Ok(Self { bundle, index })
    }
}

impl Display for ResourcePath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_json())
    }
}

/// A failed file inside one bundle: `{"B":{...},"I":n}`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailedFilePath {
    /// The bundle holding the failed file.
    pub bundle: BundlePath,
    /// The failed-file index inside that bundle.
    pub index: i32,
}

impl FailedFilePath {
    /// Render the frozen `ToJson` document.
    pub fn to_json(&self) -> String {
        format!("{{\"B\":{},\"I\":{}}}", self.bundle.to_json(), self.index)
    }

    /// Read a `ToJson` document the way `FailedFilePath.FromJson` does.
    pub fn from_json(json: &str) -> Result<Self, PathParseError> {
        let (bundle, index) = indexed_bundle_json(json, "FailedFilePath")?;
        Ok(Self { bundle, index })
    }
}

impl Display for FailedFilePath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_json())
    }
}

/// A scene rooted at its first collection: `{"C":{...}}`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenePath {
    /// The scene's first collection.
    pub first: CollectionPath,
}

impl ScenePath {
    /// Render the frozen `ToJson` document.
    pub fn to_json(&self) -> String {
        format!("{{\"C\":{}}}", self.first.to_json())
    }

    /// Read a `ToJson` document the way `ScenePath.FromJson` does.
    pub fn from_json(json: &str) -> Result<Self, PathParseError> {
        let document = read_document(json, "ScenePath")?;
        let members = exact_keys(&document, json, "ScenePath", &["C"])?;
        let first = read_collection(members[0], json, "ScenePath")?;
        Ok(Self { first })
    }
}

impl Display for ScenePath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_json())
    }
}

impl From<CollectionPath> for ScenePath {
    fn from(first: CollectionPath) -> Self {
        Self { first }
    }
}

/// The frozen record-struct `ToString` forms.
///
/// The frozen `*API` resolve-failure messages interpolate the path, which for the
/// record-struct paths renders the synthesized `ToString` (`CollectionPath { BundlePath =
/// ..., Index = ... }`), not the JSON document. `BundlePath` is the exception: it
/// overrides `ToString` with its JSON, so its `Display` is already the diagnostic form.
impl CollectionPath {
    /// Render the frozen `CollectionPath.ToString()`.
    pub fn to_diagnostic(&self) -> String {
        format!(
            "CollectionPath {{ BundlePath = {}, Index = {} }}",
            self.bundle, self.index
        )
    }
}

impl AssetPath {
    /// Render the frozen `AssetPath.ToString()`.
    pub fn to_diagnostic(&self) -> String {
        format!(
            "AssetPath {{ CollectionPath = {}, PathID = {} }}",
            self.collection.to_diagnostic(),
            self.path_id
        )
    }
}

impl ResourcePath {
    /// Render the frozen `ResourcePath.ToString()`.
    pub fn to_diagnostic(&self) -> String {
        format!(
            "ResourcePath {{ BundlePath = {}, Index = {} }}",
            self.bundle, self.index
        )
    }
}

impl FailedFilePath {
    /// Render the frozen `FailedFilePath.ToString()`.
    pub fn to_diagnostic(&self) -> String {
        format!(
            "FailedFilePath {{ BundlePath = {}, Index = {} }}",
            self.bundle, self.index
        )
    }
}

/// One of the six JSON path documents the `*API.TryGet*FromQuery` methods read.
pub trait PathDocument: Sized {
    /// Read a `ToJson` document the way the frozen `FromJson` does.
    fn from_json(json: &str) -> Result<Self, PathParseError>;
}

impl PathDocument for BundlePath {
    fn from_json(json: &str) -> Result<Self, PathParseError> {
        BundlePath::from_json(json)
    }
}

impl PathDocument for CollectionPath {
    fn from_json(json: &str) -> Result<Self, PathParseError> {
        CollectionPath::from_json(json)
    }
}

impl PathDocument for AssetPath {
    fn from_json(json: &str) -> Result<Self, PathParseError> {
        AssetPath::from_json(json)
    }
}

impl PathDocument for ResourcePath {
    fn from_json(json: &str) -> Result<Self, PathParseError> {
        ResourcePath::from_json(json)
    }
}

impl PathDocument for FailedFilePath {
    fn from_json(json: &str) -> Result<Self, PathParseError> {
        FailedFilePath::from_json(json)
    }
}

impl PathDocument for ScenePath {
    fn from_json(json: &str) -> Result<Self, PathParseError> {
        ScenePath::from_json(json)
    }
}

fn indexed_bundle_json(
    json: &str,
    type_name: &'static str,
) -> Result<(BundlePath, i32), PathParseError> {
    let document = read_document(json, type_name)?;
    let members = exact_keys(&document, json, type_name, &["B", "I"])?;
    let bundle = read_bundle(members[0], json, type_name)?;
    let index = read_i32(members[1], json, type_name)?;
    Ok((bundle, index))
}

/// Percent-encode a query value the way `WebUtility.UrlEncode` does for the characters the
/// product's paths contain.
pub fn url_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

/// The view URL the frozen `BundleAPI.GetViewUrl` builds for `path`.
pub fn bundle_view_url(path: &BundlePath) -> String {
    format!("/Bundles/View?Path={}", url_encode(&path.to_json()))
}

/// The view URL the frozen `SearchAPI.GetViewUrl` builds for `query`.
pub fn search_view_url(query: &str) -> String {
    format!("/Search/View?q={}", url_encode(query))
}

/// The view URL the frozen `CollectionAPI.GetViewUrl` builds for `path`.
pub fn collection_view_url(path: &CollectionPath) -> String {
    format!("/Collections/View?Path={}", url_encode(&path.to_json()))
}

/// The view URL the frozen `AssetAPI.GetViewUrl` builds for `path`.
pub fn asset_view_url(path: &AssetPath) -> String {
    format!("/Assets/View?Path={}", url_encode(&path.to_json()))
}

/// The view URL the frozen `ResourceAPI.GetViewUrl` builds for `path`.
pub fn resource_view_url(path: &ResourcePath) -> String {
    format!("/Resources/View?Path={}", url_encode(&path.to_json()))
}

/// The data URL the frozen `ResourceAPI.GetDataUrl` builds for `path`.
pub fn resource_data_url(path: &ResourcePath) -> String {
    format!("/Resources/Data?Path={}", url_encode(&path.to_json()))
}

/// The view URL the frozen `FailedFileAPI.GetViewUrl` builds for `path`.
pub fn failed_file_view_url(path: &FailedFilePath) -> String {
    format!("/FailedFiles/View?Path={}", url_encode(&path.to_json()))
}

/// The view URL the frozen `SceneAPI.GetViewUrl` builds for `path`.
pub fn scene_view_url(path: &ScenePath) -> String {
    format!("/Scenes/View?Path={}", url_encode(&path.to_json()))
}

/// Where the shared `TryGet*FromQuery` gate sequence stops for one request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ViewGate<T> {
    /// No `Path` query was submitted.
    MissingPath,
    /// The `Path` query is not a path document.
    InvalidPath(String),
    /// The path reads, but no game is loaded.
    NotLoaded,
    /// The path reads and a game is loaded; resolving it needs the bundle-tree
    /// projection, which is still open.
    Ready(T),
}

/// Run the gate sequence every frozen `*API.TryGet*FromQuery` runs.
///
/// `path_query` is the decoded `Path` query value (or `None` when absent) and `loaded`
/// reports `GameFileLoader.IsLoaded`. A `Ready` path still needs the bundle-tree
/// projection before it can render; the route layer reports that explicitly.
pub fn view_gate<T: PathDocument>(path_query: Option<&str>, loaded: bool) -> ViewGate<T> {
    let Some(json) = path_query else {
        return ViewGate::MissingPath;
    };
    if json.is_empty() {
        return ViewGate::MissingPath;
    }
    let path = match T::from_json(json) {
        Ok(path) => path,
        Err(error) => return ViewGate::InvalidPath(error.to_string()),
    };
    if !loaded {
        return ViewGate::NotLoaded;
    }
    ViewGate::Ready(path)
}

fn read_document(json: &str, type_name: &'static str) -> Result<JsonValue, PathParseError> {
    parse_json(json, PATH_JSON_LIMITS)
        .map_err(|error| PathParseError::new(format!("invalid {type_name} JSON {json}: {error}")))
}

/// Borrow the members of an object carrying exactly `keys`, in any order.
///
/// Unknown, missing or duplicated keys are rejected: the frozen deserializer binds only
/// the attributed properties, and a document carrying anything else is not a path.
fn exact_keys<'a>(
    value: &'a JsonValue,
    json: &str,
    type_name: &'static str,
    keys: &[&str],
) -> Result<Vec<&'a JsonValue>, PathParseError> {
    let members = match value {
        JsonValue::Object(members) => members,
        _ => {
            return Err(PathParseError::new(format!(
                "invalid {type_name} JSON {json}: expected an object"
            )));
        }
    };
    if members.len() != keys.len() {
        return Err(PathParseError::new(format!(
            "invalid {type_name} JSON {json}: expected properties {}",
            keys.join(", ")
        )));
    }
    let mut values = Vec::with_capacity(keys.len());
    for key in keys {
        let mut matches = members.iter().filter(|(name, _)| name == key);
        let Some((_, value)) = matches.next() else {
            return Err(PathParseError::new(format!(
                "invalid {type_name} JSON {json}: missing the {key} property"
            )));
        };
        if matches.next().is_some() {
            return Err(PathParseError::new(format!(
                "invalid {type_name} JSON {json}: duplicated the {key} property"
            )));
        }
        values.push(value);
    }
    Ok(values)
}

fn read_i64(value: &JsonValue, json: &str, type_name: &'static str) -> Result<i64, PathParseError> {
    match value {
        JsonValue::Integer(number) => Ok(*number),
        _ => Err(PathParseError::new(format!(
            "invalid {type_name} JSON {json}: expected an integer number"
        ))),
    }
}

fn read_i32(value: &JsonValue, json: &str, type_name: &'static str) -> Result<i32, PathParseError> {
    let number = read_i64(value, json, type_name)?;
    i32::try_from(number).map_err(|_| {
        PathParseError::new(format!(
            "invalid {type_name} JSON {json}: {number} is out of range"
        ))
    })
}

/// Read the `{"P":[...]}` bundle shape, accepting `null` as the root like the frozen
/// struct's missing backing array does.
fn read_bundle(
    value: &JsonValue,
    json: &str,
    type_name: &'static str,
) -> Result<BundlePath, PathParseError> {
    if matches!(value, JsonValue::Null) {
        return Ok(BundlePath::root());
    }
    let members = exact_keys(value, json, type_name, &["P"])?;
    Ok(BundlePath {
        path: read_indices(members[0], json, type_name)?,
    })
}

fn read_indices(
    value: &JsonValue,
    json: &str,
    type_name: &'static str,
) -> Result<Vec<i32>, PathParseError> {
    if matches!(value, JsonValue::Null) {
        return Ok(Vec::new());
    }
    let items = match value {
        JsonValue::Array(items) => items,
        _ => {
            return Err(PathParseError::new(format!(
                "invalid {type_name} JSON {json}: expected an integer array"
            )));
        }
    };
    items
        .iter()
        .map(|item| read_i32(item, json, type_name))
        .collect()
}

fn read_collection(
    value: &JsonValue,
    json: &str,
    type_name: &'static str,
) -> Result<CollectionPath, PathParseError> {
    let members = match value {
        JsonValue::Object(_) => exact_keys(value, json, type_name, &["B", "I"])?,
        _ => {
            return Err(PathParseError::new(format!(
                "invalid {type_name} JSON {json}: expected a collection path"
            )));
        }
    };
    Ok(CollectionPath {
        bundle: read_bundle(members[0], json, type_name)?,
        index: read_i32(members[1], json, type_name)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_path_json_shapes_match_the_frozen_serializer() {
        assert_eq!(BundlePath::root().to_json(), "{\"P\":[]}");
        assert_eq!(
            BundlePath::root().child(2).child(0).to_json(),
            "{\"P\":[2,0]}"
        );
        // Whitespace around the fixed shape still reads.
        assert_eq!(
            BundlePath::from_json("{ \"P\" : [ 2 , 0 ] }"),
            Ok(BundlePath::root().child(2).child(0))
        );
        // The frozen struct treats an empty and a missing backing array the same.
        assert_eq!(BundlePath::from_json("{\"P\":[]}"), Ok(BundlePath::root()));
        assert_eq!(
            BundlePath::from_json("{\"P\":null}"),
            Ok(BundlePath::root())
        );
        // Key order is insignificant to the frozen deserializer.
        assert_eq!(
            CollectionPath::from_json("{\"I\":3,\"B\":{\"P\":[1]}}"),
            Ok(CollectionPath {
                bundle: BundlePath::root().child(1),
                index: 3,
            })
        );
        assert_eq!(BundlePath::root().to_string(), "{\"P\":[]}");
    }

    #[test]
    fn bundle_path_tree_operations_match_the_frozen_struct() {
        let root = BundlePath::root();
        assert!(root.is_root());
        assert_eq!(root.depth(), 0);
        assert_eq!(root.parent(), BundlePath::root());

        let child = root.child(3);
        assert!(!child.is_root());
        assert_eq!(child.depth(), 1);
        assert_eq!(child.indices(), &[3]);
        // A single-step path has no stored parent, so it maps back to the root.
        assert_eq!(child.parent(), BundlePath::root());

        let grandchild = child.child(-1);
        assert_eq!(grandchild.indices(), &[3, -1]);
        assert_eq!(grandchild.parent(), child);

        let collection = grandchild.collection(7);
        assert_eq!(collection.to_json(), "{\"B\":{\"P\":[3,-1]},\"I\":7}");
        assert_eq!(
            grandchild.failed_file(1).to_json(),
            "{\"B\":{\"P\":[3,-1]},\"I\":1}"
        );
        assert_eq!(
            grandchild.resource(2).to_json(),
            "{\"B\":{\"P\":[3,-1]},\"I\":2}"
        );
    }

    #[test]
    fn nested_path_json_shapes_match_the_frozen_serializer() {
        let collection = CollectionPath {
            bundle: BundlePath::root().child(1),
            index: 3,
        };
        assert_eq!(collection.to_json(), "{\"B\":{\"P\":[1]},\"I\":3}");
        assert_eq!(
            CollectionPath::from_json(&collection.to_json()),
            Ok(collection.clone())
        );

        let asset = AssetPath {
            collection: CollectionPath {
                bundle: BundlePath::root(),
                index: 0,
            },
            path_id: 42,
        };
        assert_eq!(
            asset.to_json(),
            "{\"C\":{\"B\":{\"P\":[]},\"I\":0},\"D\":42}"
        );
        assert_eq!(AssetPath::from_json(&asset.to_json()), Ok(asset.clone()));
        assert_eq!(
            asset.collection.asset(42),
            AssetPath {
                collection: asset.collection.clone(),
                path_id: 42,
            }
        );

        let resource = ResourcePath {
            bundle: BundlePath::root().child(4),
            index: 5,
        };
        assert_eq!(resource.to_json(), "{\"B\":{\"P\":[4]},\"I\":5}");
        assert_eq!(ResourcePath::from_json(&resource.to_json()), Ok(resource));

        let failed = FailedFilePath {
            bundle: BundlePath::root(),
            index: 6,
        };
        assert_eq!(failed.to_json(), "{\"B\":{\"P\":[]},\"I\":6}");
        assert_eq!(FailedFilePath::from_json(&failed.to_json()), Ok(failed));

        let scene = ScenePath::from(collection.clone());
        assert_eq!(scene.to_json(), "{\"C\":{\"B\":{\"P\":[1]},\"I\":3}}");
        assert_eq!(ScenePath::from_json(&scene.to_json()), Ok(scene));
    }

    #[test]
    fn malformed_path_json_is_rejected() {
        // Each shape requires exactly its attributed keys.
        for document in [
            "{}",
            "{\"P\":[1],\"X\":2}",
            "{\"Q\":[]}",
            "{\"P\":{}}",
            "{\"P\":[1.5]}",
            "{\"P\":[1e2]}",
            "{\"P\":[\"1\"]}",
            "{\"P\":[1,]}",
            "{\"P\":[]},",
            "{\"P\":[2147483648]}",
            "{\"p\":[]}",
            "{\"P\":[1],\"P\":[]}",
            "{\"B\":{\"P\":[]},\"I\":1,\"J\":2}",
            "{\"C\":{\"B\":{\"P\":[]},\"I\":0},\"D\":\"42\"}",
        ] {
            assert!(
                BundlePath::from_json(document).is_err()
                    || CollectionPath::from_json(document).is_err()
                    || AssetPath::from_json(document).is_err(),
                "{document}"
            );
        }
        assert!(BundlePath::from_json("{\"P\":[]} ").is_ok());
        assert!(BundlePath::from_json("").is_err());
        assert!(CollectionPath::from_json("{\"B\":{\"P\":[]}}").is_err());
        assert!(AssetPath::from_json("{\"C\":{\"B\":{\"P\":[]},\"I\":0}}").is_err());
        assert!(ScenePath::from_json("{\"C\":{}}").is_err());
        assert!(ResourcePath::from_json("{\"B\":{},\"I\":0}").is_err());
        assert!(FailedFilePath::from_json("not json").is_err());
        // A 65-byte key blows the path-document string bound.
        let long_key = "K".repeat(65);
        assert!(BundlePath::from_json(&format!("{{\"{long_key}\":[]}}")).is_err());
    }

    #[test]
    fn view_urls_match_the_frozen_api_builders() {
        assert_eq!(
            bundle_view_url(&BundlePath::root()),
            "/Bundles/View?Path=%7B%22P%22%3A%5B%5D%7D"
        );
        assert_eq!(
            collection_view_url(&CollectionPath {
                bundle: BundlePath::root(),
                index: 2,
            }),
            "/Collections/View?Path=%7B%22B%22%3A%7B%22P%22%3A%5B%5D%7D%2C%22I%22%3A2%7D"
        );
        assert_eq!(
            asset_view_url(&AssetPath {
                collection: CollectionPath {
                    bundle: BundlePath::root(),
                    index: 0,
                },
                path_id: 7,
            }),
            "/Assets/View?Path=%7B%22C%22%3A%7B%22B%22%3A%7B%22P%22%3A%5B%5D%7D%2C%22I%22%3A0%7D%2C%22D%22%3A7%7D"
        );
        assert_eq!(
            resource_view_url(&ResourcePath {
                bundle: BundlePath::root(),
                index: 1,
            }),
            "/Resources/View?Path=%7B%22B%22%3A%7B%22P%22%3A%5B%5D%7D%2C%22I%22%3A1%7D"
        );
        assert_eq!(
            resource_data_url(&ResourcePath {
                bundle: BundlePath::root(),
                index: 1,
            }),
            "/Resources/Data?Path=%7B%22B%22%3A%7B%22P%22%3A%5B%5D%7D%2C%22I%22%3A1%7D"
        );
        assert_eq!(
            failed_file_view_url(&FailedFilePath {
                bundle: BundlePath::root(),
                index: 1,
            }),
            "/FailedFiles/View?Path=%7B%22B%22%3A%7B%22P%22%3A%5B%5D%7D%2C%22I%22%3A1%7D"
        );
        assert_eq!(
            scene_view_url(&ScenePath::from(CollectionPath {
                bundle: BundlePath::root(),
                index: 0,
            })),
            "/Scenes/View?Path=%7B%22C%22%3A%7B%22B%22%3A%7B%22P%22%3A%5B%5D%7D%2C%22I%22%3A0%7D%7D"
        );
        assert_eq!(
            search_view_url("MonoBehaviour"),
            "/Search/View?q=MonoBehaviour"
        );
        assert_eq!(search_view_url("a b+c"), "/Search/View?q=a%20b%2Bc");
    }

    #[test]
    fn diagnostics_match_the_frozen_record_to_string_forms() {
        let collection = CollectionPath {
            bundle: BundlePath::root().child(1),
            index: 3,
        };
        assert_eq!(
            collection.to_diagnostic(),
            "CollectionPath { BundlePath = {\"P\":[1]}, Index = 3 }"
        );
        assert_eq!(
            AssetPath {
                collection,
                path_id: 42,
            }
            .to_diagnostic(),
            "AssetPath { CollectionPath = CollectionPath { BundlePath = {\"P\":[1]}, Index = 3 }, PathID = 42 }"
        );
        assert_eq!(
            ResourcePath {
                bundle: BundlePath::root(),
                index: 1,
            }
            .to_diagnostic(),
            "ResourcePath { BundlePath = {\"P\":[]}, Index = 1 }"
        );
        assert_eq!(
            FailedFilePath {
                bundle: BundlePath::root(),
                index: 1,
            }
            .to_diagnostic(),
            "FailedFilePath { BundlePath = {\"P\":[]}, Index = 1 }"
        );
    }

    #[test]
    fn the_view_gate_follows_the_frozen_query_sequence() {
        assert_eq!(view_gate::<BundlePath>(None, false), ViewGate::MissingPath);
        assert_eq!(
            view_gate::<BundlePath>(Some(""), false),
            ViewGate::MissingPath
        );
        assert!(matches!(
            view_gate::<BundlePath>(Some("{"), false),
            ViewGate::InvalidPath(_)
        ));
        assert_eq!(
            view_gate::<BundlePath>(Some("{\"P\":[]}"), false),
            ViewGate::NotLoaded
        );
        assert_eq!(
            view_gate::<BundlePath>(Some("{\"P\":[1]}"), true),
            ViewGate::Ready(BundlePath::root().child(1))
        );
        assert!(matches!(
            view_gate::<AssetPath>(Some("{\"P\":[]}"), true),
            ViewGate::InvalidPath(_)
        ));
    }
}
