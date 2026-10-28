//! Stamps `--version` with the git commit and the build time, without extra crates:
//! `CANNIFY_GIT` is the short SHA with `-dirty` for uncommitted changes (`unknown` without git), `CANNIFY_BUILT`
//! the build time in UTC (`SOURCE_DATE_EPOCH` when set, for reproducible builds).

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Rebuild when the checkout moves: HEAD, the branch it points to and the index.
fn rerun_on_git_changes() {
    let mut paths = vec!["HEAD".to_string(), "index".to_string()];
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        paths.push(branch);
    }
    for p in paths {
        if let Some(path) = git(&["rev-parse", "--git-path", &p]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn describe() -> String {
    let Some(sha) = git(&["rev-parse", "--short", "HEAD"]) else {
        return "unknown".into();
    };
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    if dirty { format!("{sha}-dirty") } else { sha }
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian (Howard Hinnant's civil_from_days).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn built(secs: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (year, month, day) = civil(secs.div_euclid(86_400));
    let in_day = secs.rem_euclid(86_400);
    format!(
        "{day} {} {year} {:02}:{:02} UTC",
        MONTHS[month as usize - 1],
        in_day / 3600,
        in_day % 3600 / 60
    )
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    rerun_on_git_changes();
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64)
        });
    println!("cargo:rustc-env=CANNIFY_GIT={}", describe());
    println!("cargo:rustc-env=CANNIFY_BUILT={}", built(secs));
}
