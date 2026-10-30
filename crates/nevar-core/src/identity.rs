use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use iroh::SecretKey;

/// Loads this device's secret key from `path`, creating a new one on first run.
///
/// The public half of this key is the user's permanent ID (their "friend code").
pub fn load_or_create(path: &Path) -> Result<SecretKey> {
    if path.exists() {
        let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let Ok(bytes) = <[u8; 32]>::try_from(bytes.as_slice()) else {
            bail!("identity file {} is corrupt", path.display());
        };
        return Ok(SecretKey::from_bytes(&bytes));
    }

    let key = SecretKey::generate();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, key.to_bytes()).with_context(|| format!("writing {}", path.display()))?;
    Ok(key)
}
