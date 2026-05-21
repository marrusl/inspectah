//! Variant operations applied during snapshot projection.
//!
//! These functions mutate a cloned `InspectionSnapshot` in place as part of
//! the `project_snapshot()` replay loop. They handle SelectVariant,
//! EditVariant, and DiscardVariant ops for config-file variants.
//!
//! The `user_variants` map tracks user-created content (from EditVariant)
//! so that DiscardVariant can distinguish user-created from host-sourced
//! variants. It is built up during the projection replay and is NOT
//! persisted -- it is derived state.

use std::collections::{HashMap, HashSet};

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigFileEntry;
use inspectah_core::types::fleet::VariantSelection;

use crate::types::{ContentHash, ItemId, RefineError};

/// Tracks user-created variants and discarded variant hashes during projection.
#[derive(Debug, Default)]
pub struct VariantProjectionState {
    /// user_variants[path][content_hash] = content_string
    pub user_variants: HashMap<String, HashMap<ContentHash, String>>,
    /// discarded[path] = set of discarded content hashes
    pub discarded: HashMap<String, HashSet<ContentHash>>,
    /// selected[path] = content hash of the selected variant (if overridden)
    pub selected: HashMap<String, ContentHash>,
}

/// Extract the config path from an ItemId, if it is a Config variant.
fn config_path(item_id: &ItemId) -> Option<&str> {
    match item_id {
        ItemId::Config { path } => Some(path.as_str()),
        _ => None,
    }
}

/// Apply a SelectVariant op to the projection state.
///
/// Records which variant hash should be Selected for the given path.
pub fn apply_select(
    state: &mut VariantProjectionState,
    item_id: &ItemId,
    target: &ContentHash,
) {
    if let Some(path) = config_path(item_id) {
        state.selected.insert(path.to_string(), target.clone());
    }
}

/// Apply an EditVariant op to the projection state.
///
/// If the content hash matches an existing variant (host-sourced or
/// previously user-created), this converges by selecting it instead of
/// creating a duplicate. Otherwise, adds a new user variant and selects it.
pub fn apply_edit(
    state: &mut VariantProjectionState,
    item_id: &ItemId,
    content: &str,
    snap: &InspectionSnapshot,
) {
    let Some(path) = config_path(item_id) else {
        return;
    };

    let new_hash = ContentHash::from_content(content.as_bytes());

    // Check if this content already exists as a host-sourced variant
    let existing_host = snap
        .config
        .as_ref()
        .map(|c| {
            c.files
                .iter()
                .any(|e| e.path == path && ContentHash::from_content(e.content.as_bytes()) == new_hash)
        })
        .unwrap_or(false);

    // Check if it already exists as a user variant
    let existing_user = state
        .user_variants
        .get(path)
        .map(|m| m.contains_key(&new_hash))
        .unwrap_or(false);

    if existing_host || existing_user {
        // Convergence: select the existing variant instead of duplicating
        state.selected.insert(path.to_string(), new_hash);
    } else {
        // New user variant: add to user_variants and select it
        state
            .user_variants
            .entry(path.to_string())
            .or_default()
            .insert(new_hash.clone(), content.to_string());
        // Remove from discarded if it was previously discarded
        if let Some(disc) = state.discarded.get_mut(path) {
            disc.remove(&new_hash);
        }
        state.selected.insert(path.to_string(), new_hash);
    }
}

/// Apply a DiscardVariant op to the projection state.
///
/// Only user-created variants can be discarded. Host-sourced variants
/// cannot be discarded (the caller should validate this before calling).
pub fn apply_discard(
    state: &mut VariantProjectionState,
    item_id: &ItemId,
    variant: &ContentHash,
) {
    let Some(path) = config_path(item_id) else {
        return;
    };

    // Remove from user_variants
    if let Some(user_map) = state.user_variants.get_mut(path) {
        user_map.remove(variant);
    }

    // Record as discarded
    state
        .discarded
        .entry(path.to_string())
        .or_default()
        .insert(variant.clone());

    // If the discarded variant was the selected one, clear the selection
    // so that materialize_variants will apply fallback logic
    if state.selected.get(path) == Some(variant) {
        state.selected.remove(path);
    }
}

/// Validate that a SelectVariant op targets a valid item and hash.
///
/// The target hash must match a config file entry in the snapshot or
/// a user variant in the projection state.
pub fn validate_select(
    snap: &InspectionSnapshot,
    state: &VariantProjectionState,
    item_id: &ItemId,
    target: &ContentHash,
) -> Result<(), RefineError> {
    let Some(path) = config_path(item_id) else {
        return Err(RefineError::BadRequest(
            "SelectVariant only supported for Config items".into(),
        ));
    };

    // Check path exists in snapshot
    let path_exists = snap
        .config
        .as_ref()
        .map(|c| c.files.iter().any(|e| e.path == path))
        .unwrap_or(false);
    if !path_exists {
        return Err(RefineError::UnknownTarget(path.to_string()));
    }

    // Check target hash exists (host-sourced or user-created)
    let hash_in_snap = snap
        .config
        .as_ref()
        .map(|c| {
            c.files.iter().any(|e| {
                e.path == path && ContentHash::from_content(e.content.as_bytes()) == *target
            })
        })
        .unwrap_or(false);

    let hash_in_user = state
        .user_variants
        .get(path)
        .map(|m| m.contains_key(target))
        .unwrap_or(false);

    // Also check it's not discarded
    let is_discarded = state
        .discarded
        .get(path)
        .map(|d| d.contains(target))
        .unwrap_or(false);

    if (!hash_in_snap && !hash_in_user) || is_discarded {
        return Err(RefineError::UnknownTarget(format!(
            "variant hash {} not found for path {}",
            target.as_str(),
            path
        )));
    }

    Ok(())
}

/// Validate that a DiscardVariant op targets a user-created variant.
pub fn validate_discard(
    snap: &InspectionSnapshot,
    state: &VariantProjectionState,
    item_id: &ItemId,
    variant: &ContentHash,
) -> Result<(), RefineError> {
    let Some(path) = config_path(item_id) else {
        return Err(RefineError::BadRequest(
            "DiscardVariant only supported for Config items".into(),
        ));
    };

    // Check if it's a user-created variant
    let is_user = state
        .user_variants
        .get(path)
        .map(|m| m.contains_key(variant))
        .unwrap_or(false);

    if is_user {
        return Ok(());
    }

    // Check if it exists as a host-sourced variant
    let is_host = snap
        .config
        .as_ref()
        .map(|c| {
            c.files.iter().any(|e| {
                e.path == path && ContentHash::from_content(e.content.as_bytes()) == *variant
            })
        })
        .unwrap_or(false);

    if is_host {
        return Err(RefineError::BadRequest(format!(
            "cannot discard host-sourced variant {} for path {}",
            variant.as_str(),
            path
        )));
    }

    Err(RefineError::UnknownTarget(format!(
        "variant hash {} not found for path {}",
        variant.as_str(),
        path
    )))
}

/// Materialize the projection state into the snapshot's config section.
///
/// This is called after all ops have been replayed. It:
/// 1. Adds user-created variants as new ConfigFileEntry items
/// 2. Removes discarded variants
/// 3. Applies variant selection overrides
/// 4. Derives VariantSelection (Only when single, Selected/Alternative when multiple)
pub fn materialize_variants(snap: &mut InspectionSnapshot, state: &VariantProjectionState) {
    let Some(ref mut config) = snap.config else {
        return;
    };

    // Collect all paths that have any variant state
    let mut affected_paths: HashSet<String> = HashSet::new();
    affected_paths.extend(state.user_variants.keys().cloned());
    affected_paths.extend(state.discarded.keys().cloned());
    affected_paths.extend(state.selected.keys().cloned());

    if affected_paths.is_empty() {
        return;
    }

    // Add user-created variants as new entries
    for (path, user_map) in &state.user_variants {
        // Find a template entry for this path to clone metadata from
        let template = config.files.iter().find(|e| e.path == *path).cloned();
        for (hash, content) in user_map {
            // Skip if this hash is discarded
            if state
                .discarded
                .get(path)
                .map(|d| d.contains(hash))
                .unwrap_or(false)
            {
                continue;
            }
            // Don't add if it already exists in files (shouldn't happen, but safety)
            let already_exists = config.files.iter().any(|e| {
                e.path == *path && ContentHash::from_content(e.content.as_bytes()) == *hash
            });
            if already_exists {
                continue;
            }
            let mut entry = template.clone().unwrap_or_else(|| ConfigFileEntry {
                path: path.clone(),
                include: true,
                ..Default::default()
            });
            entry.content = content.clone();
            entry.fleet = None; // user-created, no fleet provenance
            entry.variant_selection = VariantSelection::Alternative; // will be fixed below
            config.files.push(entry);
        }
    }

    // Remove discarded variants
    for (path, disc_set) in &state.discarded {
        config.files.retain(|e| {
            if e.path != *path {
                return true;
            }
            let hash = ContentHash::from_content(e.content.as_bytes());
            !disc_set.contains(&hash)
        });
    }

    // Apply selection and derive VariantSelection for all affected paths
    for path in &affected_paths {
        let variants: Vec<usize> = config
            .files
            .iter()
            .enumerate()
            .filter(|(_, e)| e.path == *path)
            .map(|(i, _)| i)
            .collect();

        if variants.is_empty() {
            continue;
        }

        if variants.len() == 1 {
            // Single variant: always Only
            config.files[variants[0]].variant_selection = VariantSelection::Only;
            continue;
        }

        // Multiple variants: determine which is Selected
        if let Some(selected_hash) = state.selected.get(path) {
            // User explicitly selected one
            for &idx in &variants {
                let entry_hash =
                    ContentHash::from_content(config.files[idx].content.as_bytes());
                if entry_hash == *selected_hash {
                    config.files[idx].variant_selection = VariantSelection::Selected;
                } else {
                    config.files[idx].variant_selection = VariantSelection::Alternative;
                }
            }
        }
        // If no explicit selection, leave the original Selected/Alternative as-is
        // (the snapshot already has this from the merger)
    }
}
