use std::path::PathBuf;
use dotenvy::{EnvLoader, EnvMap, EnvSequence};

pub fn load_env_vars(project_root: &PathBuf) -> Result<EnvMap, anyhow::Error> {

    unsafe {

        let env_map = match EnvLoader::with_path(project_root.join(".env"))
            .sequence(EnvSequence::InputThenEnv)
            .load_and_modify()
        {
            Ok(env_map) => env_map,
            Err(dotenvy::Error::Io(_, _)) => EnvLoader::new()
                .sequence(EnvSequence::EnvOnly)
                .load_and_modify()?,
            err => err?,
        };
        Ok(env_map)

    }

}