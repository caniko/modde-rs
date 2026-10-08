use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

use modde_core::manifest::collection::CollectionManifest;
use modde_core::manifest::wabbajack::{InstallDirective, WabbajackManifest};
use modde_core::{
    NexusFileId, NexusModId, TransactionArtifact, TransactionError, TransactionPackage,
    TransactionPlan, TransactionProvenance, TransactionVersion,
};
use resolvo::utils::{Pool, VersionSet};
use resolvo::{
    Candidates, Condition, ConditionId, ConditionalRequirement, Dependencies, DependencyProvider,
    HintDependenciesAvailable, Interner, KnownDependencies, NameId, Problem, SolvableId, Solver,
    SolverCache, StringId, VersionSetId, VersionSetUnionId,
};

use crate::nexus::api::NexusApi;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ModdeVersionSet {
    Exact(TransactionVersion),
}

impl fmt::Display for ModdeVersionSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact(version) => version.fmt(f),
        }
    }
}

impl VersionSet for ModdeVersionSet {
    type V = CandidateRecord;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CandidateRecord {
    version: TransactionVersion,
}

impl fmt::Display for CandidateRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.version.fmt(f)
    }
}

#[derive(Debug, Clone)]
struct CandidateMeta {
    artifact: TransactionArtifact,
    rank: usize,
    dependencies: Vec<ExactRequirement>,
}

#[derive(Debug, Clone)]
struct ExactRequirement {
    package: String,
    version: TransactionVersion,
}

/// A `resolvo` dependency provider for modde's exact artifact transactions.
pub struct ModdeDependencyProvider {
    pool: Pool<ModdeVersionSet>,
    packages: Mutex<HashMap<String, Vec<SolvableId>>>,
    candidate_meta: Mutex<HashMap<SolvableId, CandidateMeta>>,
    nexus_api: Option<NexusApi>,
    lazy_nexus: Mutex<HashMap<String, LazyNexusPackage>>,
    metadata_errors: Mutex<Vec<String>>,
}

impl Default for ModdeDependencyProvider {
    fn default() -> Self {
        Self {
            pool: Pool::new(),
            packages: Mutex::new(HashMap::new()),
            candidate_meta: Mutex::new(HashMap::new()),
            nexus_api: None,
            lazy_nexus: Mutex::new(HashMap::new()),
            metadata_errors: Mutex::new(Vec::new()),
        }
    }
}

#[derive(Debug, Clone)]
struct LazyNexusPackage {
    game_domain: String,
    mod_id: NexusModId,
}

impl ModdeDependencyProvider {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_nexus_api(nexus_api: NexusApi) -> Self {
        Self {
            nexus_api: Some(nexus_api),
            ..Self::default()
        }
    }

    pub fn add_lazy_nexus_mod(&self, game_domain: String, mod_id: NexusModId) {
        let package = TransactionPackage::NexusMod {
            game_domain: game_domain.clone(),
            mod_id,
        };
        let package_key = Self::package_key(&package);
        self.pool.intern_package_name(package_key.clone());
        self.lazy_nexus
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                package_key,
                LazyNexusPackage {
                    game_domain,
                    mod_id,
                },
            );
    }

    fn package_key(package: &TransactionPackage) -> String {
        package.to_string()
    }

    fn add_artifact(
        &self,
        artifact: TransactionArtifact,
        rank: usize,
        dependencies: Vec<ExactRequirement>,
    ) -> SolvableId {
        let package = Self::package_key(&artifact.package);
        let name = self.pool.intern_package_name(package.clone());
        let solvable = self.pool.intern_solvable(
            name,
            CandidateRecord {
                version: artifact.version.clone(),
            },
        );

        self.packages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(package)
            .or_default()
            .push(solvable);
        self.candidate_meta
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                solvable,
                CandidateMeta {
                    artifact,
                    rank,
                    dependencies,
                },
            );
        solvable
    }

    fn exact_requirement(
        &self,
        package: &str,
        version: TransactionVersion,
    ) -> ConditionalRequirement {
        let name = self.pool.intern_package_name(package.to_string());
        let version_set = self
            .pool
            .intern_version_set(name, ModdeVersionSet::Exact(version));
        ConditionalRequirement {
            condition: None,
            requirement: version_set.into(),
        }
    }

    fn artifact_for(&self, solvable: SolvableId) -> Option<CandidateMeta> {
        self.candidate_meta
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&solvable)
            .cloned()
    }

    async fn fetch_lazy_nexus_candidates(&self, package_name: &str) -> Option<Vec<SolvableId>> {
        let lazy = self
            .lazy_nexus
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(package_name)
            .cloned()?;
        let api = self.nexus_api.as_ref()?;
        let files = match api.get_mod_files(&lazy.game_domain, lazy.mod_id).await {
            Ok(files) => files.files,
            Err(error) => {
                self.metadata_errors
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(format!(
                        "Nexus metadata unavailable for mod {} in {}: {error:#}",
                        lazy.mod_id, lazy.game_domain
                    ));
                return None;
            }
        };

        let mut solvables = Vec::with_capacity(files.len());
        for (rank, file) in files.into_iter().enumerate() {
            let package = TransactionPackage::NexusMod {
                game_domain: lazy.game_domain.clone(),
                mod_id: lazy.mod_id,
            };
            let version = TransactionVersion::NexusFile {
                file_id: file.file_id,
            };
            let artifact = TransactionArtifact {
                package,
                version,
                display_name: file.name,
                display_version: file.version,
                enabled_by_default: true,
                provenance: TransactionProvenance::NexusLazyMetadata,
            };
            solvables.push(self.add_artifact(artifact, rank, Vec::new()));
        }
        Some(solvables)
    }

    fn metadata_error(&self) -> Option<String> {
        self.metadata_errors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .first()
            .cloned()
    }
}

impl Interner for ModdeDependencyProvider {
    type NameId = NameId;
    type SolvableId = SolvableId;

    fn display_solvable(&self, solvable: SolvableId) -> impl fmt::Display + '_ {
        let record = self.pool.resolve_solvable(solvable);
        format!("{} {}", self.display_name(record.name), record.record)
    }

    fn display_name(&self, name: NameId) -> impl fmt::Display + '_ {
        self.pool.resolve_package_name(name).clone()
    }

    fn display_version_set(&self, version_set: VersionSetId) -> impl fmt::Display + '_ {
        self.pool.resolve_version_set(version_set).clone()
    }

    fn display_string(&self, string_id: StringId) -> impl fmt::Display + '_ {
        self.pool.resolve_string(string_id).to_owned()
    }

    fn version_set_name(&self, version_set: VersionSetId) -> NameId {
        self.pool.resolve_version_set_package_name(version_set)
    }

    fn solvable_name(&self, solvable: SolvableId) -> NameId {
        self.pool.resolve_solvable(solvable).name
    }

    fn version_sets_in_union(
        &self,
        version_set_union: VersionSetUnionId,
    ) -> impl Iterator<Item = VersionSetId> {
        self.pool.resolve_version_set_union(version_set_union)
    }

    fn resolve_condition(&self, condition: ConditionId) -> Condition {
        self.pool.resolve_condition(condition).clone()
    }
}

impl DependencyProvider for ModdeDependencyProvider {
    async fn filter_candidates(
        &self,
        candidates: &[SolvableId],
        version_set: VersionSetId,
        inverse: bool,
    ) -> Vec<SolvableId> {
        let version_set = self.pool.resolve_version_set(version_set);
        candidates
            .iter()
            .copied()
            .filter(|candidate| {
                let record = &self.pool.resolve_solvable(*candidate).record;
                let matches = match version_set {
                    ModdeVersionSet::Exact(version) => &record.version == version,
                };
                matches != inverse
            })
            .collect()
    }

    async fn get_candidates(&self, name: NameId) -> Option<Candidates> {
        let package_name = self.pool.resolve_package_name(name).clone();
        let candidates = self
            .packages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&package_name)
            .cloned()
            .or(None);
        let candidates = match candidates {
            Some(candidates) => candidates,
            None => self.fetch_lazy_nexus_candidates(&package_name).await?,
        };

        Some(Candidates {
            candidates,
            hint_dependencies_available: HintDependenciesAvailable::All,
            ..Candidates::default()
        })
    }

    async fn sort_candidates(&self, _solver: &SolverCache<Self>, solvables: &mut [SolvableId]) {
        solvables.sort_by_key(|solvable| {
            self.artifact_for(*solvable)
                .map_or(usize::MAX, |meta| meta.rank)
        });
    }

    async fn get_dependencies(&self, solvable: SolvableId) -> Dependencies {
        let Some(meta) = self.artifact_for(solvable) else {
            return Dependencies::Known(KnownDependencies::default());
        };

        let requirements = meta
            .dependencies
            .into_iter()
            .map(|dep| self.exact_requirement(&dep.package, dep.version))
            .collect();
        Dependencies::Known(KnownDependencies {
            requirements,
            constrains: Vec::new(),
        })
    }
}

/// Resolve a Nexus Collection manifest as an exact artifact transaction.
mod preflight;
pub use preflight::{
    preflight_collection_transaction, preflight_lazy_nexus_file_transaction,
    preflight_wabbajack_transaction,
};
