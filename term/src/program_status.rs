//! LBTerm: what the programs in a terminal say they're doing, from OSC 7501
//! reports (`wezterm_escape_parser::program_status`), kept as the spec says:
//! each report replaces its record, `clear` removes a record and everything
//! under it, a new prompt (OSC 133 A) ends anything working or blocked, a
//! full reset ends everything, and at most 256 records are kept, the least
//! recently updated going first.

use std::time::SystemTime;
pub use wezterm_escape_parser::program_status::{
    BlockedKind, ProgramState, ProgramStatus, ProgramStatusReport,
};

const MAX_RECORDS: usize = 256;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "use_serde", derive(serde::Deserialize, serde::Serialize))]
pub struct ProgramStatusRecord {
    /// Empty for the root record, else a `/`-separated path
    pub id: String,
    /// Never `Clear`
    pub state: ProgramState,
    pub kind: Option<BlockedKind>,
    pub progress: Option<u8>,
    /// Its own, or the nearest ancestor's
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
    /// When its report arrived
    pub updated: SystemTime,
}

#[derive(Debug, Default, Clone)]
pub struct ProgramStatusRecords {
    /// With the order each was last updated in, for eviction
    records: Vec<(ProgramStatusRecord, u64)>,
    next: u64,
}

fn under(id: &str, ancestor: &str) -> bool {
    ancestor.is_empty() || id == ancestor || id.starts_with(&format!("{ancestor}/"))
}

impl ProgramStatusRecords {
    /// Apply a report. True if anything changed.
    pub fn apply(&mut self, report: ProgramStatusReport, now: SystemTime) -> bool {
        let id = report.id.unwrap_or_default();
        if report.state == ProgramState::Clear {
            let before = self.records.len();
            self.records.retain(|(r, _)| !under(&r.id, &id));
            return self.records.len() != before;
        }
        let record = ProgramStatusRecord {
            id,
            state: report.state,
            kind: report.kind,
            progress: report.progress,
            app: report.app,
            title: report.title,
            msg: report.msg,
            updated: now,
        };
        self.next += 1;
        match self.records.iter_mut().find(|(r, _)| r.id == record.id) {
            Some(slot) => *slot = (record, self.next),
            None => {
                if self.records.len() >= MAX_RECORDS {
                    if let Some(oldest) = self
                        .records
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, (_, seq))| *seq)
                        .map(|(i, _)| i)
                    {
                        self.records.remove(oldest);
                    }
                }
                self.records.push((record, self.next));
            }
        }
        true
    }

    /// A new prompt, or the program gone: nothing is working or blocked
    /// any more. Done and error stay. True if anything changed.
    pub fn end_activity(&mut self) -> bool {
        let before = self.records.len();
        self.records.retain(|(r, _)| {
            !matches!(r.state, ProgramState::Working | ProgramState::Blocked)
        });
        self.records.len() != before
    }

    /// A full reset (RIS) removes every record.
    pub fn reset(&mut self) -> bool {
        let had = !self.records.is_empty();
        self.records.clear();
        had
    }

    /// Every record, root first then by id, each with the `app` it
    /// inherits when it has none of its own.
    pub fn records(&self) -> Vec<ProgramStatusRecord> {
        let mut out: Vec<ProgramStatusRecord> =
            self.records.iter().map(|(r, _)| r.clone()).collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        let apps: Vec<(String, Option<String>)> =
            out.iter().map(|r| (r.id.clone(), r.app.clone())).collect();
        for record in out.iter_mut().filter(|r| r.app.is_none()) {
            // The nearest ancestor with an app: the longest id it sits under
            record.app = apps
                .iter()
                .filter(|(id, app)| app.is_some() && *id != record.id && under(&record.id, id))
                .max_by_key(|(id, _)| id.len())
                .and_then(|(_, app)| app.clone());
        }
        out
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn report(state: ProgramState, id: Option<&str>) -> ProgramStatusReport {
        let mut r = ProgramStatusReport::new(state);
        r.id = id.map(ToString::to_string);
        r
    }

    fn states(store: &ProgramStatusRecords) -> Vec<(String, ProgramState)> {
        store.records().into_iter().map(|r| (r.id, r.state)).collect()
    }

    #[test]
    fn a_report_replaces_its_record_whole() {
        let mut s = ProgramStatusRecords::default();
        let mut first = report(ProgramState::Blocked, None);
        first.msg = Some("Allow?".into());
        assert!(s.apply(first, at(1)));
        s.apply(report(ProgramState::Working, None), at(2));
        let records = s.records();
        assert_eq!(records.len(), 1);
        assert_eq!((records[0].state, records[0].msg.clone()), (ProgramState::Working, None));
        assert_eq!(records[0].updated, at(2));
    }

    #[test]
    fn clear_removes_a_record_and_everything_under_it() {
        let mut s = ProgramStatusRecords::default();
        for id in [None, Some("agent"), Some("agent/1"), Some("agentx"), Some("other")] {
            s.apply(report(ProgramState::Working, id), at(1));
        }
        assert!(s.apply(report(ProgramState::Clear, Some("agent")), at(2)));
        assert_eq!(
            states(&s),
            [("".into(), ProgramState::Working), ("agentx".into(), ProgramState::Working), ("other".into(), ProgramState::Working)]
        );
        assert!(s.apply(report(ProgramState::Clear, None), at(3)));
        assert!(s.records().is_empty());
        assert!(!s.apply(report(ProgramState::Clear, None), at(4)), "nothing left to change");
    }

    #[test]
    fn a_new_prompt_ends_working_and_blocked_but_not_results() {
        let mut s = ProgramStatusRecords::default();
        s.apply(report(ProgramState::Working, None), at(1));
        s.apply(report(ProgramState::Blocked, Some("a")), at(1));
        s.apply(report(ProgramState::Done, Some("b")), at(1));
        s.apply(report(ProgramState::Error, Some("c")), at(1));
        s.apply(report(ProgramState::Idle, Some("d")), at(1));
        assert!(s.end_activity());
        assert_eq!(
            states(&s),
            [("b".into(), ProgramState::Done), ("c".into(), ProgramState::Error), ("d".into(), ProgramState::Idle)]
        );
        assert!(s.reset());
        assert!(s.records().is_empty());
    }

    #[test]
    fn records_inherit_their_app_from_the_nearest_ancestor() {
        let mut s = ProgramStatusRecords::default();
        let mut root = report(ProgramState::Working, None);
        root.app = Some("claude-code".into());
        s.apply(root, at(1));
        let mut tool = report(ProgramState::Working, Some("agent"));
        tool.app = Some("cargo".into());
        s.apply(tool, at(1));
        s.apply(report(ProgramState::Working, Some("agent/build")), at(1));
        s.apply(report(ProgramState::Working, Some("solo")), at(1));
        let apps: Vec<_> = s.records().into_iter().map(|r| (r.id, r.app)).collect();
        assert_eq!(apps[2], ("agent/build".into(), Some("cargo".into())));
        assert_eq!(apps[3], ("solo".into(), Some("claude-code".into())));
    }

    #[test]
    fn past_the_cap_the_least_recently_updated_goes() {
        let mut s = ProgramStatusRecords::default();
        for i in 0..MAX_RECORDS {
            s.apply(report(ProgramState::Working, Some(&format!("r{i}"))), at(1));
        }
        // Touch the first so the second is now the oldest
        s.apply(report(ProgramState::Done, Some("r0")), at(2));
        s.apply(report(ProgramState::Working, Some("new")), at(3));
        let ids: Vec<String> = s.records().into_iter().map(|r| r.id).collect();
        assert_eq!(ids.len(), MAX_RECORDS);
        assert!(ids.contains(&"r0".to_string()) && ids.contains(&"new".to_string()));
        assert!(!ids.contains(&"r1".to_string()));
    }
}
