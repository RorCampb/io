#![forbid(unsafe_code)]
use crate::appearance::Appearance;
use crate::config::SceneConfig;
use io_assets::contract::{AssetPackage, BuiltinMesh, MeshSource};
use io_assets::{load_model, Model};
use io_types::AppearanceId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub struct ModelLibrary {
    pub config: SceneConfig,
    pub models: Vec<Model>,
    pub names: HashMap<String, u32>,
    pub appearances: Vec<Appearance>,
    pub appearance_names: HashMap<String, AppearanceId>,
}
impl ModelLibrary {
    pub fn global() -> Result<&'static Self, &'static str> {
        static LIBRARY: OnceLock<Result<ModelLibrary, String>> = OnceLock::new();
        LIBRARY
            .get_or_init(|| {
                let path = std::env::var_os("IO_SCENE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/street-kit/demo.json")
                    });
                Self::load(&path)
            })
            .as_ref()
            .map_err(String::as_str)
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let config = SceneConfig::read(path)?;
        Self::from_config(config, path.parent().unwrap_or(Path::new(".")))
    }
    pub fn from_config(mut config: SceneConfig, directory: &Path) -> Result<Self, String> {
        config.validate()?;
        let mut namespaces = std::collections::HashSet::new();
        for import in &config.packages {
            if !namespaces.insert(&import.namespace) {
                return Err(format!("duplicate package namespace: {}", import.namespace));
            }
            let path = directory.join(&import.file);
            let package = AssetPackage::read(&path)?;
            let (assets, appearances) = package.into_definitions(&path, &import.namespace)?;
            config.assets.extend(assets);
            config.appearances.extend(appearances);
        }
        let mut models = Vec::new();
        let mut names = HashMap::new();
        for asset in &config.assets {
            if asset.name.is_empty() || names.contains_key(&asset.name) {
                return Err(format!("duplicate asset: {}", asset.name));
            }
            let model = match asset.source()? {
                MeshSource::File(file) => load_model(&directory.join(file))?,
                MeshSource::Builtin(BuiltinMesh::Box) => Model::unit_box(),
            };
            asset.validate_model(&model)?;
            eprintln!(
                "Loaded {}: {} triangles, {} joints, {} clips",
                asset.name,
                model.indices().len() / 3,
                model.joint_count(),
                model.clips().len()
            );
            models.push(model);
            names.insert(
                asset.name.clone(),
                u32::try_from(models.len()).map_err(|_| "too many assets")?,
            );
        }
        // Legacy `asset` references resolve to one-mesh appearances in asset order.
        let mut appearances: Vec<_> = models
            .iter()
            .enumerate()
            .map(|(i, model)| Appearance::single(i as u32 + 1, model))
            .collect();
        let mut appearance_names = HashMap::new();
        for spec in &config.appearances {
            if spec.name.is_empty() || appearance_names.contains_key(&spec.name) {
                return Err(format!("empty or duplicate appearance: {}", spec.name));
            }
            appearances.push(
                Appearance::resolve(spec, &names, &models)
                    .map_err(|e| format!("appearance {}: {e}", spec.name))?,
            );
            appearance_names.insert(
                spec.name.clone(),
                AppearanceId(u32::try_from(appearances.len()).map_err(|_| "too many appearances")?),
            );
        }
        Ok(Self {
            config,
            models,
            names,
            appearances,
            appearance_names,
        })
    }
    pub fn mesh(&self, id: u32) -> Option<&Model> {
        id.checked_sub(1).and_then(|i| self.models.get(i as usize))
    }
    pub fn appearance(&self, id: AppearanceId) -> Option<&Appearance> {
        id.0.checked_sub(1)
            .and_then(|i| self.appearances.get(i as usize))
    }
}
