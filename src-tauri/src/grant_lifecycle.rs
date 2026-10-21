//! The target grant lifecycle, encoding the "Target grant semantics" section
//! of docs/authorization.md and the decision in
//! docs/decisions/2026-10-21-authorization-one-pipeline.md.
//!
//! Pure: no I/O and no clock; callers pass time and events. Nothing calls
//! this yet (migration step 1 will). Where grants are stored is out of
//! scope (#325).

// Until migration step 1 calls it.
#![allow(dead_code)]

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verb {
    Apply,
    Commit,
    Drop,
}

/// User confirmation that creates an executable grant. Repository evidence
/// is modeled separately because it can establish row-retirement eligibility,
/// but never user consent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmedBy {
    Click,
    Phrase,
}

/// Host-verified evidence that an item's work has already landed. This is
/// eligibility to retire the row, not a grant and not authorization to apply,
/// commit, or drop work that has not landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedLandedEvidence {
    pub item: String,
    pub commit_sha: String,
    pub verified_at_ms: i64,
    pub never_begun: bool,
}

/// A receipt for evidence-based row retirement. Kept separate from grant
/// receipts so it cannot be mistaken for user confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRetirementReceipt {
    pub evidence: VerifiedLandedEvidence,
    pub retired_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GrantKey {
    pub item: String,
    pub verb: Verb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Cancelled,
    Expired,
    Stale,
    Replaced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Live,
    Uncertain,
    Completed,
    Ended(EndReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    Ok,
    KnownFailure,
    Unobserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Execute(AttemptOutcome),
    ReconcileHappened,
    ReconcileDidNotHappen,
    Cancel,
    TtlElapsed,
    PlanStale,
    Replaced,
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptEntry {
    Confirmed(ConfirmedBy),
    Attempt(AttemptOutcome),
    Reconciled { happened: bool },
    Ended(EndReason),
}

/// Why an event was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Terminal,
    UncertainMustReconcile,
    NotUncertain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub key: GrantKey,
    pub confirmed_by: ConfirmedBy,
    pub plan_version: Option<u64>,
    pub issued_at_ms: i64,
    pub state: State,
    pub receipt: Vec<ReceiptEntry>,
}

pub fn transition(state: State, event: Event) -> Result<(State, Option<ReceiptEntry>), Refusal> {
    use AttemptOutcome as A;
    use ReceiptEntry as R;
    if event == Event::Read {
        return Ok((state, None));
    }
    match state {
        State::Completed | State::Ended(_) => Err(Refusal::Terminal),
        State::Live => match event {
            Event::Execute(A::Ok) => Ok((State::Completed, Some(R::Attempt(A::Ok)))),
            Event::Execute(A::KnownFailure) => Ok((State::Live, Some(R::Attempt(A::KnownFailure)))),
            Event::Execute(A::Unobserved) => {
                Ok((State::Uncertain, Some(R::Attempt(A::Unobserved))))
            }
            Event::Cancel => Ok(end(EndReason::Cancelled)),
            Event::TtlElapsed => Ok(end(EndReason::Expired)),
            Event::PlanStale => Ok(end(EndReason::Stale)),
            Event::Replaced => Ok(end(EndReason::Replaced)),
            Event::ReconcileHappened | Event::ReconcileDidNotHappen => Err(Refusal::NotUncertain),
            Event::Read => unreachable!(),
        },
        State::Uncertain => match event {
            Event::ReconcileHappened => {
                Ok((State::Completed, Some(R::Reconciled { happened: true })))
            }
            Event::ReconcileDidNotHappen => {
                Ok((State::Live, Some(R::Reconciled { happened: false })))
            }
            _ => Err(Refusal::UncertainMustReconcile),
        },
    }
}

fn end(reason: EndReason) -> (State, Option<ReceiptEntry>) {
    (State::Ended(reason), Some(ReceiptEntry::Ended(reason)))
}

impl Grant {
    pub fn new(
        key: GrantKey,
        confirmed_by: ConfirmedBy,
        plan_version: Option<u64>,
        issued_at_ms: i64,
    ) -> Grant {
        Grant {
            key,
            confirmed_by,
            plan_version,
            issued_at_ms,
            state: State::Live,
            receipt: vec![ReceiptEntry::Confirmed(confirmed_by)],
        }
    }

    /// Applies the event; on refusal nothing changes.
    pub fn apply(&mut self, event: Event) -> Result<(), Refusal> {
        let (state, entry) = transition(self.state, event)?;
        self.state = state;
        if let Some(entry) = entry {
            self.receipt.push(entry);
        }
        Ok(())
    }

    pub fn may_execute(&self) -> bool {
        self.state == State::Live
    }
}

pub fn ttl_elapsed(issued_at_ms: i64, now_ms: i64, ttl_ms: i64) -> bool {
    now_ms - issued_at_ms >= ttl_ms
}

#[derive(Debug, Default)]
pub struct GrantBook {
    grants: HashMap<GrantKey, Grant>,
}

impl GrantBook {
    pub fn new() -> GrantBook {
        GrantBook::default()
    }

    pub fn get(&self, key: &GrantKey) -> Option<&Grant> {
        self.grants.get(key)
    }

    /// A live grant for the key ends as replaced; an uncertain one blocks
    /// the new authorization until reconciled. Returns the grant this one
    /// displaced, receipt intact, so the caller can keep it: receipts are
    /// append-only and outlive the map slot.
    pub fn authorize(
        &mut self,
        key: GrantKey,
        by: ConfirmedBy,
        plan_version: Option<u64>,
        issued_at_ms: i64,
    ) -> Result<Option<Grant>, Refusal> {
        if let Some(old) = self.grants.get_mut(&key) {
            match old.state {
                State::Uncertain => return Err(Refusal::UncertainMustReconcile),
                State::Live => old.apply(Event::Replaced)?,
                State::Completed | State::Ended(_) => {}
            }
        }
        let grant = Grant::new(key.clone(), by, plan_version, issued_at_ms);
        Ok(self.grants.insert(key, grant))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AttemptOutcome as A;
    use ReceiptEntry as R;

    fn key(item: &str, verb: Verb) -> GrantKey {
        GrantKey {
            item: item.to_string(),
            verb,
        }
    }

    fn grant() -> Grant {
        Grant::new(key("a", Verb::Commit), ConfirmedBy::Click, Some(3), 1000)
    }

    fn uncertain_grant() -> Grant {
        let mut g = grant();
        g.apply(Event::Execute(A::Unobserved)).unwrap();
        g
    }

    fn live_to(event: Event, state: State, entry: ReceiptEntry) {
        assert_eq!(transition(State::Live, event), Ok((state, Some(entry))));
    }

    #[test]
    fn live_execute_ok_completes() {
        live_to(Event::Execute(A::Ok), State::Completed, R::Attempt(A::Ok));
    }

    #[test]
    fn live_execute_known_failure_stays_live() {
        live_to(
            Event::Execute(A::KnownFailure),
            State::Live,
            R::Attempt(A::KnownFailure),
        );
    }

    #[test]
    fn live_execute_unobserved_is_uncertain() {
        live_to(
            Event::Execute(A::Unobserved),
            State::Uncertain,
            R::Attempt(A::Unobserved),
        );
    }

    #[test]
    fn live_cancel_ends_cancelled() {
        live_to(
            Event::Cancel,
            State::Ended(EndReason::Cancelled),
            R::Ended(EndReason::Cancelled),
        );
    }

    #[test]
    fn live_ttl_elapsed_ends_expired() {
        live_to(
            Event::TtlElapsed,
            State::Ended(EndReason::Expired),
            R::Ended(EndReason::Expired),
        );
    }

    #[test]
    fn live_plan_stale_ends_stale() {
        live_to(
            Event::PlanStale,
            State::Ended(EndReason::Stale),
            R::Ended(EndReason::Stale),
        );
    }

    #[test]
    fn live_replaced_ends_replaced() {
        live_to(
            Event::Replaced,
            State::Ended(EndReason::Replaced),
            R::Ended(EndReason::Replaced),
        );
    }

    #[test]
    fn live_reconcile_is_refused_not_uncertain() {
        for e in [Event::ReconcileHappened, Event::ReconcileDidNotHappen] {
            assert_eq!(transition(State::Live, e), Err(Refusal::NotUncertain));
        }
    }

    #[test]
    fn uncertain_reconcile_happened_completes() {
        assert_eq!(
            transition(State::Uncertain, Event::ReconcileHappened),
            Ok((State::Completed, Some(R::Reconciled { happened: true })))
        );
    }

    #[test]
    fn uncertain_reconcile_did_not_happen_goes_live() {
        assert_eq!(
            transition(State::Uncertain, Event::ReconcileDidNotHappen),
            Ok((State::Live, Some(R::Reconciled { happened: false })))
        );
    }

    #[test]
    fn uncertain_refuses_everything_but_reconcile() {
        for e in [
            Event::Execute(A::Ok),
            Event::Execute(A::KnownFailure),
            Event::Execute(A::Unobserved),
            Event::Cancel,
            Event::TtlElapsed,
            Event::PlanStale,
            Event::Replaced,
        ] {
            assert_eq!(
                transition(State::Uncertain, e),
                Err(Refusal::UncertainMustReconcile)
            );
        }
    }

    #[test]
    fn terminal_states_refuse_every_non_read_event() {
        let terminals = [
            State::Completed,
            State::Ended(EndReason::Cancelled),
            State::Ended(EndReason::Expired),
            State::Ended(EndReason::Stale),
            State::Ended(EndReason::Replaced),
        ];
        let events = [
            Event::Execute(A::Ok),
            Event::Execute(A::KnownFailure),
            Event::Execute(A::Unobserved),
            Event::ReconcileHappened,
            Event::ReconcileDidNotHappen,
            Event::Cancel,
            Event::TtlElapsed,
            Event::PlanStale,
            Event::Replaced,
        ];
        for s in terminals {
            for e in events {
                assert_eq!(transition(s, e), Err(Refusal::Terminal));
            }
        }
    }

    #[test]
    fn read_never_changes_state_or_receipt() {
        let states = [
            State::Live,
            State::Uncertain,
            State::Completed,
            State::Ended(EndReason::Cancelled),
        ];
        for s in states {
            assert_eq!(transition(s, Event::Read), Ok((s, None)));
            let mut g = grant();
            g.state = s;
            let before = g.clone();
            g.apply(Event::Read).unwrap();
            assert_eq!(g, before);
        }
    }

    #[test]
    fn refused_event_changes_nothing() {
        let mut g = uncertain_grant();
        let before = g.clone();
        assert_eq!(g.apply(Event::Cancel), Err(Refusal::UncertainMustReconcile));
        assert_eq!(g, before);
    }

    #[test]
    fn uncertain_never_permits_execute_until_reconciled() {
        let mut g = grant();
        g.apply(Event::Execute(A::Unobserved)).unwrap();
        assert!(!g.may_execute());
        assert_eq!(
            g.apply(Event::Execute(A::Ok)),
            Err(Refusal::UncertainMustReconcile)
        );
        g.apply(Event::ReconcileDidNotHappen).unwrap();
        assert!(g.may_execute());
        g.apply(Event::Execute(A::KnownFailure)).unwrap();
        g.apply(Event::Execute(A::Ok)).unwrap();
        assert_eq!(g.state, State::Completed);
        assert_eq!(
            g.receipt,
            vec![
                R::Confirmed(ConfirmedBy::Click),
                R::Attempt(A::Unobserved),
                R::Reconciled { happened: false },
                R::Attempt(A::KnownFailure),
                R::Attempt(A::Ok),
            ]
        );
    }

    #[test]
    fn retry_after_known_failure_adds_no_confirmed_entry() {
        let mut g = grant();
        g.apply(Event::Execute(A::KnownFailure)).unwrap();
        g.apply(Event::Execute(A::Ok)).unwrap();
        let confirmed = g
            .receipt
            .iter()
            .filter(|e| matches!(e, R::Confirmed(_)))
            .count();
        assert_eq!(confirmed, 1);
    }

    #[test]
    fn book_keys_do_not_displace_each_other() {
        let mut book = GrantBook::new();
        let keys = [
            key("a", Verb::Apply),
            key("a", Verb::Commit),
            key("b", Verb::Commit),
        ];
        for k in &keys {
            book.authorize(k.clone(), ConfirmedBy::Click, None, 0)
                .unwrap();
        }
        for k in &keys {
            assert_eq!(book.get(k).unwrap().state, State::Live);
            assert_eq!(book.get(k).unwrap().receipt.len(), 1);
        }
    }

    #[test]
    fn book_replaces_live_grant() {
        let mut book = GrantBook::new();
        let k = key("a", Verb::Commit);
        book.authorize(k.clone(), ConfirmedBy::Click, Some(1), 0)
            .unwrap();
        book.authorize(k.clone(), ConfirmedBy::Phrase, Some(2), 5)
            .unwrap();
        let g = book.get(&k).unwrap();
        assert_eq!(g.state, State::Live);
        assert_eq!(g.confirmed_by, ConfirmedBy::Phrase);
        assert_eq!(g.plan_version, Some(2));
        assert_eq!(g.receipt, vec![R::Confirmed(ConfirmedBy::Phrase)]);
    }

    #[test]
    fn book_replace_ends_old_grant_as_replaced() {
        let mut book = GrantBook::new();
        let k = key("a", Verb::Drop);
        assert_eq!(
            book.authorize(k.clone(), ConfirmedBy::Click, Some(1), 0),
            Ok(None)
        );
        let old = book
            .authorize(k.clone(), ConfirmedBy::Phrase, Some(2), 5)
            .unwrap()
            .expect("the displaced grant is handed back");
        assert_eq!(old.state, State::Ended(EndReason::Replaced));
        assert_eq!(
            old.receipt,
            vec![
                R::Confirmed(ConfirmedBy::Click),
                R::Ended(EndReason::Replaced)
            ]
        );
    }

    #[test]
    fn book_refuses_to_replace_uncertain_grant() {
        let mut book = GrantBook::new();
        let k = key("a", Verb::Commit);
        book.authorize(k.clone(), ConfirmedBy::Click, Some(1), 0)
            .unwrap();
        book.grants
            .get_mut(&k)
            .unwrap()
            .apply(Event::Execute(A::Unobserved))
            .unwrap();
        assert_eq!(
            book.authorize(k.clone(), ConfirmedBy::Click, Some(2), 9),
            Err(Refusal::UncertainMustReconcile)
        );
        let g = book.get(&k).unwrap();
        assert_eq!(g.state, State::Uncertain);
        assert_eq!(g.plan_version, Some(1));
    }

    #[test]
    fn book_authorizes_after_terminal_grant() {
        let mut book = GrantBook::new();
        let k = key("a", Verb::Drop);
        book.authorize(k.clone(), ConfirmedBy::Click, None, 0)
            .unwrap();
        book.grants
            .get_mut(&k)
            .unwrap()
            .apply(Event::Execute(A::Ok))
            .unwrap();
        let displaced = book
            .authorize(k.clone(), ConfirmedBy::Phrase, None, 7)
            .unwrap();
        assert!(displaced.is_some());
        assert_eq!(book.get(&k).unwrap().state, State::Live);
    }

    #[test]
    fn landed_evidence_is_a_retirement_receipt_not_a_grant_confirmation() {
        let receipt = EvidenceRetirementReceipt {
            evidence: VerifiedLandedEvidence {
                item: "a".to_string(),
                commit_sha: "abc123".to_string(),
                verified_at_ms: 10,
                never_begun: false,
            },
            retired_at_ms: 11,
        };
        assert_eq!(receipt.evidence.item, "a");
        assert_eq!(receipt.evidence.commit_sha, "abc123");
        assert!(!receipt.evidence.never_begun);
        assert_eq!(receipt.retired_at_ms, 11);
    }

    #[test]
    fn ttl_elapsed_boundary() {
        assert!(ttl_elapsed(1000, 1500, 500));
        assert!(!ttl_elapsed(1000, 1499, 500));
    }
}
