//! The decision: Trash, permanent, or refused — and why.
//!
//! This is where a caller's `Justification` is re-checked rather than
//! believed. Every arm either proves the claim against the path itself
//! or refuses it with a reason the user can act on.

use crate::catalog::{self, Disposition};
use crate::paths::normalize;
use std::path::Path;
use super::{Evidence, Justification, Roots};
use super::identity::*;
use super::roots::*;

pub(crate) fn disposition_for(path: &Path, j: &Justification, roots: &Roots) -> Result<Disposition, String> {
    match j {
        Justification::Catalog(id) => match catalog::find(id) {
            Some(entry) => match normalize(path) {
                Some(normalized) if is_within_catalog_entry(&normalized, entry, roots) => {
                    Ok(entry.disposition)
                }
                _ => {
                    // Name the category and the root, and say what is
                    // actually wrong. "Not covered by this category" would be
                    // true but useless to someone who moved the cache on
                    // purpose and is wondering why nothing happened.
                    let relocated = relocated_roots(entry, &roots.home);
                    if relocated.is_empty() {
                        Err(format!(
                            "{} is not covered by the \"{id}\" category. Nothing was removed.",
                            path.display()
                        ))
                    } else {
                        Err(format!(
                            "The \"{}\" category was skipped because it no longer resolves where Spiral Clean expects it: {}. Spiral Clean only removes a category from the exact location it declares, so nothing was removed.",
                            entry.label,
                            relocated.join("; ")
                        ))
                    }
                }
            },
            None => Err(format!(
                "\"{id}\" is not a category in this release. Nothing was removed."
            )),
        },
        // ADR-0008's deliberate second step. Trash, never permanent: a plist
        // is the only copy of a job definition, nothing regenerates it, and
        // ADR-0001 reserves permanent deletion for a catalog match.
        Justification::StartupItem => match (normalize(path), roots.startup_agents.as_deref()) {
            (Some(normalized), Some(agents)) if is_user_launch_agent(&normalized, agents) => {
                Ok(Disposition::Trash)
            }
            (_, None) => Err(
                "Your LaunchAgents folder does not resolve where macOS keeps it, so Spiral Clean will not remove anything from it. Nothing was removed."
                    .to_string(),
            ),
            (_, Some(_)) => Err(format!(
                "{} is not a login item file in your LaunchAgents folder, so Spiral Clean will not remove it. Nothing was removed.",
                path.display()
            )),
        },
        // ADR-0011, satisfied — see
        // docs/adr/0011-associate-gates-the-first-appbundle-producer.md. The
        // location bar runs first and unconditionally, exactly as it always
        // has. What is new is that `bundle_id` is no longer ignored:
        // `Evidence::Verified` is re-checked against the path itself before
        // `Permanent` is granted, so a caller cannot merely assert
        // provenance — it must be present in the name. `Evidence::Likely`
        // cannot be checked this way (a name match has no bundle id to
        // compare), so it is routed to `Trash` instead, per ADR-0004 as
        // amended.
        Justification::AppBundle { bundle_id, evidence } => {
            // First, ahead of scope and of either evidence level: Apple's own
            // software is never an uninstall target here, however convincing
            // the evidence for it looks. See `is_apple_bundle_id`.
            if is_apple_bundle_id(bundle_id) {
                return Err(format!(
                    "\"{bundle_id}\" is one of Apple's own bundle identifiers, and Spiral Clean never removes Apple software or the data belonging to it — a third-party bundle can claim any identifier it likes. Nothing was removed. Remove Apple software through the App Store or System Settings instead.",
                ));
            }
            if !is_within_app_bundle_scope(path, roots) {
                return Err(format!(
                    "{} is outside the locations an app uninstall may touch. Only the app bundle and its own support files can be removed.",
                    path.display()
                ));
            }
            match evidence {
                Evidence::Likely => Ok(Disposition::Trash),
                Evidence::Verified => {
                    if bundle_id.is_empty() {
                        return Err(format!(
                            "{} was claimed as a verified app-bundle item with an empty bundle id, which cannot be checked against anything. Nothing was removed.",
                            path.display()
                        ));
                    }
                    // Two shapes of evidence, both re-checked here rather
                    // than trusted: the path's own name carries the id, or
                    // the path *is* an app bundle whose `Info.plist` declares
                    // it. Neither is anything the caller says.
                    let carries_bundle_id = normalize(path)
                        .and_then(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_owned))
                        .is_some_and(|name| verified_name_matches(&name, bundle_id));
                    if carries_bundle_id || bundle_declares_id(path, bundle_id) {
                        Ok(Disposition::Permanent)
                    } else {
                        Err(format!(
                            "{} neither carries the bundle id \"{bundle_id}\" in its name nor declares it as its own CFBundleIdentifier. A verified app-bundle claim must be provable from the path itself, so nothing was removed.",
                            path.display()
                        ))
                    }
                }
            }
        }
        // Unreachable until this milestone's orphan-detection producer
        // lands (see the module doc), but the check goes in ahead of that
        // producer rather than after it — an `Orphan` claim is a judgement,
        // not a proof (ADR-0007), and re-checking it here is the same
        // discipline `Evidence::Verified` already applies just above: the
        // path's own name must carry the claimed `bundle_id` at a component
        // boundary, via the same `verified_name_matches` used there, not a
        // second spelling of the same rule. Disposition stays `Trash`
        // either way — this only decides whether the claim is honoured at
        // all.
        Justification::Orphan { bundle_id } => {
            let carries_bundle_id = normalize(path)
                .and_then(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_owned))
                .is_some_and(|name| verified_name_matches(&name, bundle_id));
            if carries_bundle_id {
                Ok(Disposition::Trash)
            } else {
                Err(format!(
                    "{} does not carry the bundle id \"{bundle_id}\" in its name, so the claim that it belongs to that app cannot be verified. Nothing was removed.",
                    path.display()
                ))
            }
        }
        // Authorised by location, never by the caller's say-so.
        Justification::DeviceBackup => match (normalize(path), roots.device_backups.as_deref()) {
            (Some(normalized), Some(backups)) if is_device_backup(&normalized, backups) => {
                Ok(Disposition::Trash)
            }
            (_, None) => Err(
                "Your iOS backup folder does not resolve where macOS keeps it, so Spiral Clean will not remove anything from it. Nothing was removed."
                    .to_string(),
            ),
            (_, Some(_)) => Err(format!(
                "{} is not a device backup in your MobileSync folder, so Spiral Clean will not remove it. Nothing was removed.",
                path.display()
            )),
        },
    }
}
