#![cfg(feature = "bethesda")]

use modde_games::bethesda::fomod::*;

#[test]
fn parse_fomod_info() {
    let xml = include_str!("fixtures/info.xml");
    let info = FomodInfo::parse(xml).unwrap();

    assert_eq!(info.name.as_deref(), Some("Test Mod"));
    assert_eq!(info.author.as_deref(), Some("Test Author"));
    assert_eq!(info.version.as_deref(), Some("1.0.0"));
    assert_eq!(info.website.as_deref(), Some("https://example.com"));
    assert_eq!(info.id.as_deref(), Some("12345"));
}

#[test]
fn parse_module_config() {
    let xml = include_str!("fixtures/simple_config.xml");
    let config = ModuleConfig::parse(xml).unwrap();

    assert_eq!(config.module_name.value, "Test Mod Installer");
    assert_eq!(config.module_name.position, Some(NamePosition::Left));
    assert!(config.module_image.is_some());
    assert!(config.required_install_files.is_some());

    let steps = config.install_steps.as_ref().unwrap();
    assert_eq!(steps.steps.len(), 2);
    assert_eq!(steps.steps[0].name, "Choose Textures");
    assert_eq!(steps.steps[1].name, "Optional Patches");

    // First step has one group with SelectExactlyOne
    let groups = &steps.steps[0].optional_file_groups.as_ref().unwrap().groups;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].group_type, GroupType::SelectExactlyOne);
    assert_eq!(groups[0].plugins.plugins.len(), 2);
}

#[test]
fn installer_default_selections() {
    let xml = include_str!("fixtures/simple_config.xml");
    let config = ModuleConfig::parse(xml).unwrap();

    let steps = config.install_steps.as_ref().unwrap();
    let group = &steps.steps[0].optional_file_groups.as_ref().unwrap().groups[0];

    let defaults = Installer::default_selections(group);
    // High Resolution is Recommended, should be default
    assert_eq!(defaults, vec![0]);
}

#[test]
fn installer_step_visibility() {
    let xml = include_str!("fixtures/simple_config.xml");
    let config = ModuleConfig::parse(xml).unwrap();

    // Without flags, step 1 ("Optional Patches") should be hidden
    let installer = Installer::new(config.clone());
    let visible = installer.visible_steps();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].1.name, "Choose Textures");

    // With texture_quality=high, both steps should be visible
    let mut installer = Installer::new(config);
    installer.context_mut().set_flag("texture_quality", "high");
    let visible = installer.visible_steps();
    assert_eq!(visible.len(), 2);
}

#[test]
fn installer_resolve_with_required_files() {
    let xml = include_str!("fixtures/simple_config.xml");
    let config = ModuleConfig::parse(xml).unwrap();
    let installer = Installer::new(config);

    let plan = installer.resolve();
    // Should have required files even with no selections
    assert!(plan.operations.len() >= 2);

    let sources: Vec<&str> = plan
        .operations
        .iter()
        .map(|op| op.source.as_str())
        .collect();
    assert!(sources.contains(&"readme.txt"));
    assert!(sources.contains(&"core_files"));
}

#[test]
fn installer_resolve_full_flow() {
    let xml = include_str!("fixtures/simple_config.xml");
    let config = ModuleConfig::parse(xml).unwrap();
    let mut installer = Installer::new(config);

    // Select High Resolution textures (step 0, group 0, plugin 0)
    installer.select(0, 0, vec![0]);

    // Verify flag was set
    assert_eq!(
        installer.context().flags.get("texture_quality"),
        Some(&"high".to_string())
    );

    // Now step 1 should be visible
    let visible = installer.visible_steps();
    assert_eq!(visible.len(), 2);

    let plan = installer.resolve();
    let sources: Vec<&str> = plan
        .operations
        .iter()
        .map(|op| op.source.as_str())
        .collect();

    // Should include required files
    assert!(sources.contains(&"readme.txt"));
    // Should include high res textures
    assert!(sources.contains(&"textures/high"));
    // Should include conditional LOD file (texture_quality=high)
    assert!(sources.contains(&"lods/high_lod.esp"));
}

#[test]
fn installer_resolve_standard_textures() {
    let xml = include_str!("fixtures/simple_config.xml");
    let config = ModuleConfig::parse(xml).unwrap();
    let mut installer = Installer::new(config);

    // Select Standard Resolution (step 0, group 0, plugin 1)
    installer.select(0, 0, vec![1]);

    assert_eq!(
        installer.context().flags.get("texture_quality"),
        Some(&"standard".to_string())
    );

    let plan = installer.resolve();
    let sources: Vec<&str> = plan
        .operations
        .iter()
        .map(|op| op.source.as_str())
        .collect();

    assert!(sources.contains(&"textures/standard"));
    // Should NOT include high LOD file
    assert!(!sources.contains(&"lods/high_lod.esp"));
}

#[test]
fn validate_selection_constraints() {
    let xml = include_str!("fixtures/simple_config.xml");
    let config = ModuleConfig::parse(xml).unwrap();

    let group = &config.install_steps.as_ref().unwrap().steps[0]
        .optional_file_groups
        .as_ref()
        .unwrap()
        .groups[0];

    // SelectExactlyOne: exactly 1 is valid
    assert!(Installer::validate_selection(group, &[0]).is_ok());
    // SelectExactlyOne: 0 is invalid
    assert!(Installer::validate_selection(group, &[]).is_err());
    // SelectExactlyOne: 2 is invalid
    assert!(Installer::validate_selection(group, &[0, 1]).is_err());
    // Out of bounds
    assert!(Installer::validate_selection(group, &[99]).is_err());
}

#[test]
fn condition_flag_evaluation() {
    let mut ctx = EvalContext::new();
    ctx.set_flag("test_flag", "value1");

    let dep = FlagDependency {
        flag: "test_flag".to_string(),
        value: "value1".to_string(),
    };

    let composite = CompositeDependency {
        operator: Operator::And,
        file_deps: vec![],
        flag_deps: vec![dep],
        game_deps: vec![],
        fomm_deps: vec![],
        nested: vec![],
    };

    assert!(composite.evaluate(&ctx));

    // Wrong value
    ctx.set_flag("test_flag", "other");
    assert!(!composite.evaluate(&ctx));
}
