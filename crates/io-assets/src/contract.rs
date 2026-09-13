//! Versioned, tool-independent asset delivery contract. Build recipes are not
//! executed by the importer; packages contain only data and exported geometry.
use crate::Model;
use io_types::{Bounds, Vec3};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetConfig {
    pub name: String,
    pub file: Option<PathBuf>,
    pub builtin: Option<BuiltinMesh>,
    pub kind: Option<MeshKind>,
    pub clips: Option<Vec<String>>,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinMesh {
    Box,
}

pub enum MeshSource<'a> {
    File(&'a Path),
    Builtin(BuiltinMesh),
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeshKind {
    Static,
    Skinned,
}

impl AssetConfig {
    pub fn source(&self) -> Result<MeshSource<'_>, String> {
        match (&self.file, self.builtin) {
            (Some(path), None) if !path.as_os_str().is_empty() => Ok(MeshSource::File(path)),
            (None, Some(kind)) => Ok(MeshSource::Builtin(kind)),
            _ => Err(format!(
                "asset {} needs exactly one file or builtin",
                self.name
            )),
        }
    }
    pub fn validate_model(&self, model: &Model) -> Result<(), String> {
        if let Some(kind) = self.kind {
            if (kind == MeshKind::Skinned) != (model.joint_count() > 0) {
                return Err(format!(
                    "asset {}: declared kind does not match imported skeleton",
                    self.name
                ));
            }
        }
        if let Some(clips) = &self.clips {
            let expected: HashSet<_> = clips.iter().map(String::as_str).collect();
            let actual: HashSet<_> = model.clips.iter().map(|c| c.name.as_str()).collect();
            if expected.len() != clips.len()
                || actual.len() != model.clips.len()
                || expected.contains("")
                || expected != actual
            {
                return Err(format!(
                    "asset {}: declared clips {:?} must match unique imported clips {:?}",
                    self.name,
                    clips,
                    model.clips.iter().map(|c| &c.name).collect::<Vec<_>>()
                ));
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppearanceConfig {
    pub name: String,
    pub base_asset: String,
    #[serde(default)]
    pub lods: Vec<LodConfig>,
    #[serde(default)]
    pub states: Vec<VisualStateConfig>,
    #[serde(default = "default_hysteresis")]
    pub hysteresis: f32,
    pub occupancy_bounds: Option<BoundsConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundsConfig {
    pub min: [f32; 3],
    pub max: [f32; 3],
}
impl BoundsConfig {
    pub fn resolve(&self) -> Result<Bounds, String> {
        if (0..3).any(|i| {
            !self.min[i].is_finite() || !self.max[i].is_finite() || self.min[i] > self.max[i]
        }) {
            return Err("occupancy bounds must be finite with min <= max".into());
        }
        Ok(Bounds {
            min: Vec3::new(self.min[0], self.min[1], self.min[2]),
            max: Vec3::new(self.max[0], self.max[1], self.max[2]),
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualStateConfig {
    pub name: String,
    pub lods: Vec<LodConfig>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LodConfig {
    pub asset: String,
    pub min_screen_pixels: f32,
}
fn default_hysteresis() -> f32 {
    0.1
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageImport {
    pub namespace: String,
    pub file: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetPackage {
    pub format: PackageFormat,
    pub version: u32,
    pub name: String,
    pub coordinates: Coordinates,
    pub requires: Vec<Feature>,
    pub assets: Vec<AssetConfig>,
    pub appearances: Vec<AppearanceConfig>,
}

#[derive(Deserialize)]
pub enum PackageFormat {
    #[serde(rename = "io.asset-package")]
    AssetPackage,
}
#[derive(Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    OpaqueMaterials,
    SkeletalAnimation,
}
#[derive(Deserialize)]
pub enum Units {
    #[serde(rename = "meters")]
    Meters,
}
#[derive(Deserialize)]
pub enum UpAxis {
    #[serde(rename = "+Y")]
    Y,
}
#[derive(Deserialize)]
pub enum Handedness {
    #[serde(rename = "right")]
    Right,
}
#[derive(Deserialize)]
pub enum Origin {
    #[serde(rename = "authored")]
    Authored,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coordinates {
    pub units: Units,
    pub up_axis: UpAxis,
    pub handedness: Handedness,
    pub origin: Origin,
}

pub fn valid_name(name: &str) -> bool {
    name.as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}

impl AssetPackage {
    pub fn read(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let package: Self =
            serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        package
            .validate()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(package)
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("expected io.asset-package contract version 1".into());
        }
        if !valid_name(&self.name) {
            return Err("invalid package name".into());
        }
        let mut required = HashSet::new();
        for feature in &self.requires {
            if !required.insert(*feature) {
                return Err(format!("duplicate required feature: {feature:?}"));
            }
        }
        if !required.contains(&Feature::OpaqueMaterials) {
            return Err("package must require opaque_materials".into());
        }
        if self.assets.is_empty() || self.appearances.is_empty() {
            return Err("package needs at least one asset and one appearance".into());
        }
        let mut names = HashSet::new();
        for asset in &self.assets {
            if !valid_name(&asset.name) || !names.insert(asset.name.as_str()) {
                return Err(format!("invalid or duplicate asset name: {}", asset.name));
            }
            let Some(kind) = asset.kind else {
                return Err(format!("asset {} must declare kind", asset.name));
            };
            let Some(clips) = &asset.clips else {
                return Err(format!(
                    "asset {} must declare clips (use [] for none)",
                    asset.name
                ));
            };
            if kind == MeshKind::Skinned && !required.contains(&Feature::SkeletalAnimation) {
                return Err("skinned assets must require skeletal_animation".into());
            }
            if kind == MeshKind::Static && !clips.is_empty() {
                return Err(format!(
                    "static asset {} cannot declare skeletal clips",
                    asset.name
                ));
            }
            match asset.source()? {
                MeshSource::File(file)
                    if !file.as_os_str().is_empty()
                        && file.components().all(|c| matches!(c, Component::Normal(_))) =>
                {
                    if !matches!(
                        file.extension().and_then(|s| s.to_str()),
                        Some("glb" | "gltf")
                    ) {
                        return Err(format!("asset {}: expected .glb or .gltf", asset.name));
                    }
                }
                MeshSource::Builtin(BuiltinMesh::Box) => {}
                _ => {
                    return Err(format!(
                        "asset {} needs a package-relative file without '..', or builtin box",
                        asset.name
                    ))
                }
            }
        }
        let mut appearances = HashSet::new();
        for appearance in &self.appearances {
            if !valid_name(&appearance.name) || !appearances.insert(&appearance.name) {
                return Err(format!(
                    "invalid or duplicate appearance name: {}",
                    appearance.name
                ));
            }
            for reference in std::iter::once(&appearance.base_asset)
                .chain(appearance.lods.iter().map(|lod| &lod.asset))
                .chain(
                    appearance
                        .states
                        .iter()
                        .flat_map(|s| s.lods.iter().map(|lod| &lod.asset)),
                )
            {
                if !names.contains(reference.as_str()) {
                    return Err(format!(
                        "appearance {} references unknown local asset: {reference}",
                        appearance.name
                    ));
                }
            }
        }
        Ok(())
    }

    /// Qualify local references exactly once. Mesh paths are based on the package,
    /// independent of the importing scene's directory and process working directory.
    pub fn into_definitions(
        mut self,
        manifest: &Path,
        namespace: &str,
    ) -> Result<(Vec<AssetConfig>, Vec<AppearanceConfig>), String> {
        self.validate()?;
        if !valid_name(namespace) {
            return Err(format!("invalid package namespace: {namespace}"));
        }
        let root = manifest
            .parent()
            .unwrap_or(Path::new("."))
            .canonicalize()
            .map_err(|e| format!("{}: {e}", manifest.display()))?;
        for asset in &mut self.assets {
            if let Some(file) = &mut asset.file {
                let resolved = root.join(&*file).canonicalize().map_err(|e| {
                    format!(
                        "package {namespace}, asset {} ({}): {e}",
                        asset.name,
                        file.display()
                    )
                })?;
                if !resolved.starts_with(&root) {
                    return Err(format!(
                        "asset {} resolves outside package directory",
                        asset.name
                    ));
                }
                validate_dependencies(&resolved, &root)?;
                *file = resolved;
            }
            asset.name = format!("{namespace}/{}", asset.name);
        }
        for appearance in &mut self.appearances {
            appearance.name = format!("{namespace}/{}", appearance.name);
            appearance.base_asset = format!("{namespace}/{}", appearance.base_asset);
            for lod in appearance
                .lods
                .iter_mut()
                .chain(appearance.states.iter_mut().flat_map(|s| &mut s.lods))
            {
                lod.asset = format!("{namespace}/{}", lod.asset);
            }
        }
        Ok((self.assets, self.appearances))
    }
}

fn validate_dependencies(path: &Path, root: &Path) -> Result<(), String> {
    let document = gltf::Gltf::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(extension) = document.extensions_required().next() {
        return Err(format!(
            "{}: unsupported required glTF extension: {extension}",
            path.display()
        ));
    }
    let uris = document
        .buffers()
        .filter_map(|b| match b.source() {
            gltf::buffer::Source::Uri(uri) => Some(uri),
            gltf::buffer::Source::Bin => None,
        })
        .chain(document.images().filter_map(|i| match i.source() {
            gltf::image::Source::Uri { uri, .. } => Some(uri),
            gltf::image::Source::View { .. } => None,
        }));
    for uri in uris {
        if uri.starts_with("data:") {
            continue;
        }
        let relative = Path::new(uri);
        if uri.contains([':', '%', '\\'])
            || uri.is_empty()
            || !relative
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
        {
            return Err(format!(
                "{}: dependency must be an unencoded relative path within the package: {uri}",
                path.display()
            ));
        }
        let file = path
            .parent()
            .unwrap_or(root)
            .join(relative)
            .canonicalize()
            .map_err(|e| format!("{}: dependency {uri}: {e}", path.display()))?;
        if !file.starts_with(root) {
            return Err(format!(
                "{}: dependency leaves package: {uri}",
                path.display()
            ));
        }
    }
    Ok(())
}
