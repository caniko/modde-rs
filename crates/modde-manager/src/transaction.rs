use super::*;
use files::{Anchor, Image};

struct Operation {
    path: PathBuf,
    before: Image,
    after: Image,
    seed: bool,
}

pub struct Prepared {
    root: Anchor,
    operations: Vec<Operation>,
    sources: Vec<(PathBuf, Image, bool)>,
}

fn source(path: &Path, skip_git: bool) -> Result<Image> {
    let parent = match Anchor::open(path.parent().context("source needs a parent")?) {
        Ok(parent) => parent,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            bail!("blocked: missing source {}", path.display());
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("invalid source parent: {}", path.display()));
        }
    };
    let image = parent.read(
        Path::new(path.file_name().context("source needs a name")?),
        skip_git,
    )?;
    if image == Image::Missing {
        bail!("blocked: missing source {}", path.display());
    }
    Ok(image)
}

pub(crate) fn overlap(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

fn register(paths: &mut Vec<PathBuf>, path: &Path) -> Result<()> {
    files::relative(path)?;
    if path.components().any(|c| {
        c.as_os_str()
            .to_string_lossy()
            .to_ascii_lowercase()
            .starts_with("blizzard_")
    }) {
        bail!("stock Blizzard content is protected");
    }
    if paths.iter().any(|existing| overlap(existing, path)) {
        bail!("colliding destination: {}", path.display());
    }
    paths.push(path.to_owned());
    Ok(())
}

impl Prepared {
    fn add(&mut self, path: PathBuf, after: Image, seed: bool) -> Result<()> {
        let before = self.root.read(&path, false)?;
        match (&before, &after) {
            (Image::Missing, _)
            | (Image::File(_), Image::File(_))
            | (Image::Directory(_), Image::Directory(_)) => {}
            _ => bail!("destination type conflict: {}", path.display()),
        }
        if before != after && (!seed || before == Image::Missing) {
            self.operations.push(Operation {
                path,
                before,
                after,
                seed,
            });
        }
        Ok(())
    }

    fn seed(&mut self, path: &Path, image: &Image) -> Result<()> {
        match image {
            Image::Directory(entries) => {
                match self.root.read(path, false)? {
                    Image::Missing | Image::Directory(_) => {}
                    _ => bail!("seed tree type conflict: {}", path.display()),
                }
                for (name, child) in entries {
                    self.seed(&path.join(name), child)?;
                }
            }
            Image::File(_) => self.add(path.to_owned(), image.clone(), true)?,
            Image::Missing => bail!("missing seed content"),
        }
        Ok(())
    }

    pub fn plan(&self, name: &str) -> Plan {
        Plan {
            changes: self
                .operations
                .iter()
                .map(|op| Change {
                    resource: format!("{name}/{}", op.path.display()),
                    kind: if op.before == Image::Missing {
                        ChangeKind::Create
                    } else {
                        ChangeKind::Update
                    },
                    summary: if op.seed {
                        "seed absent file"
                    } else {
                        "reconcile content"
                    }
                    .into(),
                })
                .collect(),
        }
    }

    fn revalidate(&self) -> Result<()> {
        self.root.revalidate()?;
        for (path, expected, skip_git) in &self.sources {
            if &source(path, *skip_git)? != expected {
                bail!("source changed after preparation: {}", path.display());
            }
        }
        for op in &self.operations {
            let current = self.root.read(&op.path, false)?;
            if current != op.before && !(op.seed && matches!(current, Image::File(_))) {
                bail!(
                    "destination changed after preparation: {}",
                    op.path.display()
                );
            }
        }
        Ok(())
    }

    pub fn apply(&self, instance: &Instance) -> Result<()> {
        self.apply_inner(instance, None)
    }

    fn apply_inner(&self, instance: &Instance, fail_after: Option<(usize, bool)>) -> Result<()> {
        let _lease = self.root.lock()?;
        self.revalidate()?;
        assert_stopped(instance)?;
        if self.operations.is_empty() {
            return Ok(());
        }
        // ponytail: payloads are memory-resident; stream to private staging if addon sizes outgrow RAM.
        let staging = tempfile::Builder::new()
            .prefix(".modde-transaction-")
            .tempdir_in(self.root.path())?;
        for (index, op) in self.operations.iter().enumerate() {
            let path = staging.path().join(format!("new-{index}"));
            files::write_image(&path, &op.after)?;
            if files::read_image(&path, false)? != op.after {
                bail!("staged payload verification failed");
            }
        }
        self.revalidate()?;
        assert_stopped(instance)?;
        let staging = staging.keep();
        let mut committed: Vec<usize> = Vec::new();
        let mut created = Vec::new();
        let mut pending_backup = None;
        let journal = |status: &str,
                       committed: &[usize],
                       created: &[PathBuf],
                       pending_backup: Option<usize>|
         -> Result<()> {
            atomic_write_0600(&staging.join("journal.json"), &serde_json::to_vec_pretty(&serde_json::json!({
                "version": 1, "status": status,
                "operations": self.operations.iter().enumerate().map(|(i, op)| serde_json::json!({
                    "destination": op.path, "backup": format!("old-{i}"), "payload": format!("new-{i}"),
                    "created": op.before == Image::Missing, "seed": op.seed
                })).collect::<Vec<_>>(),
                "committed": committed, "createdDirectories": created, "pendingBackup": pending_backup,
            }))?).with_context(|| format!("journal write failed; recovery at {}", instance.root.join(staging.file_name().unwrap_or_default()).display()))?;
            fs::File::open(&staging)?.sync_all()?;
            Ok(())
        };
        journal("prepared", &committed, &created, pending_backup)?;
        let result = (|| -> Result<()> {
            for (index, op) in self.operations.iter().enumerate() {
                if fail_after.is_some_and(|(after, _)| after == committed.len()) {
                    if fail_after.is_some_and(|(_, damage)| damage) {
                        if let Some(&first) = committed.first() {
                            let (_parent, path) = self.root.target(
                                &self.operations[first].path,
                                false,
                                &mut Vec::new(),
                            )?;
                            fs::write(path, "concurrent user data")?;
                        }
                    }
                    bail!("injected commit failure");
                }
                self.root.revalidate()?;
                let current = self.root.read(&op.path, false)?;
                if op.seed && matches!(current, Image::File(_)) {
                    continue;
                }
                if current != op.before {
                    bail!("destination changed before commit");
                }
                let (parent, target) = self.root.target(&op.path, true, &mut created)?;
                journal("committing", &committed, &created, pending_backup)?;
                self.root.revalidate_parent(&op.path, &parent)?;
                let payload = staging.join(format!("new-{index}"));
                let backup = staging.join(format!("old-{index}"));
                if op.seed {
                    match fs::hard_link(&payload, &target) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                            if !matches!(self.root.read(&op.path, false)?, Image::File(_)) {
                                bail!("seed destination type changed");
                            }
                            continue;
                        }
                        Err(e) => return Err(e.into()),
                    }
                } else {
                    if op.before != Image::Missing {
                        files::rename_noreplace(&target, &backup)?;
                        pending_backup = Some(index);
                        journal("committing", &committed, &created, pending_backup)?;
                    }
                    if let Err(error) = files::rename_noreplace(&payload, &target) {
                        if op.before != Image::Missing {
                            files::rename_noreplace(&backup, &target).with_context(|| {
                                format!("rollback failed; backup {}", backup.display())
                            })?;
                            pending_backup = None;
                        }
                        return Err(error.into());
                    }
                }
                committed.push(index);
                pending_backup = None;
                journal("committing", &committed, &created, pending_backup)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            let rollback = (|| -> Result<()> {
                if let Some(index) = pending_backup {
                    let (_parent, target) =
                        self.root
                            .target(&self.operations[index].path, false, &mut Vec::new())?;
                    files::rename_noreplace(&staging.join(format!("old-{index}")), &target)?;
                    pending_backup = None;
                }
                for &index in committed.iter().rev() {
                    let op = &self.operations[index];
                    if self.root.read(&op.path, false)? != op.after {
                        bail!("rollback refuses changed destination {}", op.path.display());
                    }
                    let (parent, target) = self.root.target(&op.path, false, &mut Vec::new())?;
                    self.root.revalidate_parent(&op.path, &parent)?;
                    files::rename_noreplace(
                        &target,
                        &staging.join(format!("rolled-back-{index}")),
                    )?;
                    if op.before != Image::Missing {
                        files::rename_noreplace(&staging.join(format!("old-{index}")), &target)?;
                    }
                }
                for path in created.iter().rev() {
                    let (_parent, target) = self.root.target(path, false, &mut Vec::new())?;
                    fs::remove_dir(target)?;
                }
                Ok(())
            })();
            journal(
                if rollback.is_ok() {
                    "rolled-back"
                } else {
                    "rollback-failed"
                },
                &committed,
                &created,
                pending_backup,
            )?;
            bail!(
                "apply failed: {error:#}; rollback: {rollback:?}; recovery retained at {}",
                instance
                    .root
                    .join(staging.file_name().context("staging name")?)
                    .display()
            );
        }
        journal("committed", &committed, &created, pending_backup)?;
        println!(
            "recovery journal: {}",
            instance
                .root
                .join(staging.file_name().context("staging name")?)
                .join("journal.json")
                .display()
        );
        Ok(())
    }
}

pub fn prepare(name: &str, instance: &Instance) -> Result<Prepared> {
    validate_instance(name, instance)?;
    let mut prepared = Prepared {
        root: Anchor::open(&instance.root)?,
        operations: Vec::new(),
        sources: Vec::new(),
    };
    let mut destinations = Vec::new();
    let lock = if !instance.addons.is_empty() {
        let path = instance
            .lock_file
            .as_ref()
            .context("blocked: addon lock_file is required")?;
        let image = source(path, false)?;
        let Image::File(bytes) = &image else {
            bail!("lock_file must be regular");
        };
        let lock: LockFile = serde_json::from_slice(bytes)?;
        prepared.sources.push((path.clone(), image, false));
        lock
    } else {
        LockFile::default()
    };
    for addon in &instance.addons {
        let locked = lock
            .repositories
            .get(&addon.id)
            .context("blocked: missing addon lock; import or update explicitly")?;
        if locked.branch != addon.branch
            || addon
                .revision
                .as_ref()
                .is_some_and(|revision| revision != &locked.revision)
        {
            bail!("stale addon lock: {}", addon.id);
        }
        let checkout = checkout_path(instance, &addon.id);
        if locked.imported_from.is_none() {
            verify_checkout_revision(&checkout, &locked.revision)?;
        }
        let image = source(&checkout, true)?;
        if let Some(digest) = &locked.content_sha256 {
            if digest != &digest_image(&image)? {
                bail!("locked content changed: {}", addon.id);
            }
        } else {
            bail!(
                "blocked: legacy addon lock lacks content verification; explicitly import or update"
            );
        }
        if locked.repository.as_deref() != Some(repository(addon).as_str()) {
            bail!("locked repository differs from declaration");
        }
        for directory in &addon.directories {
            if directory
                .target
                .to_ascii_lowercase()
                .starts_with("blizzard_")
            {
                bail!("stock Blizzard addon is protected");
            }
            let destination = Path::new("Interface/AddOns").join(&directory.target);
            register(&mut destinations, &destination)?;
            let mut content = &image;
            if directory.source != "." {
                files::relative(Path::new(&directory.source))?;
                for component in Path::new(&directory.source).components() {
                    let Image::Directory(entries) = content else {
                        bail!("addon source is not a directory");
                    };
                    content = entries
                        .get(
                            component
                                .as_os_str()
                                .to_str()
                                .context("invalid source name")?,
                        )
                        .context("missing addon subdirectory")?;
                }
            }
            let Image::Directory(entries) = content else {
                bail!("addon source is not a directory");
            };
            let Some(Image::File(toc)) = entries.get(&format!("{}.toc", directory.target)) else {
                bail!("missing applicable TOC");
            };
            let expected = instance
                .interface
                .unwrap_or(if instance.client == "wow-classic" {
                    11200
                } else {
                    30300
                });
            let values: Vec<_> = std::str::from_utf8(toc)?
                .lines()
                .filter_map(|line| line.strip_prefix("## Interface:"))
                .map(|value| value.trim().parse::<u32>())
                .collect();
            if values.len() != 1 || values[0].as_ref().ok() != Some(&expected) {
                bail!("invalid applicable TOC");
            }
            prepared.add(destination, content.clone(), false)?;
        }
        prepared.sources.push((checkout, image, true));
    }
    for config in &instance.config {
        register(&mut destinations, &config.path)?;
        if config.settings.is_empty() {
            continue;
        }
        let before = prepared.root.read(&config.path, false)?;
        let body = match &before {
            Image::Missing => "",
            Image::File(bytes) => std::str::from_utf8(bytes)?,
            _ => bail!("config must be a regular file"),
        };
        let rendered = render_config(body, &config.settings)?;
        prepared.add(
            config.path.clone(),
            Image::File(rendered.into_bytes()),
            false,
        )?;
    }
    for saved in &instance.saved_variables {
        register(&mut destinations, &saved.path)?;
        let path = saved
            .source
            .as_ref()
            .context("SavedVariables needs a source")?;
        let image = source(path, false)?;
        if !matches!(image, Image::File(_)) {
            bail!("SavedVariables source must be a file");
        }
        prepared.add(saved.path.clone(), image.clone(), saved.mode == "seed")?;
        prepared.sources.push((path.clone(), image, false));
    }
    for tree in &instance.seed_trees {
        register(&mut destinations, &tree.destination)?;
        let image = source(&tree.source, false)?;
        if !matches!(image, Image::Directory(_)) {
            bail!("seed_trees source must be a directory");
        }
        prepared.seed(&tree.destination, &image)?;
        prepared.sources.push((tree.source.clone(), image, false));
    }
    for (path, _, _) in &prepared.sources {
        // All source ancestors were opened without following links above.
        if destinations
            .iter()
            .any(|destination| overlap(path, &instance.root.join(destination)))
        {
            bail!("source/destination overlap: {}", path.display());
        }
    }
    Ok(prepared)
}

fn repository(addon: &AddonRepo) -> String {
    addon
        .repository
        .clone()
        .unwrap_or_else(|| format!("https://github.com/Ascension-Addons/{}.git", addon.id))
}

/// One entry of the compiled-in addon catalog (`addons.json`): the GitHub
/// repository, the branch to follow, and the reviewed commit hash that is
/// the source of the addon. `follow: false` marks a pin-only entry whose
/// origin is gone — importable from a matching local checkout, never
/// updated.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    id: String,
    repository: String,
    branch: String,
    revision: String,
    #[serde(default)]
    directories: Vec<AddonDirectory>,
    follow: bool,
}

/// Parse and validate the compiled-in catalog. Fails closed on duplicate
/// ids, non-HTTPS or non-GitHub repositories, malformed revisions, empty
/// branches, and entries with no directory mapping.
pub fn addon_catalog() -> Result<BTreeMap<String, CatalogEntry>> {
    #[derive(Deserialize)]
    struct Catalog {
        version: u32,
        addons: Vec<CatalogEntry>,
    }
    let catalog: Catalog = serde_json::from_str(include_str!("../addons.json"))
        .context("parse compiled-in addon catalog")?;
    if catalog.version != 1 {
        bail!("unsupported addon catalog version: {}", catalog.version);
    }
    validate_catalog(catalog.addons)
}

fn validate_catalog(addons: Vec<CatalogEntry>) -> Result<BTreeMap<String, CatalogEntry>> {
    let mut map = BTreeMap::new();
    for entry in addons {
        if entry.id.is_empty() {
            bail!("addon catalog has an empty id");
        }
        if !entry.repository.starts_with("https://github.com/") {
            bail!("addon catalog entry '{}' is not an https GitHub URL", entry.id);
        }
        if entry.branch.is_empty() {
            bail!("addon catalog entry '{}' has no branch", entry.id);
        }
        if entry.revision.len() != 40
            || !entry.revision.bytes().all(|c| c.is_ascii_hexdigit())
        {
            bail!("addon catalog entry '{}' has no full commit SHA-1", entry.id);
        }
        if entry.directories.is_empty() {
            bail!("addon catalog entry '{}' has no directory mapping", entry.id);
        }
        if map.insert(entry.id.clone(), entry).is_some() {
            bail!("duplicate addon catalog id");
        }
    }
    Ok(map)
}

/// Resolve instance addon references against the catalog. Instance fields
/// win when set; an empty branch and empty directories inherit the catalog
/// entry, while repository/revision fall back only when absent. Addons
/// unknown to the catalog pass through only with a complete inline
/// descriptor (repository required) — otherwise resolution fails closed
/// instead of guessing an origin.
pub fn resolve_addons(addons: &[AddonRepo]) -> Result<Vec<AddonRepo>> {
    let catalog = addon_catalog()?;
    addons
        .iter()
        .map(|addon| {
            let entry = catalog.get(&addon.id);
            let repository = addon
                .repository
                .clone()
                .or_else(|| entry.map(|entry| entry.repository.clone()));
            let repository = repository.with_context(|| {
                format!(
                    "addon '{}' is not in the catalog and declares no repository",
                    addon.id
                )
            })?;
            let branch = if addon.branch.is_empty() {
                entry.map(|entry| entry.branch.clone())
            } else {
                Some(addon.branch.clone())
            }
            .with_context(|| {
                format!(
                    "addon '{}' is not in the catalog and declares no branch",
                    addon.id
                )
            })?;
            let revision = addon
                .revision
                .clone()
                .or_else(|| entry.map(|entry| entry.revision.clone()));
            let directories = if addon.directories.is_empty() {
                entry.map(|entry| entry.directories.clone())
            } else {
                Some(addon.directories.clone())
            }
            .with_context(|| {
                format!(
                    "addon '{}' is not in the catalog and declares no directories",
                    addon.id
                )
            })?;
            Ok(AddonRepo {
                id: addon.id.clone(),
                branch,
                repository: Some(repository),
                directories,
                local_source: addon.local_source.clone(),
                revision,
            })
        })
        .collect()
}

/// Whether `update` follows an addon (fetches its branch) or leaves the
/// pin alone. Unknown ids default to following: a complete inline
/// descriptor is self-sufficient for update.
pub fn addon_follow(id: &str) -> Result<bool> {
    Ok(addon_catalog()?
        .get(id)
        .map(|entry| entry.follow)
        .unwrap_or(true))
}

fn digest_image(image: &Image) -> Result<String> {
    Ok(hex(&Sha256::digest(serde_json::to_vec(image)?)))
}

pub fn checkout_digest(path: &Path) -> Result<String> {
    digest_image(&source(path, true)?)
}

fn manifest(image: &Image, path: &Path, entries: &mut BTreeMap<PathBuf, Value>) {
    match image {
        Image::File(bytes) => {
            entries.insert(
                path.into(),
                serde_json::json!({"bytes": bytes.len(), "sha256": hex(&Sha256::digest(bytes))}),
            );
        }
        Image::Directory(children) => {
            for (name, child) in children {
                manifest(child, &path.join(name), entries);
            }
        }
        Image::Missing => {}
    }
}

pub fn snapshot(config: &Config, live: &Path, destination: &Path) -> Result<()> {
    if overlap(live, destination) {
        bail!("snapshot overlaps source");
    }
    let mut locks = Vec::new();
    for instance in config.instances.values() {
        let anchor = Anchor::open(&instance.root)?;
        locks.push(anchor.lock()?);
        assert_stopped(instance)?;
    }
    let mut accounts = BTreeMap::new();
    for account in ["CANIKO", "DEJANICA"] {
        let image = source(&live.join("WTF/Account").join(account), false)?;
        if !matches!(image, Image::Directory(_)) {
            bail!("account root must be a directory");
        }
        accounts.insert(account.into(), image);
    }
    let image = Image::Directory(accounts);
    let parent = Anchor::open(
        destination
            .parent()
            .context("snapshot needs existing private parent")?,
    )?;
    let name = Path::new(destination.file_name().context("snapshot needs a name")?);
    if parent.read(name, false)? != Image::Missing {
        bail!("snapshot destination already exists");
    }
    let staging = tempfile::Builder::new()
        .prefix(".snapshot-")
        .tempdir_in(parent.path())?;
    files::write_image(&staging.path().join("Account"), &image)?;
    if files::read_image(&staging.path().join("Account"), false)? != image {
        bail!("snapshot verification failed");
    }
    for (account, before) in match &image {
        Image::Directory(entries) => entries,
        _ => unreachable!(),
    } {
        if &source(&live.join("WTF/Account").join(account), false)? != before {
            bail!("live account changed during snapshot");
        }
    }
    for instance in config.instances.values() {
        assert_stopped(instance)?;
    }
    let mut entries = BTreeMap::new();
    manifest(&image, Path::new("Account"), &mut entries);
    atomic_write_0600(
        &staging.path().join("manifest.json"),
        &serde_json::to_vec_pretty(
            &serde_json::json!({"version":1,"source":live,"files":entries}),
        )?,
    )?;
    parent.revalidate()?;
    files::rename_noreplace(staging.path(), &parent.path().join(name))?;
    // TempDir's old staging path is now absent; the published snapshot is retained.
    println!(
        "verified private account snapshot: {}",
        destination.display()
    );
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotManifest {
    version: u64,
    source: PathBuf,
    files: BTreeMap<String, ManifestEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestEntry {
    bytes: u64,
    sha256: String,
}

/// Read-only snapshot gate: the snapshot root must contain exactly `Account/`
/// (with the two approved account namespaces) plus its manifest, and every
/// file must match the manifest inventory and digests. Symlinks, unsupported
/// objects and inspection errors fail via the anchored reader. An optional
/// approved manifest digest binds the manifest itself to prior evidence.
pub fn verify_snapshot(
    snapshot: &Path,
    manifest_path: &Path,
    expected_manifest_sha256: Option<&str>,
) -> Result<()> {
    for path in [snapshot, manifest_path] {
        if !path.is_absolute() {
            bail!("snapshot paths must be absolute: {}", path.display());
        }
    }
    if manifest_path.parent() != Some(snapshot) {
        bail!("snapshot manifest must live at the snapshot root");
    }
    let manifest_bytes = match files::read_image(manifest_path, false)? {
        Image::File(bytes) => bytes,
        _ => bail!("snapshot manifest must be a regular file"),
    };
    if let Some(expected) = expected_manifest_sha256 {
        if hex(&Sha256::digest(&manifest_bytes)) != expected.to_ascii_lowercase() {
            bail!("snapshot manifest identity differs from approved digest");
        }
    }
    let parsed: SnapshotManifest =
        serde_json::from_slice(&manifest_bytes).context("parse snapshot manifest")?;
    if parsed.version != 1 {
        bail!("unsupported snapshot manifest version");
    }
    for (name, entry) in &parsed.files {
        files::relative(Path::new(name))?;
        if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
            bail!("invalid digest in snapshot manifest for {name}");
        }
    }
    let tree = source(snapshot, false)?;
    let Image::Directory(top) = &tree else {
        bail!("snapshot root must be a directory");
    };
    let manifest_name = manifest_path
        .file_name()
        .context("snapshot manifest needs a name")?
        .to_string_lossy()
        .into_owned();
    if top.len() != 2
        || !matches!(top.get("Account"), Some(Image::Directory(_)))
        || top.get(&manifest_name) != Some(&Image::File(manifest_bytes.clone()))
    {
        bail!("snapshot root must contain only Account/ and its manifest");
    }
    let Image::Directory(accounts) = &top["Account"] else {
        unreachable!()
    };
    if accounts.len() != 2
        || !matches!(accounts.get("CANIKO"), Some(Image::Directory(_)))
        || !matches!(accounts.get("DEJANICA"), Some(Image::Directory(_)))
    {
        bail!("snapshot must contain exactly the CANIKO and DEJANICA namespaces");
    }
    let mut actual = BTreeMap::new();
    manifest(&top["Account"], Path::new("Account"), &mut actual);
    if actual.len() != parsed.files.len() {
        bail!("snapshot inventory differs from manifest");
    }
    for (name, entry) in &parsed.files {
        let key = PathBuf::from(name);
        let Some(value) = actual.get(&key) else {
            bail!("snapshot file missing from manifest inventory: {name}");
        };
        if value.get("bytes").and_then(Value::as_u64) != Some(entry.bytes)
            || value.get("sha256").and_then(Value::as_str) != Some(&entry.sha256)
        {
            bail!("snapshot file differs from manifest: {name}");
        }
    }
    println!(
        "verified snapshot {} from source {} ({} files)",
        snapshot.display(),
        parsed.source.display(),
        parsed.files.len()
    );
    Ok(())
}

fn git_bytes(path: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .current_dir(path)
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "--no-optional-locks",
        ])
        .args(args)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .output()
        .context("run read-only Git command")?;
    if !output.status.success() {
        bail!("read-only Git command failed for {}", path.display());
    }
    Ok(output.stdout)
}

fn insert_blob(image: &mut Image, path: &Path, bytes: Vec<u8>) -> Result<()> {
    let mut components = path.components();
    let name = components
        .next()
        .context("empty Git path")?
        .as_os_str()
        .to_str()
        .context("non-UTF8 Git path")?;
    let Image::Directory(entries) = image else {
        bail!("Git tree collision");
    };
    let remaining = components.as_path();
    if remaining.as_os_str().is_empty() {
        if entries.insert(name.into(), Image::File(bytes)).is_some() {
            bail!("duplicate Git path");
        }
    } else {
        insert_blob(
            entries
                .entry(name.into())
                .or_insert_with(|| Image::Directory(BTreeMap::new())),
            remaining,
            bytes,
        )?;
    }
    Ok(())
}

fn reviewed_checkout(addon: &AddonRepo) -> Result<(Image, LockedRepository)> {
    let path = addon
        .local_source
        .as_ref()
        .context("import requires local_source for every addon")?;
    let revision = addon
        .revision
        .as_ref()
        .context("import requires a reviewed exact revision")?;
    if revision.len() != 40 || !revision.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("import revision must be a full commit SHA-1");
    }
    let anchor = Anchor::open(path)?;
    let actual = String::from_utf8(git_bytes(&anchor.path(), &["rev-parse", "HEAD"])?)?;
    if actual.trim() != revision {
        bail!("local HEAD differs from reviewed revision");
    }
    let origin = String::from_utf8(git_bytes(&anchor.path(), &["remote", "get-url", "origin"])?)?;
    if origin.trim() != repository(addon) {
        bail!("local repository identity differs from declaration");
    }
    let mut expected = Image::Directory(BTreeMap::new());
    let listing = git_bytes(&anchor.path(), &["ls-tree", "-rz", "--full-tree", revision])?;
    for record in listing.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .context("invalid Git tree record")?;
        let metadata: Vec<_> = std::str::from_utf8(&record[..tab])?
            .split_whitespace()
            .collect();
        if metadata.len() != 3
            || !matches!(metadata[0], "100644" | "100755")
            || metadata[1] != "blob"
        {
            bail!("symlink, submodule or unsupported Git tree entry");
        }
        let name = Path::new(std::str::from_utf8(&record[tab + 1..])?);
        files::relative(name)?;
        if name.components().any(|c| c.as_os_str() == ".git") {
            bail!("Git metadata in payload");
        }
        insert_blob(
            &mut expected,
            name,
            git_bytes(&anchor.path(), &["cat-file", "blob", metadata[2]])?,
        )?;
    }
    let current = source(path, true)?;
    if current != expected {
        bail!(
            "local payload differs from commit (including untracked/ignored files): {}",
            path.display()
        );
    }
    let timestamp = String::from_utf8(git_bytes(
        &anchor.path(),
        &["show", "-s", "--format=%ct", revision],
    )?)?
    .trim()
    .parse::<u64>()?;
    let locked = LockedRepository {
        branch: addon.branch.clone(),
        revision: revision.clone(),
        repository: Some(repository(addon)),
        imported_from: Some(path.clone()),
        committed_at: Some(timestamp),
        content_sha256: Some(digest_image(&expected)?),
    };
    Ok((expected, locked))
}

pub fn import(config: &Config) -> Result<()> {
    let mut imports = Vec::new();
    for (name, instance) in &config.instances {
        validate_instance(name, instance)?;
        let payloads = instance
            .addons
            .iter()
            .map(|addon| Ok((addon.id.clone(), reviewed_checkout(addon)?)))
            .collect::<Result<Vec<_>>>()?;
        let state = state_dir(instance);
        let lock = instance
            .lock_file
            .as_ref()
            .context("import requires lock_file")?;
        if !lock.starts_with(&state) {
            bail!("import lock_file must be inside manager state");
        }
        imports.push((name, instance, payloads));
    }
    for (name, instance, payloads) in imports {
        let installation = Anchor::open(&instance.root)?;
        let _lease = installation.lock()?;
        assert_stopped(instance)?;
        let state = state_dir(instance);
        let system = Anchor::open(Path::new("/"))?;
        let state_relative = state.strip_prefix("/")?;
        let _state_parent =
            system.parent(&state_relative.join("placeholder"), true, &mut Vec::new())?;
        let state_anchor = Anchor::open(&state)?;
        let staging = tempfile::Builder::new()
            .prefix(".import-")
            .tempdir_in(state_anchor.path())?;
        let mut lock = read_lock(instance)?;
        for (id, (image, locked)) in &payloads {
            files::write_image(&staging.path().join(id), image)?;
            lock.repositories.insert(id.clone(), locked.clone());
        }
        // Refuse destructive imports. Existing source storage must already match.
        for (id, (image, _)) in &payloads {
            let old = state_anchor.read(&Path::new("repos").join(id), true)?;
            if old != Image::Missing && &old != image {
                bail!("import would replace existing source storage for {id}");
            }
        }
        for (id, (_, locked)) in &payloads {
            let destination = Path::new("repos").join(id);
            if state_anchor.read(&destination, true)? == Image::Missing {
                let (_parent, target) = state_anchor.target(&destination, true, &mut Vec::new())?;
                files::rename_noreplace(&staging.path().join(id), &target)?;
            }
            println!(
                "{name}: imported {id} {} (commit timestamp {}; historical import, not update freshness approval)",
                locked.revision,
                locked.committed_at.unwrap_or_default()
            );
        }
        let relative_lock = instance
            .lock_file
            .as_ref()
            .context("lock_file")?
            .strip_prefix(&state)?;
        let (_parent, lock_path) = state_anchor.target(relative_lock, true, &mut Vec::new())?;
        match state_anchor.read(relative_lock, false)? {
            Image::Missing | Image::File(_) => {}
            _ => bail!("invalid lock file"),
        }
        atomic_write_0600(&lock_path, &serde_json::to_vec_pretty(&lock)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture() -> (tempfile::TempDir, Instance) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("game");
        fs::create_dir_all(root.join("Interface/AddOns")).unwrap();
        let instance =
            serde_json::from_value(serde_json::json!({"root": root, "client": "wow-classic"}))
                .unwrap();
        (dir, instance)
    }

    fn git_input(repo: &Path, args: &[&str], bytes: &[u8]) -> String {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().into()
    }

    fn addon_fixture() -> (tempfile::TempDir, Instance, PathBuf) {
        let (dir, mut instance) = fixture();
        let repo = dir.path().join("repo");
        fs::create_dir(&repo).unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .arg(&repo)
                .status()
                .unwrap()
                .success()
        );
        let toc = b"## Interface: 11200\n";
        fs::write(repo.join("Example.toc"), toc).unwrap();
        let blob = git_input(&repo, &["hash-object", "-w", "--stdin"], toc);
        let tree = git_input(
            &repo,
            &["mktree"],
            format!("100644 blob {blob}\tExample.toc\n").as_bytes(),
        );
        // Raw, unpublished fixture objects avoid touching operator Git identity/configuration.
        let commit = git_input(&repo, &["hash-object", "-t", "commit", "-w", "--stdin"], format!(
            "tree {tree}\nauthor Fixture <fixture@example.invalid> 1 +0000\ncommitter Fixture <fixture@example.invalid> 1 +0000\n\nfixture\n"
        ).as_bytes());
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["update-ref", "HEAD", &commit])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args([
                    "remote",
                    "add",
                    "origin",
                    "https://example.invalid/addon.git"
                ])
                .status()
                .unwrap()
                .success()
        );
        instance.state_dir = Some(dir.path().join("state"));
        instance.lock_file = Some(dir.path().join("state/addons.lock.json"));
        instance.addons.push(AddonRepo {
            id: "Example".into(),
            branch: "master".into(),
            repository: Some("https://example.invalid/addon.git".into()),
            directories: vec![AddonDirectory {
                source: ".".into(),
                target: "Example".into(),
            }],
            local_source: Some(repo.clone()),
            revision: Some(commit),
        });
        (dir, instance, repo)
    }

    fn catalog_entry(id: &str) -> CatalogEntry {
        CatalogEntry {
            id: id.into(),
            repository: "https://github.com/example/addon.git".into(),
            branch: "master".into(),
            revision: "0".repeat(40),
            directories: vec![AddonDirectory {
                source: ".".into(),
                target: id.into(),
            }],
            follow: true,
        }
    }

    #[test]
    fn addon_catalog_lists_reviewed_registrations() {
        let catalog = addon_catalog().unwrap();
        assert_eq!(catalog.len(), 17);
        let pfui = &catalog["pfUI"];
        assert_eq!(pfui.repository, "https://github.com/shagu/pfUI.git");
        assert_eq!(pfui.branch, "master");
        assert_eq!(
            pfui.revision,
            "b2f6df84a93a4ce6adbe1fd8f0372454795151f1"
        );
        assert!(pfui.follow);
        // Pin-only entries (dead origins) never fetch.
        assert!(!catalog["aux-addon"].follow);
        assert!(!catalog["_LazyPig"].follow);
        assert!(!catalog["CleveRoidMacros"].follow);
        // Branch carries the line in use, not a stale default.
        assert_eq!(catalog["BetterCharacterStats"].branch, "main");
        assert_eq!(
            catalog["pfUI-Gryphons"].repository,
            "https://github.com/Macumbafeh/pfUI-Gryphons.git"
        );
    }

    #[test]
    fn addon_catalog_validation_fails_closed() {
        // Duplicate ids.
        let mut dup = vec![catalog_entry("A"), catalog_entry("A")];
        assert!(validate_catalog(std::mem::take(&mut dup)).is_err());
        // Non-HTTPS repository.
        let mut entry = catalog_entry("A");
        entry.repository = "git@github.com:example/addon.git".into();
        assert!(validate_catalog(vec![entry]).is_err());
        // Non-GitHub repository.
        let mut entry = catalog_entry("A");
        entry.repository = "https://example.invalid/addon.git".into();
        assert!(validate_catalog(vec![entry]).is_err());
        // Short revision.
        let mut entry = catalog_entry("A");
        entry.revision = "abc123".into();
        assert!(validate_catalog(vec![entry]).is_err());
        // Empty branch and missing directories.
        let mut entry = catalog_entry("A");
        entry.branch.clear();
        assert!(validate_catalog(vec![entry]).is_err());
        let mut entry = catalog_entry("A");
        entry.directories.clear();
        assert!(validate_catalog(vec![entry]).is_err());
    }

    fn addon_ref(id: &str) -> AddonRepo {
        AddonRepo {
            id: id.into(),
            branch: String::new(),
            repository: None,
            directories: Vec::new(),
            local_source: None,
            revision: None,
        }
    }

    #[test]
    fn resolve_addons_merges_catalog_and_overrides() {
        // Bare id inherits the whole catalog entry.
        let resolved = resolve_addons(&[addon_ref("pfUI")]).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].branch, "master");
        assert_eq!(
            resolved[0].repository.as_deref(),
            Some("https://github.com/shagu/pfUI.git")
        );
        assert_eq!(
            resolved[0].revision.as_deref(),
            Some("b2f6df84a93a4ce6adbe1fd8f0372454795151f1")
        );
        assert_eq!(resolved[0].directories.len(), 1);
        // Explicit fields win (fork trials outside the catalog).
        let mut trial = addon_ref("pfUI");
        trial.branch = "dev".into();
        trial.repository = Some("https://example.invalid/fork.git".into());
        trial.revision = Some("1".repeat(40));
        let resolved = resolve_addons(&[trial]).unwrap();
        assert_eq!(resolved[0].branch, "dev");
        assert_eq!(
            resolved[0].repository.as_deref(),
            Some("https://example.invalid/fork.git")
        );
        // Unknown ids pass through only with a complete inline descriptor.
        let mut inline = addon_ref("Custom");
        inline.branch = "master".into();
        inline.repository = Some("https://example.invalid/custom.git".into());
        inline.revision = Some("2".repeat(40));
        inline.directories = vec![AddonDirectory {
            source: ".".into(),
            target: "Custom".into(),
        }];
        let resolved = resolve_addons(&[inline]).unwrap();
        assert_eq!(resolved[0].branch, "master");
        // Unknown ids without a repository fail closed (no guessing origins).
        assert!(resolve_addons(&[addon_ref("Custom")]).is_err());
    }

    #[test]
    fn addon_follow_defaults_to_following() {
        assert!(addon_follow("pfUI").unwrap());
        assert!(!addon_follow("aux-addon").unwrap());
        assert!(addon_follow("not-in-catalog").unwrap());
    }

    #[test]
    fn historical_import_provenance_addon_drift_and_lock_stability() {
        let (_dir, instance, repo) = addon_fixture();
        assert!(assert_recent_checkout(&repo, "Example").is_err());
        let source_before = source(&repo, false).unwrap();
        let config = Config {
            version: 1,
            instances: BTreeMap::from([("test".into(), instance.clone())]),
        };
        import(&config).unwrap();
        assert_eq!(source(&repo, false).unwrap(), source_before);
        let lock_before = fs::read(instance.lock_file.as_ref().unwrap()).unwrap();
        let lock: LockFile = serde_json::from_slice(&lock_before).unwrap();
        assert_eq!(lock.repositories["Example"].committed_at, Some(1));
        assert_eq!(
            lock.repositories["Example"].imported_from,
            Some(repo.clone())
        );
        prepare("test", &instance)
            .unwrap()
            .apply(&instance)
            .unwrap();
        assert!(prepare("test", &instance).unwrap().operations.is_empty());
        let target = instance.root.join("Interface/AddOns/Example/Example.toc");
        fs::write(&target, "drift").unwrap();
        assert_eq!(prepare("test", &instance).unwrap().operations.len(), 1);
        prepare("test", &instance)
            .unwrap()
            .apply(&instance)
            .unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"## Interface: 11200\n");
        assert_eq!(
            fs::read(instance.lock_file.as_ref().unwrap()).unwrap(),
            lock_before
        );
        fs::write(repo.join("untracked"), "not represented by commit").unwrap();
        assert!(import(&config).is_err());
        assert_eq!(
            fs::read(instance.lock_file.as_ref().unwrap()).unwrap(),
            lock_before
        );
    }

    #[test]
    fn stale_locks_changed_storage_and_stock_targets_fail() {
        let (_dir, mut instance, _repo) = addon_fixture();
        let config = Config {
            version: 1,
            instances: BTreeMap::from([("test".into(), instance.clone())]),
        };
        assert!(
            prepare("test", &instance)
                .err()
                .unwrap()
                .to_string()
                .contains("blocked: missing source")
        );
        import(&config).unwrap();
        instance.addons[0].branch = "other".into();
        assert!(prepare("test", &instance).is_err());
        instance.addons[0].branch = "master".into();
        instance.addons[0].directories[0].target = "Blizzard_Test".into();
        assert!(prepare("test", &instance).is_err());
        instance.addons[0].directories[0].target = "Example".into();
        fs::write(
            checkout_path(&instance, "Example").join("Example.toc"),
            "changed",
        )
        .unwrap();
        assert!(prepare("test", &instance).is_err());
    }

    #[test]
    fn both_accounts_merge_reseed_and_second_apply_is_noop() {
        let (dir, mut instance) = fixture();
        let snapshot = dir.path().join("private");
        for account in ["CANIKO", "DEJANICA"] {
            fs::create_dir_all(snapshot.join(account).join("Nordanaar/Character")).unwrap();
            fs::write(snapshot.join(account).join("saved.lua"), "snapshot").unwrap();
            fs::write(
                snapshot
                    .join(account)
                    .join("Nordanaar/Character/bindings.wtf"),
                "exact\r\n",
            )
            .unwrap();
            instance.seed_trees.push(SeedTree {
                source: snapshot.join(account),
                destination: Path::new("WTF/Account").join(account),
            });
        }
        let existing = instance.root.join("WTF/Account/CANIKO/saved.lua");
        fs::create_dir_all(existing.parent().unwrap()).unwrap();
        fs::write(&existing, "mutable user bytes").unwrap();
        let prepared = prepare("test", &instance).unwrap();
        assert_eq!(prepared.operations.len(), 3);
        assert!(!instance.root.join("WTF/Account/DEJANICA").exists());
        prepared.apply(&instance).unwrap();
        drop(prepared);
        assert_eq!(fs::read_to_string(&existing).unwrap(), "mutable user bytes");
        let before = source(&instance.root, false).unwrap();
        let prepared = prepare("test", &instance).unwrap();
        assert!(prepared.plan("test").changes.is_empty());
        prepared.apply(&instance).unwrap();
        drop(prepared);
        assert_eq!(source(&instance.root, false).unwrap(), before);
        fs::remove_file(&existing).unwrap();
        prepare("test", &instance)
            .unwrap()
            .apply(&instance)
            .unwrap();
        assert_eq!(fs::read_to_string(existing).unwrap(), "snapshot");
    }

    #[test]
    fn seed_race_preserves_new_file_and_replace_is_content_sensitive() {
        let (dir, mut instance) = fixture();
        let seed = dir.path().join("seed.lua");
        fs::write(&seed, "seed").unwrap();
        instance.saved_variables.push(SavedVariables {
            path: "saved.lua".into(),
            mode: "seed".into(),
            source: Some(seed),
        });
        let prepared = prepare("test", &instance).unwrap();
        fs::write(instance.root.join("saved.lua"), "won race").unwrap();
        prepared.apply(&instance).unwrap();
        drop(prepared);
        assert_eq!(
            fs::read_to_string(instance.root.join("saved.lua")).unwrap(),
            "won race"
        );
        instance.saved_variables[0].mode = "replace".into();
        prepare("test", &instance)
            .unwrap()
            .apply(&instance)
            .unwrap();
        assert!(prepare("test", &instance).unwrap().operations.is_empty());
    }

    #[test]
    fn parent_symlink_source_change_and_collisions_fail_before_commit() {
        let (dir, mut instance) = fixture();
        instance.config.push(ConfigFile {
            path: "WTF/Config.wtf".into(),
            settings: BTreeMap::from([("autoSelfCast".into(), Value::Bool(true))]),
        });
        let prepared = prepare("test", &instance).unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, instance.root.join("WTF")).unwrap();
        assert!(prepared.apply(&instance).is_err());
        drop(prepared);
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
        fs::remove_file(instance.root.join("WTF")).unwrap();
        instance.config.push(instance.config[0].clone());
        assert!(prepare("test", &instance).is_err());
        instance.config.clear();
        let seed = dir.path().join("seed");
        fs::write(&seed, "before").unwrap();
        instance.saved_variables.push(SavedVariables {
            path: "saved".into(),
            mode: "seed".into(),
            source: Some(seed.clone()),
        });
        let prepared = prepare("test", &instance).unwrap();
        fs::write(&seed, "after").unwrap();
        assert!(prepared.apply(&instance).is_err());
        assert!(!instance.root.join("saved").exists());
    }

    #[test]
    fn rollback_restores_replacements_and_created_parents() {
        let (_dir, mut instance) = fixture();
        fs::write(instance.root.join("first"), "SET value \"old\"\n").unwrap();
        for path in ["first", "new/second", "third"] {
            instance.config.push(ConfigFile {
                path: path.into(),
                settings: BTreeMap::from([("value".into(), Value::String("new".into()))]),
            });
        }
        let prepared = prepare("test", &instance).unwrap();
        assert!(prepared.apply_inner(&instance, Some((2, false))).is_err());
        assert_eq!(
            fs::read_to_string(instance.root.join("first")).unwrap(),
            "SET value \"old\"\n"
        );
        assert!(!instance.root.join("new").exists());
        assert!(!instance.root.join("third").exists());
        let journals: Vec<_> = fs::read_dir(&instance.root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".modde-transaction-")
            })
            .collect();
        assert_eq!(journals.len(), 1);
        let journal: Value =
            serde_json::from_slice(&fs::read(journals[0].join("journal.json")).unwrap()).unwrap();
        assert_eq!(journal["status"], "rolled-back");
        assert_eq!(journal["committed"], serde_json::json!([0, 1]));
    }

    #[test]
    fn rollback_failure_retains_backup_and_does_not_overwrite_user_changes() {
        let (_dir, mut instance) = fixture();
        fs::write(instance.root.join("first"), "original").unwrap();
        for path in ["first", "second"] {
            instance.config.push(ConfigFile {
                path: path.into(),
                settings: BTreeMap::from([("value".into(), Value::Bool(true))]),
            });
        }
        let prepared = prepare("test", &instance).unwrap();
        assert!(prepared.apply_inner(&instance, Some((1, true))).is_err());
        assert_eq!(
            fs::read_to_string(instance.root.join("first")).unwrap(),
            "concurrent user data"
        );
        let recovery = fs::read_dir(&instance.root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".modde-transaction-")
            })
            .unwrap();
        assert_eq!(
            fs::read_to_string(recovery.join("old-0")).unwrap(),
            "original"
        );
        let journal: Value =
            serde_json::from_slice(&fs::read(recovery.join("journal.json")).unwrap()).unwrap();
        assert_eq!(journal["status"], "rollback-failed");
    }

    #[test]
    fn verified_snapshot_excludes_global_and_client_assets_and_never_overwrites() {
        let (dir, instance) = fixture();
        let live = dir.path().join("live");
        for account in ["CANIKO", "DEJANICA"] {
            fs::create_dir_all(live.join("WTF/Account").join(account)).unwrap();
            fs::write(
                live.join("WTF/Account").join(account).join("saved.lua"),
                account,
            )
            .unwrap();
        }
        fs::write(live.join("WTF/Config.wtf"), "do not copy").unwrap();
        fs::write(live.join("WoW.exe"), "do not copy").unwrap();
        let config = Config {
            version: 1,
            instances: BTreeMap::from([("test".into(), instance)]),
        };
        let destination = dir.path().join("snapshot");
        snapshot(&config, &live, &destination).unwrap();
        assert!(!destination.join("WTF/Config.wtf").exists());
        assert!(!destination.join("WoW.exe").exists());
        let manifest: Value =
            serde_json::from_slice(&fs::read(destination.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["files"].as_object().unwrap().len(), 2);
        let before = source(&destination, false).unwrap();
        assert!(snapshot(&config, &live, &destination).is_err());
        assert_eq!(source(&destination, false).unwrap(), before);
    }

    #[test]
    fn verify_snapshot_accepts_clean_and_rejects_drift() {
        let (dir, instance) = fixture();
        let live = dir.path().join("live");
        for account in ["CANIKO", "DEJANICA"] {
            fs::create_dir_all(live.join("WTF/Account").join(account)).unwrap();
            fs::write(
                live.join("WTF/Account").join(account).join("saved.lua"),
                account,
            )
            .unwrap();
        }
        let config = Config {
            version: 1,
            instances: BTreeMap::from([("test".into(), instance)]),
        };
        let destination = dir.path().join("snapshot");
        snapshot(&config, &live, &destination).unwrap();
        let manifest_path = destination.join("manifest.json");
        let manifest_bytes = fs::read(&manifest_path).unwrap();
        let manifest_digest = hex(&Sha256::digest(&manifest_bytes));
        verify_snapshot(&destination, &manifest_path, None).unwrap();
        verify_snapshot(&destination, &manifest_path, Some(&manifest_digest)).unwrap();
        assert!(verify_snapshot(&destination, &manifest_path, Some(&"0".repeat(64))).is_err());
        // Manifest outside the snapshot root is refused.
        let elsewhere = dir.path().join("manifest-copy.json");
        fs::write(&elsewhere, &manifest_bytes).unwrap();
        assert!(verify_snapshot(&destination, &elsewhere, None).is_err());
        let target = destination.join("Account/CANIKO/saved.lua");
        let before = fs::read(&target).unwrap();
        fs::write(&target, "tampered").unwrap();
        assert!(verify_snapshot(&destination, &manifest_path, None).is_err());
        fs::write(&target, &before).unwrap();
        verify_snapshot(&destination, &manifest_path, None).unwrap();
        let planted = destination.join("Account/CANIKO/planted.lua");
        fs::write(&planted, "extra").unwrap();
        assert!(verify_snapshot(&destination, &manifest_path, None).is_err());
        fs::remove_file(&planted).unwrap();
        fs::remove_file(&target).unwrap();
        assert!(verify_snapshot(&destination, &manifest_path, None).is_err());
        fs::write(&target, &before).unwrap();
        std::os::unix::fs::symlink(
            destination.join("Account/DEJANICA"),
            destination.join("Account/CANIKO/dirlink"),
        )
        .unwrap();
        assert!(verify_snapshot(&destination, &manifest_path, None).is_err());
        fs::remove_file(destination.join("Account/CANIKO/dirlink")).unwrap();
        fs::write(&manifest_path, "not json").unwrap();
        assert!(verify_snapshot(&destination, &manifest_path, None).is_err());
        fs::write(&manifest_path, &manifest_bytes).unwrap();
        verify_snapshot(&destination, &manifest_path, None).unwrap();
        // A snapshot missing one approved namespace is refused.
        let partial = dir.path().join("partial");
        fs::create_dir_all(partial.join("Account/CANIKO")).unwrap();
        fs::write(partial.join("Account/CANIKO/saved.lua"), "CANIKO").unwrap();
        let partial_manifest = partial.join("manifest.json");
        fs::write(
            &partial_manifest,
            serde_json::to_vec(&serde_json::json!({"version": 1, "source": live,
                "files": {"Account/CANIKO/saved.lua":
                    {"bytes": 6, "sha256": hex(&Sha256::digest(b"CANIKO"))}}}))
            .unwrap(),
        )
        .unwrap();
        assert!(verify_snapshot(&partial, &partial_manifest, None).is_err());
    }

    #[test]
    fn instance_lock_and_no_clobber_rename_are_enforced() {
        let (dir, instance) = fixture();
        let holder = Anchor::open(&instance.root).unwrap();
        let _lease = holder.lock().unwrap();
        assert!(
            prepare("test", &instance)
                .unwrap()
                .apply(&instance)
                .is_err()
        );
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        fs::write(&from, "new").unwrap();
        fs::write(&to, "existing").unwrap();
        assert!(files::rename_noreplace(&from, &to).is_err());
        assert_eq!(fs::read_to_string(to).unwrap(), "existing");
        assert_eq!(fs::read_to_string(from).unwrap(), "new");
    }

    #[test]
    fn process_guard_blocks_mutation() {
        let (_dir, mut instance) = fixture();
        instance.processes = vec!["[".into()]; // Invalid pgrep regex must not mean stopped.
        assert!(prepare("test", &instance).is_err());
        instance.processes = vec![format!("^{}$", std::process::id())];
        // A valid inspection with no matching command is allowed; no game is launched.
        assert_stopped(&instance).unwrap();
    }
}
