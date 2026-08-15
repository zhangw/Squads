use directories::ProjectDirs;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::{
    env, fs, io::Write, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}
};

pub fn truncate_name(name: String, max_length: usize) -> String {
    if name.len() > max_length {
        let cutoff = max_length.saturating_sub(3);
        let mut end = name.len();

        for (idx, _) in name.char_indices() {
            if idx > cutoff {
                end = idx;
                break;
            }
        }

        let mut truncated = name[..end].to_string();
        truncated.push_str("...");
        truncated
    } else {
        name.to_string()
    }
}

pub fn save_to_cache<T>(filename: &str, content: &T)
where
    T: Serialize,
{
    let project_dirs = ProjectDirs::from("", "ianterzo", "squads");
    let mut cache_dir = project_dirs.unwrap().cache_dir().to_path_buf();
    fs::create_dir_all(cache_dir.clone()).expect("Failed to create cache directory");

    cache_dir.push(filename);

    let json = serde_json::to_string_pretty(content).expect("Failed to serialize content");
    let mut file = fs::File::create(cache_dir).unwrap();
    file.write_all(json.as_bytes()).unwrap();
}

pub fn get_cache<T: DeserializeOwned>(filename: &str) -> Option<T> {
    let project_dirs = ProjectDirs::from("", "ianterzo", "squads");

    let mut cache_dir = project_dirs.unwrap().cache_dir().to_path_buf();
    cache_dir.push(filename);

    if cache_dir.exists() {
        let file_content = fs::read_to_string(cache_dir).ok()?;
        serde_json::from_str(&file_content).ok()
    } else {
        None
    }
}

/// Persist secrets (access_tokens.json) with create-new 0600 semantics and an
/// atomic rename: the secret never exists with permissive permissions and a
/// pre-existing symlink at the target is replaced, not followed.
pub fn save_private_to_cache<T>(filename: &str, content: &T)
where
    T: Serialize,
{
    let project_dirs = ProjectDirs::from("", "ianterzo", "squads");
    let cache_dir = project_dirs.unwrap().cache_dir().to_path_buf();
    fs::create_dir_all(&cache_dir).expect("Failed to create cache directory");

    let json = serde_json::to_string_pretty(content).expect("Failed to serialize content");
    write_atomic_private(&cache_dir.join(filename), json.as_bytes());
}

fn write_atomic_private(target: &Path, data: &[u8]) {
    let dir = target.parent().unwrap_or(Path::new("."));
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "secret".to_string());
    let tmp = dir.join(format!(".{}.{}.tmp", name, std::process::id()));
    let res = (|| -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut opts = fs::OpenOptions::new();
            opts.write(true).create_new(true).mode(0o600);
            let mut f = opts.open(&tmp)?;
            f.write_all(data)?;
            f.sync_all()?;
            fs::rename(&tmp, target)?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
            f.write_all(data)?;
            f.sync_all()?;
            fs::rename(&tmp, target)?;
            Ok(())
        }
    })();
    if let Err(e) = res {
        let _ = fs::remove_file(&tmp);
        eprintln!("Failed to persist credential cache {}: {}", target.display(), e);
    }
}

pub fn delete_cache(filename: &str) {
    if let Some(project_dirs) = ProjectDirs::from("", "ianterzo", "squads") {
        let cache_path = project_dirs.cache_dir().join(filename);
        let _ = fs::remove_file(cache_path);
    }
}

pub fn get_epoch_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn get_epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

pub fn get_image_dir() -> PathBuf {
	PathBuf::from(env::var("SQUADS_IMAGE_DIR").unwrap_or("images".to_string()))
}

pub fn get_resource_dir() -> PathBuf {
	PathBuf::from(env::var("SQUADS_RESOURCE_DIR").unwrap_or("resources".to_string()))
}
