//! The API compared with its snapshots in `api_snapshots/` (`just api-check`): fails when the snapshot of the
//! current version is missing or differs from the source, and when it changed without the version bump the change
//! needs. `just api-save` (`WW_API_SAVE=1 cargo test -p cannify_micro_api --test evolution`) saves the current version's
//! snapshot. WireWeaver's docs/evolution/checker_tool.md explains the check, docs/evolution/rules.md the rules.

#[test]
fn api_evolution() {
    wire_weaver_evolution::Check::new(env!("CARGO_MANIFEST_DIR"))
        .require_current(true)
        .assert();
}
