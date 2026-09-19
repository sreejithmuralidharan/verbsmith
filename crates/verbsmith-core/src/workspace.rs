use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    Environment, Error, HttpRequest, Result, WORKSPACE_SCHEMA_VERSION, WorkspaceManifest,
    parse_document,
};

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub manifest: WorkspaceManifest,
}

impl Workspace {
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let manifest_path = root.join("verbsmith.toml");
        let text = fs::read_to_string(&manifest_path).map_err(|source| Error::Read {
            path: manifest_path.clone(),
            source,
        })?;
        let manifest: WorkspaceManifest = toml::from_str(&text).map_err(|source| Error::Parse {
            path: manifest_path,
            message: source.to_string(),
        })?;
        if manifest.schema > WORKSPACE_SCHEMA_VERSION {
            return Err(Error::UnsupportedSchema {
                found: manifest.schema,
                supported: WORKSPACE_SCHEMA_VERSION,
            });
        }
        Ok(Self { root, manifest })
    }

    pub fn discover(start: impl AsRef<Path>) -> Result<Self> {
        let root = discover_workspace(start.as_ref())?;
        Self::load(root)
    }

    pub fn requests(&self) -> Result<Vec<HttpRequest>> {
        let pattern_path = self.root.join(&self.manifest.request_glob);
        let pattern = pattern_path.to_string_lossy();
        let entries = glob::glob(&pattern).map_err(|source| Error::Parse {
            path: self.root.join("verbsmith.toml"),
            message: format!("invalid request_glob: {source}"),
        })?;
        let mut paths = entries
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|source| Error::Parse {
                path: self.root.join("verbsmith.toml"),
                message: format!("failed to expand request_glob: {source}"),
            })?
            .into_iter()
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        paths.sort();

        let mut requests = Vec::new();
        for path in paths {
            let text = fs::read_to_string(&path).map_err(|source| Error::Read {
                path: path.clone(),
                source,
            })?;
            requests.extend(parse_document(&path, &text)?);
        }
        Ok(requests)
    }

    pub fn environment(&self, name: Option<&str>) -> Result<Environment> {
        let Some(name) = name.or(self.manifest.default_environment.as_deref()) else {
            return Ok(Environment::default());
        };
        let path = self.root.join("environments").join(format!("{name}.toml"));
        let text = fs::read_to_string(&path).map_err(|source| Error::Read {
            path: path.clone(),
            source,
        })?;
        toml::from_str(&text).map_err(|source| Error::Parse {
            path,
            message: source.to_string(),
        })
    }

    pub fn variables(&self, environment: &Environment) -> BTreeMap<String, String> {
        let mut variables = self.manifest.variables.clone();
        variables.extend(environment.values.clone());
        variables.extend(environment.secrets.clone());
        variables
    }
}

pub fn discover_workspace(start: &Path) -> Result<PathBuf> {
    let mut current = if start.is_file() {
        start.parent().unwrap_or(start).to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if current.join("verbsmith.toml").is_file() {
            return Ok(current);
        }
        if !current.pop() {
            return Err(Error::WorkspaceNotFound(start.to_path_buf()));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn discovers_parent_workspace() {
        let directory = tempdir().unwrap();
        fs::write(
            directory.path().join("verbsmith.toml"),
            "schema = 1\nname = \"test\"\n",
        )
        .unwrap();
        let nested = directory.path().join("one/two");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(discover_workspace(&nested).unwrap(), directory.path());
    }
}
