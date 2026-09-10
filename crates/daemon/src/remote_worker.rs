use ferry_proto::ipc::{IpcEvent, IpcResource};
use ferry_store::remote_jobs;

use crate::provider::ProviderRegistry;
use crate::store_adapter::SqliteStore;

const IDLE_POLL: std::time::Duration = std::time::Duration::from_millis(500);

pub fn run<F>(mut store: SqliteStore, provider: ProviderRegistry, emit: F)
where
    F: Fn(IpcEvent),
{
    loop {
        let job = match remote_jobs::claim_next(store.conn()) {
            Ok(Some(job)) => job,
            Ok(None) => {
                std::thread::sleep(IDLE_POLL);
                continue;
            }
            Err(err) => {
                eprintln!("ferry-daemon: remote worker could not read its job queue: {err}");
                std::thread::sleep(IDLE_POLL);
                continue;
            }
        };

        let (result, error) = execute(&job.kind, &job.params, &mut store, &provider);
        if let Err(err) = remote_jobs::finish(store.conn(), &job.id, result.as_deref(), error.as_deref()) {
            eprintln!("ferry-daemon: remote worker could not record job {} completion: {err}", job.id);
        }
        emit(IpcEvent::Changed {
            resource: IpcResource::Provider,
            id: Some(job.id),
        });
    }
}

fn execute(
    kind: &str,
    params: &str,
    store: &mut SqliteStore,
    provider: &ProviderRegistry,
) -> (Option<String>, Option<String>) {
    match kind {
        "gist_publish" => {
            let item_id = json_field(params, "item_id");
            match provider.publish_gist(store, &item_id) {
                Ok(view) => {
                    let _ = provider_audit(store, "gist.published", "ok");
                    (
                        Some(format!("{{\"url\":{},\"id\":{}}}", quote(&view.url), quote(&view.id))),
                        None,
                    )
                }
                Err(e) => (None, Some(e.to_string())),
            }
        }
        "roster_apply" => {
            let locator = json_field(params, "locator");
            match provider.apply_roster(store, &locator) {
                Ok(summary) => {
                    let _ = provider_audit(store, "roster.fetched", "applied");
                    (
                        Some(format!(
                            "{{\"added\":{},\"peer_count\":{},\"skipped\":{}}}",
                            summary.added, summary.peer_count, summary.skipped_existing
                        )),
                        None,
                    )
                }
                Err(e) => (None, Some(e.to_string())),
            }
        }
        other => (None, Some(format!("unknown remote job kind: {other}"))),
    }
}

fn provider_audit(store: &mut SqliteStore, kind: &str, outcome: &str) -> Result<(), ferry_core::ports::StoreError> {
    use ferry_core::ports::Store;
    store.record_provider_event(kind, outcome)
}

fn json_field(params: &str, field: &str) -> String {
    serde_json::from_str::<serde_json::Value>(params)
        .ok()
        .and_then(|v| v.get(field).and_then(|f| f.as_str().map(String::from)))
        .unwrap_or_default()
}

fn quote(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}
