//! Default `GameTool` behaviour for a tool implementing only the required
//! methods.

use std::path::PathBuf;

use modde_games::tools::{
    AppliedFiles, GameTool, ToolApplyPreview, ToolAvailability, ToolCategory, ToolConfig,
    ToolReleaseSummary,
};
use smallvec::SmallVec;

struct Minimal;

impl GameTool for Minimal {
    fn tool_id(&self) -> &'static str {
        "minimal"
    }

    fn display_name(&self) -> &'static str {
        "Minimal Tool"
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Overlay
    }

    fn detect_available(&self) -> ToolAvailability {
        ToolAvailability::Available { version: None }
    }

    fn env_vars(&self, _config: &ToolConfig) -> SmallVec<[(String, String); 4]> {
        SmallVec::from_vec(vec![("MINIMAL".to_string(), "1".to_string())])
    }

    fn default_config(&self) -> ToolConfig {
        ToolConfig::new("minimal")
    }
}

#[tokio::test]
async fn defaults_describe_a_tool_without_optional_features() {
    let tool = Minimal;
    let config = tool.default_config_for(None);
    let game = tempfile::tempdir().unwrap();

    assert_eq!(config.tool_id, "minimal");
    assert_eq!(tool.description(), "Game-specific tool integration.");
    assert!(matches!(
        tool.settings_schema_for(None, &config).as_slice(),
        []
    ));
    assert_eq!(
        tool.env_vars_for(None, &config).as_slice(),
        [("MINIMAL".to_string(), "1".to_string())]
    );
    assert!(tool.wrapper_command(&config).is_none());
    assert!(matches!(
        tool.wine_dll_overrides_for(None, &config).as_slice(),
        []
    ));
    assert!(matches!(
        tool.apply_for(game.path(), None, &config)
            .unwrap()
            .files
            .as_slice(),
        []
    ));
    assert_eq!(
        tool.preview_apply_for(game.path(), None, &config).unwrap(),
        ToolApplyPreview::default()
    );
    assert!(tool.generate_config_for(None, &config).is_none());
    assert!(!tool.supports_releases());

    let release = ToolReleaseSummary {
        tag: "v1".to_string(),
        name: None,
        published_at: None,
        assets: Vec::new(),
    };
    assert!(matches!(
        tool.installable_release_assets(&release).as_slice(),
        []
    ));
    let listed = tool.list_releases().await.unwrap_err().to_string();
    assert!(
        listed.contains("Minimal Tool does not support release selection"),
        "{listed}"
    );
    let installed = tool
        .install_release("game", config.clone(), "v1", "a.zip")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        installed.contains("does not support release installation"),
        "{installed}"
    );
    let pinned = tool
        .install_release_from_path("game", config, "v1", "a.zip", PathBuf::from("a.zip"))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        pinned.contains("does not support release pinning"),
        "{pinned}"
    );
}

#[test]
fn default_revert_removes_only_existing_applied_files() {
    let game = tempfile::tempdir().unwrap();
    std::fs::write(game.path().join("dxgi.dll"), b"x").unwrap();
    let applied = AppliedFiles {
        files: vec![PathBuf::from("dxgi.dll"), PathBuf::from("already-gone.ini")],
    };

    Minimal.revert(game.path(), &applied).unwrap();
    assert!(!game.path().join("dxgi.dll").exists());
}
