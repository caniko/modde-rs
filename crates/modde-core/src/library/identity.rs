use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallationIdentity {
    pub paths: BTreeSet<PathBuf>,
    pub entitlements: BTreeSet<String>,
    /// The target observed when an alias was bound, never re-resolved on lookup.
    #[serde(default)]
    pub resolved_paths: BTreeMap<PathBuf, PathBuf>,
}

/// Resolve existing ancestors too, so an as-yet uncreated save directory does
/// not change its identity when created beneath a symlinked library root.
pub fn normalized_path(path: &Path) -> PathBuf {
    if let Ok(path) = path.canonicalize() {
        return path;
    }
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        return normalized_path(parent).join(name);
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if result.file_name().is_some_and(|name| name != "..") {
                    result.pop();
                } else if !result.has_root() {
                    result.push(component);
                }
            }
            _ => result.push(component),
        }
    }
    if result != path {
        normalized_path(&result)
    } else {
        result
    }
}

impl LibraryPreferences {
    /// Reconcile a physical installation under the preferences lock. Preserve
    /// old keys rather than moving vaults, active slots, or sandbox directories.
    /// Multiple used keys cannot be merged safely without inspecting their DBs.
    pub fn bind_installation(
        &mut self,
        paths: &[PathBuf],
        old_ids: &[String],
        entitlements: &[String],
    ) -> Result<String> {
        let first = paths.first().context("installation has no path")?;
        let resolved: BTreeMap<_, _> = paths
            .iter()
            .map(|path| (path.clone(), normalized_path(path)))
            .collect();
        for identity in self.installations.values() {
            for (alias, target) in &resolved {
                let changed = match identity.resolved_paths.get(alias) {
                    Some(previous) => previous != target,
                    // Old records stored both the alias and its canonical target.
                    // Do not use today's filesystem to reinterpret those records.
                    None => identity.paths.contains(alias) && !identity.paths.contains(target),
                };
                if changed {
                    bail!(
                        "installation alias {} changed target to {}; explicitly rebind it before launching",
                        alias.display(),
                        target.display()
                    );
                }
            }
        }
        let aliases: BTreeSet<_> = resolved.keys().chain(resolved.values()).cloned().collect();
        let mut candidates: BTreeSet<_> = self
            .installations
            .iter()
            .filter(|(_, identity)| {
                resolved.values().any(|target| {
                    identity
                        .resolved_paths
                        .values()
                        .any(|previous| previous == target)
                        || (identity.resolved_paths.is_empty() && identity.paths.contains(target))
                })
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in old_ids {
            if self.launches.contains_key(id)
                || self.needs_deploy.contains(id)
                || self
                    .legacy_save_bindings
                    .values()
                    .any(|binding| &binding.installation == id)
            {
                candidates.insert(id.clone());
            }
        }
        if candidates.len() > 1 {
            bail!(
                "installation {} has conflicting saved identities ({}); reconcile them before launching",
                first.display(),
                candidates.into_iter().collect::<Vec<_>>().join(", ")
            );
        }
        let id = candidates
            .into_iter()
            .next()
            .or_else(|| old_ids.first().cloned())
            .unwrap_or_else(|| installation_id("installation", &normalized_path(first)));
        let identity = self.installations.entry(id.clone()).or_default();
        identity.paths.extend(aliases);
        for (alias, target) in resolved {
            identity.resolved_paths.insert(alias, target.clone());
            identity.resolved_paths.insert(target.clone(), target);
        }
        identity.entitlements.extend(entitlements.iter().cloned());
        // Only migrate a local favorite when a store first claims the copy.
        // Re-propagating all aliases would resurrect a removed favorite.
        let stores: Vec<_> = entitlements
            .iter()
            .filter(|key| !key.starts_with("local:"))
            .cloned()
            .collect();
        if !stores.is_empty()
            && identity
                .entitlements
                .iter()
                .any(|key| key.starts_with("local:") && self.favorites.contains(key))
        {
            for key in identity
                .entitlements
                .iter()
                .filter(|key| key.starts_with("local:"))
            {
                self.favorites.remove(key);
            }
            self.favorites.extend(stores);
        }
        Ok(id)
    }
}
