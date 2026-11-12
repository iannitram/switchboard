//! SKY-W: the `SkyModel` the Sky view paints, assembled from the snapshot store and the priority
//! copy in the app data folder. Every field is worked out by the sky modules; this file only
//! joins them. The serialised names are the face's (app/src/sky/model.ts).

use std::path::Path;

use serde::Serialize;

use crate::clock::UnixSeconds;
use crate::github::snapshot::{self, SnapshotStore};
use crate::github::{Issue, RepoSnapshot};
use crate::sky::chain;
use crate::sky::codemap;
use crate::sky::drift::{self, Drift};
use crate::sky::features::{self, Feature, FeatureBlock, Release};
use crate::sky::jobs;
use crate::sky::layout::{self, Placement};
use crate::sky::priority::{self, PriorityCopy};
use crate::CoreError;

/// The priority copy's file in the app data folder (SKY-9; U4 moves it to fleet-state later).
pub const PRIORITY_FILE: &str = "priority.json";

/// A fleet job in flight on one issue (SKY-4, SKY-8). Input only, not sent to the face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarJob {
    pub repo: String,
    pub number: u64,
    pub job: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkyModel {
    pub age_minutes: i64,
    pub priority_age_days: Option<i64>,
    pub constellations: Vec<Constellation>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Constellation {
    pub repo: String,
    pub url: String,
    pub rank: usize,
    pub radius: f64,
    pub size: f64,
    pub angle: f64,
    pub brightness: f64,
    pub features: Vec<SkyFeature>,
    /// [blocker, blocked]
    pub edges: Vec<[u64; 2]>,
    pub chain: Vec<u64>,
    pub drift: Vec<DriftItem>,
    pub reuses: Vec<ReuseLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkyFeature {
    pub id: String,
    pub title: String,
    pub release: String,
    pub stars: Vec<Star>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Star {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub url: String,
    pub labels: Vec<String>,
    pub size: Option<String>,
    pub milestone: Option<String>,
    pub in_flight: bool,
    pub pr: Option<StarPr>,
    pub job: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StarPr {
    pub number: u64,
    pub url: String,
    pub state: String,
    pub verdict: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DriftItem {
    pub kind: String,
    pub text: String,
    pub link: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReuseLine {
    pub repo: String,
    pub what: String,
    pub warning: bool,
}

/// The sky for the snapshots, in the priority copy's order (then the rest by name). No snapshot
/// is `GitHub: no snapshot yet`; a bad ROADMAP.md is a parse error naming its repo.
pub fn build(
    snaps: &[RepoSnapshot],
    priority: Option<&PriorityCopy>,
    jobs: &[StarJob],
    now: UnixSeconds,
) -> Result<SkyModel, CoreError> {
    if snaps.is_empty() {
        return Err(CoreError::GitHub("no snapshot yet".to_string()));
    }

    let mut ordered: Vec<&RepoSnapshot> = Vec::new();
    if let Some(copy) = priority {
        for repo in &copy.order {
            if ordered.iter().any(|s| &s.repo == repo) {
                continue;
            }
            if let Some(snap) = snaps.iter().find(|s| &s.repo == repo) {
                ordered.push(snap);
            }
        }
    }
    let mut rest: Vec<&RepoSnapshot> = snaps
        .iter()
        .filter(|s| !ordered.iter().any(|o| o.repo == s.repo))
        .collect();
    rest.sort_by(|a, b| a.repo.cmp(&b.repo));
    ordered.extend(rest);

    let names: Vec<String> = ordered.iter().map(|s| s.repo.clone()).collect();
    let today = utc_date(now);
    let constellations = ordered
        .iter()
        .zip(layout::place(&names))
        .map(|(snap, placement)| constellation(snaps, snap, &placement, jobs, &today))
        .collect::<Result<Vec<_>, _>>()?;

    let age_minutes = snaps
        .iter()
        .map(|snap| snapshot::age_minutes(snap, now))
        .max()
        .unwrap_or(0);
    let priority_age_days = priority.map(|copy| priority::age_days(copy, now));

    Ok(SkyModel {
        age_minutes,
        priority_age_days,
        constellations,
    })
}

/// The snapshot store and the priority copy in the app data folder, then `build`, with no job
/// in flight.
pub fn load(data_dir: &Path, now: UnixSeconds) -> Result<SkyModel, CoreError> {
    load_with_ledger(data_dir, &[], now)
}

/// `load`, with the jobs in flight read from the fleet ledger's `events` (`jobs::in_flight`),
/// the same jobs `star` names (SKY-4, #343).
pub fn load_with_ledger(
    data_dir: &Path,
    events: &[serde_json::Value],
    now: UnixSeconds,
) -> Result<SkyModel, CoreError> {
    let snaps = SnapshotStore::new(data_dir).load_all()?;
    let path = data_dir.join(PRIORITY_FILE);
    let copy = match std::fs::read_to_string(&path) {
        Ok(text) => Some(priority::parse(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(CoreError::Io(format!("{}: {e}", path.display()))),
    };
    let repos: Vec<String> = snaps.iter().map(|snap| snap.repo.clone()).collect();
    let jobs = jobs::in_flight(events, &repos);
    build(&snaps, copy.as_ref(), &jobs, now)
}

fn constellation(
    snaps: &[RepoSnapshot],
    snap: &RepoSnapshot,
    placement: &Placement,
    jobs: &[StarJob],
    today: &str,
) -> Result<Constellation, CoreError> {
    let block = match &snap.roadmap_md {
        None => FeatureBlock {
            updated: None,
            features: Vec::new(),
        },
        Some(text) => features::parse(text).map_err(|e| match e {
            CoreError::Parse(msg) => CoreError::Parse(format!("{}: {msg}", snap.repo)),
            other => other,
        })?,
    };

    Ok(Constellation {
        repo: snap.repo.clone(),
        url: format!("https://github.com/{}", snap.repo),
        rank: placement.rank,
        radius: placement.radius,
        size: placement.size,
        angle: placement.angle,
        brightness: layout::brightness(&snap.issues, &block),
        features: block
            .features
            .iter()
            .map(|feature| sky_feature(snap, feature, jobs))
            .collect(),
        edges: chain::edges(&snap.issues)
            .into_iter()
            .map(|(blocked, blocker)| [blocker, blocked])
            .collect(),
        chain: chain::longest(&snap.issues),
        drift: drift::find(&snap.issues, &block, today)
            .iter()
            .map(|item| drift_item(snap, &block, item))
            .collect(),
        reuses: reuse_lines(snaps, snap),
    })
}

fn sky_feature(snap: &RepoSnapshot, feature: &Feature, jobs: &[StarJob]) -> SkyFeature {
    let label = format!("feature:{}", feature.id);
    let release = match feature.release {
        Release::V1 => "1.0",
        Release::After1 => "after-1.0",
        Release::After2 => "after-2.0",
    };
    SkyFeature {
        id: feature.id.clone(),
        title: feature.title.clone(),
        release: release.to_string(),
        stars: snap
            .issues
            .iter()
            .filter(|issue| issue.labels.contains(&label))
            .map(|issue| sky_star(snap, issue, jobs))
            .collect(),
    }
}

fn sky_star(snap: &RepoSnapshot, issue: &Issue, jobs: &[StarJob]) -> Star {
    let job = jobs
        .iter()
        .find(|job| job.repo == snap.repo && job.number == issue.number);
    let pr = snap
        .pulls
        .iter()
        .find(|pr| pr.closes.contains(&issue.number))
        .map(|pr| StarPr {
            number: pr.number,
            url: format!("https://github.com/{}/pull/{}", snap.repo, pr.number),
            state: pr.state.clone(),
            verdict: pr
                .verdicts
                .last()
                .filter(|v| matches!(v.verdict.as_str(), "PASS" | "FAIL"))
                .map(|v| v.verdict.clone()),
        });
    Star {
        number: issue.number,
        title: issue.title.clone(),
        state: issue.state.clone(),
        url: format!("https://github.com/{}/issues/{}", snap.repo, issue.number),
        labels: issue.labels.clone(),
        size: issue
            .labels
            .iter()
            .find_map(|label| label.strip_prefix("size:"))
            .map(str::to_string),
        milestone: issue.milestone.clone(),
        in_flight: job.is_some(),
        pr,
        job: job.map(|job| job.job.clone()),
    }
}

/// One drift item with the link GitHub serves for it. An orphan links its issue. A dark feature
/// and a stale roadmap link the ROADMAP.md on the default branch, but only when the snapshot has
/// one; without it the link is the repo page, and the stale item says so (`no ROADMAP.md`).
fn drift_item(snap: &RepoSnapshot, block: &FeatureBlock, item: &Drift) -> DriftItem {
    let roadmap = match snap.roadmap_md {
        Some(_) => format!("https://github.com/{}/blob/HEAD/ROADMAP.md", snap.repo),
        None => format!("https://github.com/{}", snap.repo),
    };
    match item {
        Drift::Orphan { number, features } => {
            let title = snap
                .issues
                .iter()
                .find(|issue| issue.number == *number)
                .map_or("", |issue| issue.title.as_str());
            let text = if features.is_empty() {
                format!("#{number} {title}: on no feature")
            } else {
                let labels: Vec<String> =
                    features.iter().map(|id| format!("feature:{id}")).collect();
                format!("#{number} {title}: on {}", labels.join(", "))
            };
            DriftItem {
                kind: "orphan".to_string(),
                text,
                link: format!("https://github.com/{}/issues/{number}", snap.repo),
            }
        }
        Drift::Dark { feature } => {
            let title = block
                .features
                .iter()
                .find(|f| f.id == *feature)
                .map_or("", |f| f.title.as_str());
            DriftItem {
                kind: "dark".to_string(),
                text: format!("{feature}: {title} has no issue"),
                link: roadmap,
            }
        }
        Drift::Stale { updated, age_days } => {
            let text = match (updated, age_days) {
                (Some(u), Some(d)) => format!("ROADMAP.md updated {u}, {d} days ago"),
                (Some(u), None) => format!("ROADMAP.md Updated: {u} is not a date"),
                (None, _) if snap.roadmap_md.is_none() => "no ROADMAP.md".to_string(),
                (None, _) => "ROADMAP.md has no Updated: line".to_string(),
            };
            DriftItem {
                kind: "stale".to_string(),
                text,
                link: roadmap,
            }
        }
    }
}

fn reuse_lines(snaps: &[RepoSnapshot], snap: &RepoSnapshot) -> Vec<ReuseLine> {
    let Some(text) = &snap.code_map_md else {
        return Vec::new();
    };
    codemap::reuses(text)
        .into_iter()
        .map(|reuse| {
            let archived = archived_of(snaps, &reuse.repo);
            ReuseLine {
                warning: codemap::is_warning(archived, None),
                repo: reuse.repo,
                what: reuse.what,
            }
        })
        .collect()
}

/// Whether the snapshot named `name` (full, or the short name after a `/`) is archived; false
/// when no snapshot matches.
fn archived_of(snaps: &[RepoSnapshot], name: &str) -> bool {
    let suffix = format!("/{name}");
    snaps
        .iter()
        .find(|s| s.repo == name)
        .or_else(|| snaps.iter().find(|s| s.repo.ends_with(suffix.as_str())))
        .is_some_and(|s| s.archived)
}

/// The UTC date of `now` as `YYYY-MM-DD` (Howard Hinnant's civil_from_days).
fn utc_date(now: UnixSeconds) -> String {
    let z = now.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}
