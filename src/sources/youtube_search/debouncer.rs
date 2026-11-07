use super::client::InnerTubeClient;
use super::types::{SearchResponse, SearchScope};
use crate::core::app::App;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{mpsc, watch, Mutex};

static GLOBAL_SEARCHER: OnceLock<DebouncedSearcher> = OnceLock::new();

/// Initializes the global debounced live searcher and hooks its response pump to `App`.
pub fn init_live_search(app: Arc<Mutex<App>>) {
    if GLOBAL_SEARCHER.get().is_some() {
        return;
    }
    let client = Arc::new(InnerTubeClient::new());
    let (searcher, mut rx_result) = DebouncedSearcher::new(client);
    let _ = GLOBAL_SEARCHER.set(searcher);

    tokio::spawn(async move {
        while let Some(response) = rx_result.recv().await {
            let mut app_guard = app.lock().await;
            app_guard.set_live_search_response(response);
        }
    });
}

/// Triggers a debounced live search query globally.
pub fn trigger_query(query: impl Into<String>, scope: SearchScope) {
    if let Some(searcher) = GLOBAL_SEARCHER.get() {
        searcher.on_query_change(query, scope);
    }
}

/// Clears active live search query globally.
pub fn clear_query() {
    if let Some(searcher) = GLOBAL_SEARCHER.get() {
        searcher.clear();
    }
}

/// Default debounce duration per technical specification: 250ms
pub const DEFAULT_DEBOUNCE_MS: u64 = 250;

#[derive(Debug, Clone)]
pub struct DebounceRequest {
    pub id: u64,
    pub query: String,
    pub scope: SearchScope,
}

/// Debounced search runner that manages 250ms debounce intervals,
/// superseding outdated queries on rapid typing.
pub struct DebouncedSearcher {
    client: Arc<InnerTubeClient>,
    debounce_duration: Duration,
    next_request_id: Arc<AtomicU64>,
    active_request_id: Arc<AtomicU64>,
    tx_query: watch::Sender<Option<DebounceRequest>>,
}

impl DebouncedSearcher {
    /// Creates a new DebouncedSearcher and starts its background worker task.
    ///
    /// Results are delivered through the returned `mpsc::Receiver<SearchResponse>`.
    pub fn new(client: Arc<InnerTubeClient>) -> (Self, mpsc::Receiver<SearchResponse>) {
        Self::with_duration(client, Duration::from_millis(DEFAULT_DEBOUNCE_MS))
    }

    /// Creates a new DebouncedSearcher with custom debounce duration
    pub fn with_duration(
        client: Arc<InnerTubeClient>,
        duration: Duration,
    ) -> (Self, mpsc::Receiver<SearchResponse>) {
        let (tx_query, rx_query) = watch::channel(None);
        let (tx_result, rx_result) = mpsc::channel(32);

        let next_request_id = Arc::new(AtomicU64::new(1));
        let active_request_id = Arc::new(AtomicU64::new(0));

        let runner = DebounceWorker {
            client: Arc::clone(&client),
            debounce_duration: duration,
            active_request_id: Arc::clone(&active_request_id),
            rx_query,
            tx_result,
        };

        tokio::spawn(runner.run());

        (
            Self {
                client,
                debounce_duration: duration,
                next_request_id,
                active_request_id,
                tx_query,
            },
            rx_result,
        )
    }

    /// Submits a query change (called on every keystroke in search mode).
    /// Resets the 250ms debounce countdown.
    pub fn on_query_change(&self, query: impl Into<String>, scope: SearchScope) -> u64 {
        let req_id = self.next_request_id.fetch_add(1, Ordering::SeqCst);
        let req = DebounceRequest {
            id: req_id,
            query: query.into(),
            scope,
        };
        let _ = self.tx_query.send(Some(req));
        req_id
    }

    /// Clears any pending search query
    pub fn clear(&self) {
        let req_id = self.next_request_id.fetch_add(1, Ordering::SeqCst);
        self.active_request_id.store(req_id, Ordering::SeqCst);
        let _ = self.tx_query.send(None);
    }

    /// Executes an immediate search without waiting for the 250ms debounce (e.g. on Enter)
    pub async fn search_immediate(&self, query: &str, scope: SearchScope) -> anyhow::Result<SearchResponse> {
        self.client.search_full(query, scope).await
    }

    pub fn debounce_duration(&self) -> Duration {
        self.debounce_duration
    }
}

struct DebounceWorker {
    client: Arc<InnerTubeClient>,
    debounce_duration: Duration,
    active_request_id: Arc<AtomicU64>,
    rx_query: watch::Receiver<Option<DebounceRequest>>,
    tx_result: mpsc::Sender<SearchResponse>,
}

impl DebounceWorker {
    async fn run(mut self) {
        let mut pending: Option<DebounceRequest> = None;

        loop {
            if let Some(req) = pending.take() {
                // Wait for debounce duration or new query
                tokio::select! {
                    _ = tokio::time::sleep(self.debounce_duration) => {
                        // Debounce period elapsed! Check if request is still current
                        let latest_req = self.rx_query.borrow().clone();
                        let is_current = latest_req.as_ref().is_some_and(|r| r.id == req.id);

                        if is_current {
                            self.active_request_id.store(req.id, Ordering::SeqCst);
                            let client = Arc::clone(&self.client);
                            let tx = self.tx_result.clone();
                            let active_id = Arc::clone(&self.active_request_id);

                            tokio::spawn(async move {
                                if let Ok(resp) = client.search_full(&req.query, req.scope).await {
                                    // Verify no newer request was dispatched before sending results
                                    if active_id.load(Ordering::SeqCst) == req.id {
                                        let _ = tx.send(resp).await;
                                    }
                                }
                            });
                        }
                    }
                    changed = self.rx_query.changed() => {
                        if changed.is_err() {
                            // Sender dropped, exit worker
                            break;
                        }
                        pending = self.rx_query.borrow().clone();
                    }
                }
            } else {
                // No pending query, wait for next change
                if self.rx_query.changed().await.is_err() {
                    break;
                }
                pending = self.rx_query.borrow().clone();
            }
        }
    }
}
