use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    iter,
    path::{Component, Path, PathBuf},
};

use age::{Decryptor, Encryptor, scrypt};
use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use verbsmith_core::{ExecutionOptions, Header, HttpRequest, Workspace, execute};

use crate::{SyncCommand, vault_entry};

#[derive(Debug, Serialize, Deserialize)]
struct Snapshot {
    schema: u32,
    files: Vec<SnapshotFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SnapshotFile {
    path: String,
    contents: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SyncState {
    last_revision: Option<String>,
    last_snapshot: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Revision {
    id: String,
    parent_id: Option<String>,
    sha256: String,
    ciphertext: String,
}

pub fn run(command: SyncCommand, workspace: &Workspace) -> Result<u8> {
    let config = workspace
        .manifest
        .sync
        .as_ref()
        .ok_or_else(|| anyhow!("add a [sync] section to verbsmith.toml before using sync"))?;
    let token = vault_entry(workspace, &config.token_secret)?
        .get_password()
        .with_context(|| {
            format!(
                "reading sync token `{}` from the credential manager",
                config.token_secret
            )
        })?;
    let encryption_key = vault_entry(workspace, &config.encryption_secret)?
        .get_password()
        .with_context(|| {
            format!(
                "reading sync key `{}` from the credential manager",
                config.encryption_secret
            )
        })?;
    let mut state = load_state(workspace)?;

    match command {
        SyncCommand::Status => {
            let remote = get_head(workspace, &token)?;
            println!(
                "local:  {}",
                state.last_revision.as_deref().unwrap_or("none")
            );
            if let Some(revision) = &remote {
                println!("remote: {}", revision.id);
                println!(
                    "parent: {}",
                    revision.parent_id.as_deref().unwrap_or("none")
                );
                println!("sha256: {}", revision.sha256);
            } else {
                println!("remote: none");
            }
            Ok(
                if remote.as_ref().map(|revision| &revision.id) == state.last_revision.as_ref() {
                    0
                } else {
                    1
                },
            )
        }
        SyncCommand::Push => {
            let remote = get_head(workspace, &token)?;
            let remote_id = remote.as_ref().map(|revision| revision.id.as_str());
            if remote_id != state.last_revision.as_deref() {
                bail!("remote workspace changed; pull and resolve it before pushing");
            }
            let snapshot = create_snapshot(workspace)?;
            let snapshot_json = serde_json::to_vec(&snapshot)?;
            let snapshot_hash = digest(&snapshot_json);
            let ciphertext = encrypt(&snapshot_json, encryption_key)?;
            let response = request(
                "PUT",
                &format!(
                    "{}/api/v1/workspaces/{}/revisions",
                    config.endpoint.trim_end_matches('/'),
                    config.workspace_id
                ),
                &token,
                Some(serde_json::to_string(&serde_json::json!({
                    "base_revision": state.last_revision,
                    "ciphertext": STANDARD.encode(ciphertext),
                }))?),
            )?;
            if response.status != 201 {
                bail!(
                    "sync server returned {}: {}",
                    response.status,
                    String::from_utf8_lossy(&response.body)
                );
            }
            let revision: Revision = serde_json::from_slice(&response.body)?;
            state.last_revision = Some(revision.id.clone());
            state.last_snapshot = Some(snapshot_hash);
            save_state(workspace, &state)?;
            println!("Pushed revision {}", revision.id);
            Ok(0)
        }
        SyncCommand::Pull { force } => {
            let revision = get_head(workspace, &token)?
                .ok_or_else(|| anyhow!("remote workspace has no revisions"))?;
            let encrypted = STANDARD
                .decode(&revision.ciphertext)
                .context("remote ciphertext is not valid base64")?;
            let plaintext = decrypt(&encrypted, encryption_key)?;
            let snapshot: Snapshot = serde_json::from_slice(&plaintext)
                .context("decrypted revision is not a workspace snapshot")?;
            validate_snapshot(&snapshot)?;
            let remote_snapshot_hash = digest(&serde_json::to_vec(&snapshot)?);
            let current_snapshot_hash = digest(&serde_json::to_vec(&create_snapshot(workspace)?)?);
            if !should_apply_remote(
                state.last_snapshot.as_deref(),
                &current_snapshot_hash,
                &remote_snapshot_hash,
                force,
            )? {
                state.last_revision = Some(revision.id.clone());
                state.last_snapshot = Some(remote_snapshot_hash);
                save_state(workspace, &state)?;
                println!("Already up to date at {}", revision.id);
                return Ok(0);
            }
            backup_and_apply(workspace, &snapshot)?;
            state.last_revision = Some(revision.id.clone());
            state.last_snapshot = Some(remote_snapshot_hash);
            save_state(workspace, &state)?;
            println!("Pulled revision {}", revision.id);
            Ok(0)
        }
    }
}

fn should_apply_remote(
    last_snapshot: Option<&str>,
    current_snapshot: &str,
    remote_snapshot: &str,
    force: bool,
) -> Result<bool> {
    if current_snapshot == remote_snapshot {
        return Ok(false);
    }
    let local_changed = last_snapshot.is_none_or(|previous| current_snapshot != previous);
    if local_changed && !force {
        bail!(
            "local workspace changed; commit it or repeat with --force to create a backup and replace files"
        );
    }
    Ok(true)
}

fn create_snapshot(workspace: &Workspace) -> Result<Snapshot> {
    let mut paths = vec![workspace.root.join("verbsmith.toml")];
    for pattern in [
        workspace.manifest.request_glob.as_str(),
        "environments/*.toml",
    ] {
        let absolute = workspace.root.join(pattern).to_string_lossy().into_owned();
        for entry in glob::glob(&absolute)? {
            let path = entry?;
            if path.is_file() {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths.dedup();
    let files = paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&workspace.root)
                .context("snapshot path escaped workspace")?;
            Ok(SnapshotFile {
                path: relative.to_string_lossy().replace('\\', "/"),
                contents: fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Snapshot { schema: 1, files })
}

fn get_head(workspace: &Workspace, token: &str) -> Result<Option<Revision>> {
    let config = workspace
        .manifest
        .sync
        .as_ref()
        .expect("validated sync config");
    let response = request(
        "GET",
        &format!(
            "{}/api/v1/workspaces/{}/head",
            config.endpoint.trim_end_matches('/'),
            config.workspace_id
        ),
        token,
        None,
    )?;
    match response.status {
        200 => Ok(Some(serde_json::from_slice(&response.body)?)),
        404 => Ok(None),
        status => bail!(
            "sync server returned {status}: {}",
            String::from_utf8_lossy(&response.body)
        ),
    }
}

fn request(
    method: &str,
    url: &str,
    token: &str,
    body: Option<String>,
) -> Result<verbsmith_core::HttpResponse> {
    let mut headers = vec![Header {
        name: "Authorization".into(),
        value: format!("Bearer {token}"),
    }];
    if body.is_some() {
        headers.push(Header {
            name: "Content-Type".into(),
            value: "application/json".into(),
        });
    }
    execute(
        &HttpRequest {
            name: "sync".into(),
            method: method.into(),
            url: url.into(),
            headers,
            body: body.unwrap_or_default(),
            source: PathBuf::from("<sync>"),
            line: 1,
            timeout: None,
            tags: Vec::new(),
            depends_on: Vec::new(),
            assertions: Vec::new(),
            captures: Vec::new(),
            disabled: false,
        },
        &ExecutionOptions::default(),
    )
    .map_err(Into::into)
}

fn encrypt(plaintext: &[u8], passphrase: String) -> Result<Vec<u8>> {
    let passphrase = age::secrecy::SecretString::from(passphrase);
    let encryptor = Encryptor::with_user_passphrase(passphrase);
    let mut encrypted = Vec::new();
    let mut writer = encryptor.wrap_output(&mut encrypted)?;
    writer.write_all(plaintext)?;
    writer.finish()?;
    Ok(encrypted)
}

fn decrypt(ciphertext: &[u8], passphrase: String) -> Result<Vec<u8>> {
    let passphrase = age::secrecy::SecretString::from(passphrase);
    let decryptor = Decryptor::new(ciphertext)?;
    let identity = scrypt::Identity::new(passphrase);
    let mut reader = decryptor.decrypt(iter::once(&identity as _))?;
    let mut plaintext = Vec::new();
    reader.read_to_end(&mut plaintext)?;
    Ok(plaintext)
}

fn validate_snapshot(snapshot: &Snapshot) -> Result<()> {
    if snapshot.schema != 1 {
        bail!("unsupported remote snapshot schema {}", snapshot.schema);
    }
    let mut paths = HashSet::new();
    for file in &snapshot.files {
        let normalized = file.path.replace('\\', "/");
        let path = Path::new(&normalized);
        if file.path.is_empty()
            || path.is_absolute()
            || normalized
                .split('/')
                .next()
                .is_some_and(|part| part.contains(':'))
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            bail!("remote snapshot contains unsafe path `{}`", file.path);
        }
        let portable_path = normalized.to_lowercase();
        if !paths.insert(portable_path) {
            bail!(
                "remote snapshot contains duplicate or case-conflicting path `{}`",
                file.path
            );
        }
    }
    Ok(())
}

fn backup_and_apply(workspace: &Workspace, snapshot: &Snapshot) -> Result<()> {
    let backup = workspace
        .root
        .join(".verbsmith/backups")
        .join(time::OffsetDateTime::now_utc().unix_timestamp().to_string());
    for file in &snapshot.files {
        let target = workspace.root.join(&file.path);
        reject_symlink_ancestors(&workspace.root, Path::new(&file.path))?;
        if target.exists() {
            let metadata = fs::symlink_metadata(&target)?;
            if !metadata.file_type().is_file() {
                bail!(
                    "refusing to replace non-file snapshot target `{}`",
                    file.path
                );
            }
            let backup_target = backup.join(&file.path);
            if let Some(parent) = backup_target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&target, backup_target)?;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = target.with_extension("verbsmith-sync-tmp");
        fs::write(&temporary, &file.contents)?;
        fs::rename(temporary, target)?;
    }
    Ok(())
}

fn reject_symlink_ancestors(root: &Path, relative: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            bail!("snapshot path is not normalized: {}", relative.display());
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "refusing to write snapshot through symbolic link `{}`",
                    current.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn state_path(workspace: &Workspace) -> PathBuf {
    workspace.root.join(".verbsmith/sync.toml")
}

fn load_state(workspace: &Workspace) -> Result<SyncState> {
    let path = state_path(workspace);
    if !path.exists() {
        return Ok(SyncState::default());
    }
    toml::from_str(&fs::read_to_string(path)?).map_err(Into::into)
}

fn save_state(workspace: &Workspace, state: &SyncState) -> Result<()> {
    let path = state_path(workspace);
    fs::create_dir_all(path.parent().expect("sync state has parent"))?;
    fs::write(path, toml::to_string_pretty(state)?)?;
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_snapshot_round_trips() {
        let plaintext = br#"{"schema":1,"files":[]}"#;
        let encrypted = encrypt(plaintext, "correct horse battery staple".into()).unwrap();
        assert_ne!(encrypted, plaintext);
        assert_eq!(
            decrypt(&encrypted, "correct horse battery staple".into()).unwrap(),
            plaintext
        );
    }

    #[test]
    fn unsafe_snapshot_paths_are_rejected() {
        for path in ["../secret", r"..\secret", "/etc/passwd", "C:/secret"] {
            let snapshot = Snapshot {
                schema: 1,
                files: vec![SnapshotFile {
                    path: path.into(),
                    contents: String::new(),
                }],
            };
            assert!(validate_snapshot(&snapshot).is_err(), "accepted {path}");
        }
    }

    #[test]
    fn duplicate_and_case_conflicting_paths_are_rejected() {
        let snapshot = Snapshot {
            schema: 1,
            files: vec![
                SnapshotFile {
                    path: "requests/health.http".into(),
                    contents: String::new(),
                },
                SnapshotFile {
                    path: "Requests/HEALTH.http".into(),
                    contents: String::new(),
                },
            ],
        };
        assert!(validate_snapshot(&snapshot).is_err());
    }

    #[test]
    fn pull_requires_force_for_changed_or_untracked_local_files() {
        assert!(should_apply_remote(Some("old"), "changed", "remote", false).is_err());
        assert!(should_apply_remote(None, "local", "remote", false).is_err());
        assert!(should_apply_remote(Some("old"), "changed", "remote", true).unwrap());
        assert!(!should_apply_remote(Some("old"), "same", "same", false).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_ancestors_are_rejected() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), directory.path().join("requests")).unwrap();
        let error = reject_symlink_ancestors(directory.path(), Path::new("requests/escaped.http"))
            .unwrap_err();
        assert!(error.to_string().contains("symbolic link"));
    }
}
