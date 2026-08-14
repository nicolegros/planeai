use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::client::JiraClient;
use crate::config::JiraConfig;
use crate::repository::JiraRepository;
use planeai_tasks::model::{CreateParams, Status, UpdateParams};
use planeai_tasks::provider::TaskProvider;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SyncResult {
    pub created: usize,
    pub updated: usize,
    pub departed: usize,
    pub errors: usize,
}

/// Notified when issues disappear from JQL results.
/// The app layer implements this to show confirmation toasts.
pub trait SyncListener: Send + Sync {
    /// Called for each issue that disappeared from JQL results.
    /// The listener is responsible for eventually marking the task done (or not).
    fn on_issue_departed(&self, issue_key: &str, summary: &str);

    /// Called after each successful sync cycle completes.
    fn on_sync_complete(&self, result: &SyncResult);
}

/// No-op listener for tests that don't care about events.
pub struct NoOpListener;
impl SyncListener for NoOpListener {
    fn on_issue_departed(&self, _key: &str, _summary: &str) {}
    fn on_sync_complete(&self, _result: &SyncResult) {}
}

pub struct JiraSync {
    client: Arc<JiraClient>,
    repo: Arc<JiraRepository>,
    task_provider: Arc<dyn TaskProvider + Send + Sync>,
    config: JiraConfig,
    listener: Arc<dyn SyncListener>,
    /// Serializes timer-driven and manually requested runs against the shared SQLite/task state.
    run_lock: tokio::sync::Mutex<()>,
}

impl JiraSync {
    pub fn new(
        client: Arc<JiraClient>,
        repo: Arc<JiraRepository>,
        task_provider: Arc<dyn TaskProvider + Send + Sync>,
        config: JiraConfig,
    ) -> Self {
        Self::with_listener(client, repo, task_provider, config, Arc::new(NoOpListener))
    }

    pub fn with_listener(
        client: Arc<JiraClient>,
        repo: Arc<JiraRepository>,
        task_provider: Arc<dyn TaskProvider + Send + Sync>,
        config: JiraConfig,
        listener: Arc<dyn SyncListener>,
    ) -> Self {
        Self {
            client,
            repo,
            task_provider,
            config,
            listener,
            run_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn start(&self, cancel: CancellationToken) {
        let mut interval =
            tokio::time::interval(Duration::from_millis(self.config.sync_interval_ms));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("jira sync loop cancelled");
                    return;
                }
                _ = interval.tick() => {
                    match self.sync_now_with_cancellation(&cancel).await {
                        Ok(r) if !cancel.is_cancelled() => {
                            info!(created = r.created, updated = r.updated, departed = r.departed, errors = r.errors, "jira sync complete");
                        }
                        Ok(_) | Err(crate::Error::Cancelled) => {
                            info!("jira sync loop cancelled");
                            return;
                        }
                        Err(e) => warn!(error = %e, "jira sync error"),
                    }
                }
            }
        }
    }

    pub async fn sync_now(&self) -> Result<SyncResult, crate::Error> {
        self.sync_now_with_cancellation(&CancellationToken::new())
            .await
    }

    pub async fn sync_now_with_cancellation(
        &self,
        cancel: &CancellationToken,
    ) -> Result<SyncResult, crate::Error> {
        let _run = tokio::select! {
            _ = cancel.cancelled() => return Err(crate::Error::Cancelled),
            guard = self.run_lock.lock() => guard,
        };
        if cancel.is_cancelled() {
            return Err(crate::Error::Cancelled);
        }

        let mut result = SyncResult::default();

        if self.config.sources.is_empty() {
            tracing::warn!("sync_now: no sync sources configured");
            return Ok(result);
        }

        for (source_name, mapping) in &self.config.sources {
            tokio::select! {
                _ = cancel.cancelled() => return Err(crate::Error::Cancelled),
                sync_result = self.sync_project(source_name, mapping, &mut result, cancel) => match sync_result {
                    Ok(()) => {}
                    Err(crate::Error::Cancelled) => return Err(crate::Error::Cancelled),
                    Err(e) => {
                        warn!(source = %source_name, error = %e, "sync failed for source, continuing");
                        result.errors += 1;
                    }
                }
            }

            if cancel.is_cancelled() {
                return Err(crate::Error::Cancelled);
            }
        }

        if result.errors == 0 {
            self.listener.on_sync_complete(&result);
        }

        Ok(result)
    }

    async fn sync_project(
        &self,
        source_name: &str,
        mapping: &crate::config::JiraSyncSource,
        result: &mut SyncResult,
        cancel: &CancellationToken,
    ) -> Result<(), crate::Error> {
        let issues = self.client.search(&mapping.jql).await?;
        if cancel.is_cancelled() {
            return Err(crate::Error::Cancelled);
        }

        let mut seen_keys = HashSet::new();

        for issue in &issues {
            if cancel.is_cancelled() {
                return Err(crate::Error::Cancelled);
            }
            seen_keys.insert(issue.issue_key.clone());

            // Upsert raw issue into local store
            let jira_issue = crate::model::JiraIssue {
                issue_key: issue.issue_key.clone(),
                summary: issue.summary.clone(),
                description: issue.description.clone(),
                status: issue.status.clone(),
                priority: issue.priority.clone(),
                labels: issue.labels.clone(),
                sync_status: crate::model::SyncStatus::Synced,
                last_synced_at: chrono::Utc::now(),
                source_name: source_name.to_string(),
            };
            self.repo.upsert_issue(&jira_issue)?;

            match self.task_provider.get(&issue.issue_key) {
                Err(planeai_tasks::provider::Error::NotFound) => {
                    let status = map_status(
                        &issue.status,
                        &mapping.status_map,
                        issue.status_category.as_deref(),
                    );
                    let priority = map_priority(issue.priority.as_deref());
                    self.task_provider.create(CreateParams {
                        key: Some(issue.issue_key.clone()),
                        title: issue.summary.clone(),
                        description: issue.description.clone(),
                        status: Some(status),
                        priority,
                        tags: issue.labels.clone(),
                        ..Default::default()
                    })?;
                    result.created += 1;
                }
                Ok(task) => {
                    let new_status = map_status(
                        &issue.status,
                        &mapping.status_map,
                        issue.status_category.as_deref(),
                    );
                    let needs_update = task.title != issue.summary
                        || task.description != issue.description
                        || task.status != new_status;

                    if needs_update {
                        self.task_provider.update(
                            &issue.issue_key,
                            UpdateParams {
                                title: Some(issue.summary.clone()),
                                description: Some(issue.description.clone()),
                                status: Some(new_status),
                                ..Default::default()
                            },
                        )?;
                        result.updated += 1;
                    }

                    self.repo.mark_synced(&issue.issue_key)?;
                }
                Err(e) => return Err(crate::Error::Storage(e.to_string())),
            }
        }

        // Notify listener about issues that disappeared from JQL results
        let synced_keys = self.repo.list_synced_keys(source_name)?;
        for key in synced_keys {
            if cancel.is_cancelled() {
                return Err(crate::Error::Cancelled);
            }
            if seen_keys.contains(&key) {
                continue;
            }
            match self.task_provider.get(&key) {
                Ok(task) => {
                    if task.status == Status::Done {
                        continue;
                    }

                    self.listener.on_issue_departed(&key, &task.title);
                    self.repo.mark_departed(&[&key])?;
                    result.departed += 1;
                }
                Err(planeai_tasks::provider::Error::NotFound) => {}
                Err(e) => {
                    warn!(key = %key, error = %e, "failed to look up task for disappeared issue");
                }
            }
        }

        Ok(())
    }
}

fn map_status(
    jira_status: &str,
    status_map: &std::collections::HashMap<String, String>,
    status_category: Option<&str>,
) -> Status {
    // 1. Explicit status_map takes priority
    if let Some(mapped) = status_map.get(jira_status).and_then(|v| Status::parse(v)) {
        return mapped;
    }
    // 2. Fall back to Jira statusCategory
    if let Some(category) = status_category {
        match category {
            "To Do" => return Status::Todo,
            "In Progress" => return Status::InProgress,
            "Done" => return Status::Done,
            _ => {}
        }
    }
    // 3. Default
    Status::Todo
}

fn map_priority(name: Option<&str>) -> i32 {
    match name {
        Some("Highest") => 1,
        Some("High") => 2,
        Some("Medium") => 3,
        Some("Low") => 4,
        Some("Lowest") => 5,
        _ => 0,
    }
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
