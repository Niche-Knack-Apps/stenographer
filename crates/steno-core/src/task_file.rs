use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::model::task::{Task, TaskSource, TaskVar};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TasksFile {
    #[serde(default)]
    pub tasks: HashMap<String, TaskDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub commands: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub vars: HashMap<String, TaskVar>,
}

pub fn load(path: &Path) -> Result<TasksFile> {
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            let file: TasksFile =
                toml::from_str(&contents).map_err(|e| crate::Error::ConfigParse {
                    path: path.to_path_buf(),
                    source: e,
                })?;
            Ok(file)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TasksFile::default()),
        Err(e) => Err(crate::Error::FileRead {
            path: path.to_path_buf(),
            source: e,
        }),
    }
}

pub fn save(path: &Path, file: &TasksFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| crate::Error::FileWrite {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    let contents = toml::to_string_pretty(file).map_err(|e| crate::Error::Other(e.to_string()))?;
    std::fs::write(path, contents).map_err(|e| crate::Error::FileWrite {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(())
}

pub fn to_tasks(file: &TasksFile) -> Vec<Task> {
    let mut tasks: Vec<Task> = file
        .tasks
        .iter()
        .map(|(name, def)| Task {
            name: name.clone(),
            description: def.description.clone(),
            commands: def.commands.clone(),
            vars: def.vars.clone(),
            source: TaskSource::User,
            confidence: 0.0,
            occurrence_count: 0,
            last_seen: None,
            db_id: None,
        })
        .collect();
    tasks.sort_by(|a, b| a.name.cmp(&b.name));
    tasks
}

pub fn add_task(file: &mut TasksFile, name: &str, def: TaskDef) {
    file.tasks.insert(name.to_string(), def);
}

pub fn remove_task(file: &mut TasksFile, name: &str) -> bool {
    file.tasks.remove(name).is_some()
}

pub fn rename_task(file: &mut TasksFile, old: &str, new: &str) -> bool {
    if let Some(def) = file.tasks.remove(old) {
        file.tasks.insert(new.to_string(), def);
        true
    } else {
        false
    }
}
