//! What a project and the engine declare: the YAML subset and its edits,
//! `exuno.yaml` and its profiles, the shipped catalog and the layered
//! tool config, and the revisions and hashes that tell an update from drift.

pub mod catalog;
pub mod edit_paths;
pub mod format_rev;
pub mod leftovers;
pub mod mcp_catalog;
pub mod names;
pub mod payload;
pub mod profiles;
pub mod project_config;
pub mod skill_cards;
pub mod skill_metadata;
pub mod skill_source;
pub mod snapshot;
pub mod template_manifest;
pub mod tool;
pub mod version;
pub mod yaml_edit;
pub mod yaml_subset;
