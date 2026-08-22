//! Read-only model catalogue state and model-root migration for the Electron slice.

use std::{fs, path::Path, sync::Mutex};

use double_love_engine::{
    ModelDescriptorWithInstallation, ModelError, ModelInstallState, ModelManager,
};

#[derive(Default)]
pub struct ModelState {
    manager: Mutex<Option<ModelManager>>,
}

impl std::fmt::Debug for ModelState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelState").finish_non_exhaustive()
    }
}

impl ModelState {
    fn with_manager<T>(
        &self,
        root: &Path,
        operation: impl FnOnce(&mut ModelManager) -> Result<T, ModelError>,
    ) -> Result<T, ModelError> {
        let mut slot = self.manager.lock().map_err(|_| {
            ModelError::InvalidState("model manager lock is unavailable".to_string())
        })?;
        if slot.as_ref().is_none_or(|manager| manager.root() != root) {
            *slot = Some(ModelManager::with_builtin_catalog(root)?);
        }
        let manager = slot.as_mut().expect("model manager initialized");
        mark_bundled_installed(manager)?;
        operation(manager)
    }

    pub fn snapshot(&self, root: &Path) -> Result<Vec<ModelDescriptorWithInstallation>, String> {
        self.with_manager(root, |manager| Ok(manager.snapshot()))
            .map_err(|error| error.to_string())
    }

    pub fn migrate_root(&self, old_root: &Path, new_root: &Path) -> Result<(), String> {
        if old_root == new_root {
            return Ok(());
        }
        if !new_root.is_absolute() {
            return Err("新的模型目录必须是绝对路径。".to_string());
        }

        // Build both managers off to the side. The live manager remains rooted at the old
        // directory until every installed model has been copied and verified successfully.
        let old =
            ModelManager::with_builtin_catalog(old_root).map_err(|error| error.to_string())?;
        let mut next =
            ModelManager::with_builtin_catalog(new_root).map_err(|error| error.to_string())?;
        for snapshot in old.snapshot() {
            if snapshot.installation.state != ModelInstallState::Installed
                || snapshot.descriptor.bundled
            {
                continue;
            }
            let source = old
                .installation_dir(&snapshot.descriptor.id)
                .map_err(|error| error.to_string())?;
            let destination = next
                .installation_dir(&snapshot.descriptor.id)
                .map_err(|error| error.to_string())?;
            copy_directory(&source, &destination)?;
            next.verify_directory(&snapshot.descriptor.id, &destination)
                .map_err(|error| error.to_string())?;
            next.transition(&snapshot.descriptor.id, ModelInstallState::Installed)
                .map_err(|error| error.to_string())?;
        }
        mark_bundled_installed(&mut next).map_err(|error| error.to_string())?;

        let mut slot = self
            .manager
            .lock()
            .map_err(|_| "model manager lock is unavailable".to_string())?;
        *slot = Some(next);
        Ok(())
    }
}

fn mark_bundled_installed(manager: &mut ModelManager) -> Result<(), ModelError> {
    let bundled = manager
        .catalog()
        .iter()
        .filter(|descriptor| descriptor.bundled)
        .map(|descriptor| descriptor.id.clone())
        .collect::<Vec<_>>();
    for model_id in bundled {
        if manager.installation(&model_id)?.state != ModelInstallState::Installed {
            manager.transition(&model_id, ModelInstallState::Installed)?;
        }
    }
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Err("旧模型目录不可用。".to_string());
    }
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            return Err("模型目录不能包含符号链接。".to_string());
        }
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), target).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}
