use crate::models::{ItemStatus, QueueItem};

#[derive(Default)]
pub struct TransferTotals {
    pub downloaded_bytes: u64,
    pub known_total_bytes: u64,
    pub with_known_total: usize,
}

#[derive(Default)]
pub struct StatusCounts {
    pub resolving: usize,
    pub ready: usize,
    pub queued: usize,
    pub active: usize,
    pub done: usize,
    pub failed: usize,
}

pub fn compute_status_counts(items: &[QueueItem]) -> StatusCounts {
    let mut out = StatusCounts::default();
    for it in items {
        match it.status {
            ItemStatus::Resolving => out.resolving += 1,
            ItemStatus::Idle => out.ready += 1,
            ItemStatus::Queued => out.queued += 1,
            ItemStatus::Downloading => out.active += 1,
            ItemStatus::Done => out.done += 1,
            ItemStatus::Failed => out.failed += 1,
        }
    }
    out
}
