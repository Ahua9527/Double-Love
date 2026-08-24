use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;
use uuid::Uuid;

use crate::storage::{ProjectStore, StorageError};

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("project path must be a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("project path is outside the selected workspace: {0}")]
    InvalidPath(PathBuf),
    #[error("project storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("filesystem error: {0}")]
    Filesystem(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProjectSummary {
    pub project_id: String,
    pub root: String,
    pub database: String,
    pub revision: u64,
}

fn project_root(path: &Path) -> Result<PathBuf, ProjectError> {
    if path.as_os_str().is_empty() {
        return Err(ProjectError::InvalidPath(path.to_path_buf()));
    }

    let root = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if root.exists() && !root.is_dir() {
        return Err(ProjectError::NotDirectory(root));
    }
    Ok(root)
}

fn prepare_project(root: &Path, created: bool) -> Result<ProjectSummary, ProjectError> {
    fs::create_dir_all(root)?;
    let doublelove = root.join(".doublelove");
    fs::create_dir_all(doublelove.join("cache"))?;
    fs::create_dir_all(doublelove.join("logs"))?;
    fs::create_dir_all(doublelove.join("exports"))?;

    let database = doublelove.join("project.sqlite");
    let store = ProjectStore::open(&database)?;
    let project_id = store
        .project_id()?
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    store.set_project_id(&project_id)?;
    if created {
        store.ensure_project_created_at()?;
    }

    let manifest_path = doublelove.join("manifest.json");
    if !manifest_path.exists() {
        let manifest = serde_json::json!({
            "schemaVersion": 1,
            "projectId": project_id.clone(),
            "rawInputs": "read-only references",
        });
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).expect("manifest serializes"),
        )?;
    }

    Ok(ProjectSummary {
        project_id,
        root: root.to_string_lossy().into_owned(),
        database: database.to_string_lossy().into_owned(),
        revision: store.revision()?,
    })
}

pub fn create_project(path: &Path) -> Result<ProjectSummary, ProjectError> {
    prepare_project(&project_root(path)?, true)
}

pub fn open_project(path: &Path) -> Result<ProjectSummary, ProjectError> {
    let root = project_root(path)?;
    let database = root.join(".doublelove/project.sqlite");
    if !database.exists() {
        return Err(ProjectError::InvalidPath(root));
    }
    prepare_project(&root, false)
}
