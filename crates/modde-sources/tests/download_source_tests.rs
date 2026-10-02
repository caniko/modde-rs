//! Unit tests for download source implementations.
//!
//! Tests `can_handle()` dispatch, `resolve()` output, and `DownloadHandle` construction
//! without requiring network access.

use modde_core::GameId;
use std::collections::HashMap;

use modde_core::manifest::wabbajack::DownloadDirective;
use modde_sources::traits::{AnySource, DownloadHandle, DownloadSource, VerifiedFile};

// ── DownloadHandle construction ────────────────────────────────────

#[test]
fn test_download_handle_default_fields() {
    let handle = DownloadHandle {
        url: "https://example.com/file.zip".to_string(),
        candidate_urls: Vec::new(),
        headers: HashMap::new(),
        expected_hash: 12345,
        size_hint: None,
    };

    assert_eq!(handle.url, "https://example.com/file.zip");
    assert_eq!(handle.candidate_urls, [] as [std::string::String; 0]);
    assert!(handle.headers.is_empty());
    assert_eq!(handle.expected_hash, 12345);
    assert!(handle.size_hint.is_none());
}

#[test]
fn test_download_handle_with_headers() {
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), "Bearer token".to_string());
    headers.insert("User-Agent".to_string(), "modde/1.0".to_string());

    let handle = DownloadHandle {
        url: "https://cdn.example.com/file.zip".to_string(),
        candidate_urls: Vec::new(),
        headers,
        expected_hash: 0,
        size_hint: Some(1024 * 1024),
    };

    assert_eq!(handle.headers.len(), 2);
    assert_eq!(handle.size_hint, Some(1048576));
}

#[test]
fn test_download_handle_clone() {
    let handle = DownloadHandle {
        url: "https://example.com/file.zip".to_string(),
        candidate_urls: vec!["https://mirror.example.com/file.zip".to_string()],
        headers: HashMap::from([("key".to_string(), "value".to_string())]),
        expected_hash: 42,
        size_hint: Some(100),
    };

    let cloned = handle.clone();
    assert_eq!(handle.url, cloned.url);
    assert_eq!(handle.candidate_urls, cloned.candidate_urls);
    assert_eq!(handle.expected_hash, cloned.expected_hash);
    assert_eq!(handle.headers, cloned.headers);
    assert_eq!(handle.size_hint, cloned.size_hint);
}

// ── VerifiedFile ───────────────────────────────────────────────────

#[test]
fn test_verified_file_creation() {
    let vf = VerifiedFile {
        path: std::path::PathBuf::from("/store/mod.zip"),
        hash: 99999,
    };
    assert_eq!(vf.path.to_str().unwrap(), "/store/mod.zip");
    assert_eq!(vf.hash, 99999);
}

#[test]
fn test_verified_file_clone() {
    let vf = VerifiedFile {
        path: std::path::PathBuf::from("/tmp/file.dat"),
        hash: 42,
    };
    let cloned = vf.clone();
    assert_eq!(vf.path, cloned.path);
    assert_eq!(vf.hash, cloned.hash);
}

// ── DirectSource can_handle ────────────────────────────────────────

#[test]
fn test_direct_source_can_handle_direct_url() {
    let client = reqwest::Client::new();
    let source = modde_sources::direct::DirectSource::new(client);

    let directive = DownloadDirective::DirectURL {
        url: "https://example.com/mod.zip".to_string(),
        headers: HashMap::new(),
        mirror_resolver: None,
        hash: 123,
    };
    assert!(source.can_handle(&directive));
}

#[test]
fn test_direct_source_rejects_nexus() {
    let client = reqwest::Client::new();
    let source = modde_sources::direct::DirectSource::new(client);

    let directive = DownloadDirective::Nexus {
        game_id: GameId::from("skyrimse"),
        mod_id: 1.into(),
        file_id: 1.into(),
        hash: 123,
    };
    assert!(!source.can_handle(&directive));
}

#[test]
fn test_direct_source_rejects_github() {
    let client = reqwest::Client::new();
    let source = modde_sources::direct::DirectSource::new(client);

    let directive = DownloadDirective::GitHub {
        user: "u".to_string(),
        repo: "r".to_string(),
        tag: "t".to_string(),
        asset: "a".to_string(),
        hash: 123,
    };
    assert!(!source.can_handle(&directive));
}

// ── DirectSource resolve ───────────────────────────────────────────

#[tokio::test]
async fn test_direct_source_resolve() {
    let client = reqwest::Client::new();
    let source = modde_sources::direct::DirectSource::new(client);

    let mut headers = HashMap::new();
    headers.insert("X-Custom".to_string(), "value".to_string());

    let directive = DownloadDirective::DirectURL {
        url: "https://cdn.example.com/mod.zip".to_string(),
        headers: headers.clone(),
        mirror_resolver: None,
        hash: 12345,
    };

    let handle = source.resolve(&directive).await.unwrap();
    assert_eq!(handle.url, "https://cdn.example.com/mod.zip");
    assert_eq!(handle.expected_hash, 12345);
    assert_eq!(handle.headers.get("X-Custom").unwrap(), "value");
    assert_eq!(handle.candidate_urls, [] as [std::string::String; 0]);
    assert!(handle.size_hint.is_none());
}

#[tokio::test]
async fn test_direct_source_resolve_wrong_type_errors() {
    let client = reqwest::Client::new();
    let source = modde_sources::direct::DirectSource::new(client);

    let directive = DownloadDirective::Nexus {
        game_id: GameId::from("skyrimse"),
        mod_id: 1.into(),
        file_id: 1.into(),
        hash: 1,
    };

    let result = source.resolve(&directive).await;
    assert!(result.is_err());
}

// ── GitHubSource can_handle ────────────────────────────────────────

#[test]
fn test_github_source_can_handle_github() {
    let client = reqwest::Client::new();
    let source = modde_sources::github::GitHubSource::new(client);

    let directive = DownloadDirective::GitHub {
        user: "user".to_string(),
        repo: "repo".to_string(),
        tag: "v1.0".to_string(),
        asset: "release.zip".to_string(),
        hash: 42,
    };
    assert!(source.can_handle(&directive));
}

#[test]
fn test_github_source_rejects_direct() {
    let client = reqwest::Client::new();
    let source = modde_sources::github::GitHubSource::new(client);

    let directive = DownloadDirective::DirectURL {
        url: "https://example.com".to_string(),
        headers: HashMap::new(),
        mirror_resolver: None,
        hash: 1,
    };
    assert!(!source.can_handle(&directive));
}

#[test]
fn test_github_source_rejects_mega() {
    let client = reqwest::Client::new();
    let source = modde_sources::github::GitHubSource::new(client);

    let directive = DownloadDirective::Mega {
        url: "https://mega.nz/file/X#Y".to_string(),
        hash: 1,
    };
    assert!(!source.can_handle(&directive));
}

// ── GoogleDriveSource can_handle ───────────────────────────────────

#[test]
fn test_gdrive_source_can_handle_gdrive() {
    let client = reqwest::Client::new();
    let source = modde_sources::gdrive::GoogleDriveSource::new(client);

    let directive = DownloadDirective::GoogleDrive {
        id: "1aBcDeF".to_string(),
        hash: 42,
    };
    assert!(source.can_handle(&directive));
}

#[test]
fn test_gdrive_source_rejects_nexus() {
    let client = reqwest::Client::new();
    let source = modde_sources::gdrive::GoogleDriveSource::new(client);

    let directive = DownloadDirective::Nexus {
        game_id: GameId::from("skyrimse"),
        mod_id: 1.into(),
        file_id: 1.into(),
        hash: 1,
    };
    assert!(!source.can_handle(&directive));
}

#[tokio::test]
async fn test_gdrive_source_resolve() {
    let client = reqwest::Client::new();
    let source = modde_sources::gdrive::GoogleDriveSource::new(client);

    let directive = DownloadDirective::GoogleDrive {
        id: "abc123xyz".to_string(),
        hash: 99999,
    };

    let handle = source.resolve(&directive).await.unwrap();
    assert!(handle.url.contains("abc123xyz"));
    assert!(handle.url.contains("drive.usercontent.google.com"));
    assert_eq!(handle.expected_hash, 99999);
    assert!(handle.size_hint.is_none());
}

#[tokio::test]
async fn test_gdrive_source_resolve_wrong_type_errors() {
    let client = reqwest::Client::new();
    let source = modde_sources::gdrive::GoogleDriveSource::new(client);

    let directive = DownloadDirective::GitHub {
        user: "u".to_string(),
        repo: "r".to_string(),
        tag: "t".to_string(),
        asset: "a".to_string(),
        hash: 1,
    };

    let result = source.resolve(&directive).await;
    assert!(result.is_err());
}

// ── MegaSource can_handle ──────────────────────────────────────────

#[test]
fn test_mega_source_can_handle_mega() {
    let client = reqwest::Client::new();
    let source = modde_sources::mega::MegaSource::new(client);

    let directive = DownloadDirective::Mega {
        url: "https://mega.nz/file/HANDLE#KEY".to_string(),
        hash: 42,
    };
    assert!(source.can_handle(&directive));
}

#[test]
fn test_mega_source_rejects_direct() {
    let client = reqwest::Client::new();
    let source = modde_sources::mega::MegaSource::new(client);

    let directive = DownloadDirective::DirectURL {
        url: "https://example.com".to_string(),
        headers: HashMap::new(),
        mirror_resolver: None,
        hash: 1,
    };
    assert!(!source.can_handle(&directive));
}

#[test]
fn test_mega_source_rejects_github() {
    let client = reqwest::Client::new();
    let source = modde_sources::mega::MegaSource::new(client);

    let directive = DownloadDirective::GitHub {
        user: "u".to_string(),
        repo: "r".to_string(),
        tag: "t".to_string(),
        asset: "a".to_string(),
        hash: 1,
    };
    assert!(!source.can_handle(&directive));
}

#[test]
fn test_mega_source_rejects_gdrive() {
    let client = reqwest::Client::new();
    let source = modde_sources::mega::MegaSource::new(client);

    let directive = DownloadDirective::GoogleDrive {
        id: "abc".to_string(),
        hash: 1,
    };
    assert!(!source.can_handle(&directive));
}

// ── Source dispatch correctness ────────────────────────────────────

#[test]
fn test_exactly_one_source_handles_each_directive_type() {
    let client = reqwest::Client::new();
    let sources: Vec<AnySource> = vec![
        AnySource::Direct(modde_sources::direct::DirectSource::new(client.clone())),
        AnySource::GitHub(modde_sources::github::GitHubSource::new(client.clone())),
        AnySource::GoogleDrive(modde_sources::gdrive::GoogleDriveSource::new(
            client.clone(),
        )),
        AnySource::Mega(modde_sources::mega::MegaSource::new(client.clone())),
        AnySource::WabbajackCdn(modde_sources::wabbajack::cdn::WabbajackCdnSource::new(
            client.clone(),
        )),
    ];

    let test_directives: Vec<DownloadDirective> = vec![
        DownloadDirective::DirectURL {
            url: "https://x.com".to_string(),
            headers: HashMap::new(),
            mirror_resolver: None,
            hash: 1,
        },
        DownloadDirective::GitHub {
            user: "u".to_string(),
            repo: "r".to_string(),
            tag: "t".to_string(),
            asset: "a".to_string(),
            hash: 2,
        },
        DownloadDirective::GoogleDrive {
            id: "id".to_string(),
            hash: 3,
        },
        DownloadDirective::Mega {
            url: "https://mega.nz/file/X#Y".to_string(),
            hash: 4,
        },
        DownloadDirective::WabbajackCdn {
            url: "https://authored-files.wabbajack.org/file.zip_abc".to_string(),
            hash: 5,
        },
    ];

    for directive in &test_directives {
        let handlers: Vec<_> = sources.iter().filter(|s| s.can_handle(directive)).collect();
        assert_eq!(
            handlers.len(),
            1,
            "exactly one source should handle {directive:?}"
        );
    }
}

#[test]
fn test_no_source_handles_nexus_without_nexus_source() {
    // NexusSource is not included (requires API key)
    let client = reqwest::Client::new();
    let sources: Vec<AnySource> = vec![
        AnySource::Direct(modde_sources::direct::DirectSource::new(client.clone())),
        AnySource::GitHub(modde_sources::github::GitHubSource::new(client.clone())),
        AnySource::GoogleDrive(modde_sources::gdrive::GoogleDriveSource::new(
            client.clone(),
        )),
        AnySource::Mega(modde_sources::mega::MegaSource::new(client.clone())),
    ];

    let nexus = DownloadDirective::Nexus {
        game_id: GameId::from("skyrimse"),
        mod_id: 42.into(),
        file_id: 99.into(),
        hash: 5,
    };

    let handlers: Vec<_> = sources.iter().filter(|s| s.can_handle(&nexus)).collect();
    assert_eq!(handlers.len(), 0, "no basic source should handle Nexus");
}
