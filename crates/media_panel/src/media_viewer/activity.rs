use super::*;

impl MediaViewer {
    /// Load the activity layer's job listing (T2): the newest-first page
    /// that feeds the drawer's history rows, the completed-result ingest,
    /// and the poll's page half. The status probes complete the
    /// observation when the page admits deeper jobs.
    pub(super) fn load_jobs(&mut self, cx: &mut Context<Self>) {
        let Some(epoch) = self.jobs_request.begin(RequestOwner::Jobs) else {
            return;
        };
        self.status = None;
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_jobs_listing(
                epoch,
                Err("Tool invoker not wired — panel cannot query jobs.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let limit = hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT;
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "job_list",
            serde_json::json!({ "limit": limit }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => parse_job_listing(&text),
                Err(error) => Err(format!("job_list failed: {}", error.message())),
            };
            this.update(cx, |this, cx| this.apply_jobs_listing(epoch, result, cx))
                .log_err();
        })
        .detach();
    }

    /// expect: Queue rows and failures belong only to the latest jobs request.
    /// [P1] Motivating: slow queue callbacks cannot replace fresher queue state.
    /// pre: epoch was returned by jobs_request.begin.
    /// post: stale outcomes are ignored; failures preserve rows and stop polling.
    pub(super) fn apply_jobs_listing(
        &mut self,
        epoch: u64,
        result: Result<JobListing, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.jobs_request.owns(epoch) {
            return;
        }
        match result {
            Ok(listing) => {
                let visible_job_ids = listing
                    .jobs
                    .iter()
                    .map(|job| job.id.clone())
                    .collect::<HashSet<_>>();
                self.processed_job_results
                    .retain(|job_id| visible_job_ids.contains(job_id));
                let completed_results = listing
                    .jobs
                    .iter()
                    .filter(|job| job.status == "completed")
                    .filter_map(|job| {
                        job.result
                            .clone()
                            .map(|result| (job.id.clone(), job.op.clone(), result))
                    })
                    .collect::<Vec<_>>();
                self.jobs = listing.jobs;
                self.jobs_total = Some(listing.total);
                self.jobs_limit = listing.limit;
                self.jobs_has_more = listing.has_more;
                self.jobs_history_scope = Some(listing.history_scope);
                self.jobs_restart_behavior = Some(listing.restart_behavior);
                self.jobs_request.complete(epoch, Ok(()));
                for (job_id, op, result) in completed_results {
                    if self.processed_job_results.insert(job_id) {
                        self.ingest_tool_result(&result, &op, cx);
                    }
                }
                if listing.has_more {
                    // Deeper pages exist: the newest-first page cannot see
                    // whether a live job hides behind them — only the status
                    // probes can (T2, loop-register L11). The tick settles
                    // when the battery lands.
                    self.observe_live_jobs(cx);
                } else {
                    // The page IS the whole bounded store: its nonterminal
                    // rows are the complete live set.
                    self.live_jobs = self
                        .jobs
                        .iter()
                        .filter(|job| job_is_nonterminal(job))
                        .cloned()
                        .collect();
                    self.settle_activity_tick(cx);
                }
            }
            Err(error) => {
                self.jobs_request.complete(epoch, Err(error));
                self.stop_queue_polling();
            }
        }
        cx.notify();
    }

    /// The poll condition (T2, loop-register L11): poll while any
    /// nonterminal job is observed ANYWHERE in the bounded store. No
    /// page-visibility limit — a live job deeper than the newest-first
    /// page stays observed through the status probes — and no tab
    /// limit: the activity layer is ambient, not a place you visit. A
    /// failed observation battery over a listing with deeper pages
    /// leaves the deep live set unknown — keep observing rather than
    /// silently stop.
    pub(super) fn should_poll_jobs(&self) -> bool {
        !self.live_jobs.is_empty()
            || (self.jobs_has_more
                && matches!(self.job_observation_request.state, ResourceState::Failed(_)))
    }

    pub(super) fn schedule_queue_poll(&mut self, cx: &mut Context<Self>) {
        if self.queue_poll_task.is_some() || !self.should_poll_jobs() {
            return;
        }
        self.queue_poll_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(QUEUE_POLL_INTERVAL).await;
            this.update(cx, |this, cx| {
                this.queue_poll_task = None;
                if this.should_poll_jobs() {
                    this.load_jobs(cx);
                }
            })
            .log_err();
        }));
    }

    /// One observation battery (T2): the three status-filtered `job_list`
    /// probes. The server filters by status BEFORE the newest-first sort
    /// and limit, so each probe sees every job of that status anywhere in
    /// the bounded store; together with the page's own nonterminal rows
    /// they form the complete live set.
    pub(super) fn observe_live_jobs(&mut self, cx: &mut Context<Self>) {
        let Some(epoch) = self
            .job_observation_request
            .begin(RequestOwner::JobObservation)
        else {
            return; // a battery is already in flight; its apply settles the tick
        };
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_live_observation(
                epoch,
                Err("Tool invoker not wired — panel cannot observe jobs.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let limit = hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT;
        let tasks = NONTERMINAL_JOB_STATUSES.map(|status| {
            invoker.invoke_tool(
                MEDIA_SERVER,
                "job_list",
                serde_json::json!({ "status": status, "limit": limit }),
            )
        });
        cx.spawn(async move |this, cx| {
            let mut observed = Vec::new();
            let mut failure = None;
            for task in tasks {
                let outcome = match task.await {
                    Ok(text) => parse_job_listing(&text),
                    Err(error) => Err(format!("job_list failed: {}", error.message())),
                };
                match outcome {
                    Ok(listing) => observed.extend(listing.jobs),
                    Err(error) => failure = Some(error),
                }
            }
            let result = match failure {
                Some(error) => Err(error),
                None => Ok(observed),
            };
            this.update(cx, |this, cx| {
                this.apply_live_observation(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// expect: The observed live set belongs only to the latest observation battery.
    /// [P1] Motivating: a slow probe battery cannot graft stale rows onto the current live set.
    /// pre: epoch was returned by job_observation_request.begin.
    /// post: stale outcomes are ignored; the owned battery settles the tick exactly once.
    pub(super) fn apply_live_observation(
        &mut self,
        epoch: u64,
        result: Result<Vec<JobRecord>, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.job_observation_request.owns(epoch) {
            return;
        }
        let page_live = self
            .jobs
            .iter()
            .filter(|job| job_is_nonterminal(job))
            .cloned()
            .collect::<Vec<_>>();
        match result {
            Ok(observed) => {
                // Defensive: the probes filter server-side, but the panel's
                // live set is nonterminal BY CONSTRUCTION — a
                // filter-violating response row never enters it (retention
                // would otherwise keep it forever).
                let mut live = page_live;
                live.extend(observed.into_iter().filter(job_is_nonterminal));
                dedupe_jobs_newest_first(&mut live);
                self.live_jobs = live;
                self.job_observation_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                let status = format!("job_list status observation failed: {error}");
                self.job_observation_request.complete(epoch, Err(error));
                self.status = Some(status);
                // A failed battery proves nothing about deeper jobs: keep
                // every previously observed live entry (badges and the
                // poll condition survive a transient probe failure).
                let mut live = page_live;
                let retained = self
                    .live_jobs
                    .iter()
                    .filter(|job| !live.iter().any(|known| known.id == job.id))
                    .cloned()
                    .collect::<Vec<_>>();
                live.extend(retained);
                dedupe_jobs_newest_first(&mut live);
                self.live_jobs = live;
            }
        }
        self.settle_activity_tick(cx);
        cx.notify();
    }

    /// The activity tick's settle step (T2): the live set is final for
    /// this tick — reconcile the R5 transcribe tracking against it, then
    /// arm or stop the 1 Hz poll by the observed-anywhere condition.
    pub(super) fn settle_activity_tick(&mut self, cx: &mut Context<Self>) {
        // A transcribe job that landed while the Library is showing reloads
        // the stored-transcript listing once so the finished transcript
        // appears under its recording in the recordings section (R5 — the
        // section is the recordings browser's new home, T3). Deep transcribe
        // jobs count too — the probes observe them.
        let still_transcribing = self
            .live_jobs
            .iter()
            .filter(|job| job.op == "transcribe")
            .map(|job| job.id.clone())
            .collect::<HashSet<_>>();
        let landed = self
            .transcribing_ids
            .iter()
            .any(|id| !still_transcribing.contains(id));
        self.transcribing_ids = still_transcribing;
        if landed && self.active_tab == ViewerTab::Library {
            self.load_transcript_listing(cx);
        }
        if self.should_poll_jobs() {
            self.schedule_queue_poll(cx);
        } else {
            self.stop_queue_polling();
        }
    }

    /// Toggle the activity drawer (T2): opening refreshes the job listing
    /// — the drawer is observed on demand (FCP Background Tasks), so it
    /// never shows stale history as its opening state.
    pub(super) fn toggle_activity(&mut self, cx: &mut Context<Self>) {
        self.activity_open = !self.activity_open;
        if self.activity_open {
            self.load_jobs(cx);
        }
        cx.notify();
    }

    /// Cancel a queued/running job.
    pub(super) fn cancel_job(&mut self, job_id: String, cx: &mut Context<Self>) {
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.status = Some("Tool invoker not wired — panel cannot cancel jobs.".into());
            cx.notify();
            return;
        };
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "job_cancel",
            serde_json::json!({ "job_id": job_id }),
        );
        cx.spawn(async move |this, cx| match task.await {
            Ok(_) => {
                this.update(cx, |this, cx| this.load_jobs(cx)).log_err();
            }
            Err(error) => {
                this.update(cx, |this, cx| {
                    this.status = Some(format!("job_cancel failed: {}", error.message()));
                    cx.notify();
                })
                .log_err();
            }
        })
        .detach();
    }

    /// The activity drawer (T2): background research jobs observed on
    /// demand — the ambient process state's monitor (FCP Background
    /// Tasks: automatic work, observed when wanted, cancellable here).
    /// Not a navigation mode: the toolbar's Activity toggle opens it over
    /// any mode. Rows keep op/status/created_at with Cancel for
    /// queued/running jobs — the observed live set first (a live job
    /// deeper than the newest-first page is observed by the status probes
    /// and stays cancellable here, loop-register L11), then the page's
    /// recent history. The footer carries the process-local history
    /// disclosure verbatim from the fields the strict listing parse
    /// already validates.
    pub(super) fn render_activity_drawer(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let body = if self.jobs_total.is_none() {
            let (title, hint) = match &self.jobs_request.state {
                ResourceState::Loading => ("Loading activity", "Fetching research jobs…"),
                ResourceState::Failed(error) => ("Activity unavailable", error.as_str()),
                ResourceState::Idle => (
                    "Activity not loaded",
                    "Open the drawer or refresh to load it.",
                ),
                ResourceState::Ready => (
                    "Activity ready — no jobs",
                    "The job listing returned no rows.",
                ),
            };
            self.render_empty(title, hint).into_any_element()
        } else if self.jobs.is_empty() && self.live_jobs.is_empty() {
            self.render_empty(
                "No research jobs",
                "No research jobs have been submitted (or process-local history restarted).",
            )
            .into_any_element()
        } else {
            let live_ids = self
                .live_jobs
                .iter()
                .map(|job| job.id.as_str())
                .collect::<HashSet<_>>();
            let recent = self
                .jobs
                .iter()
                .filter(|job| !live_ids.contains(job.id.as_str()))
                .collect::<Vec<_>>();
            let mut rows = v_flex()
                .id("media-activity-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .gap_0p5()
                .p_2();
            if !self.live_jobs.is_empty() {
                rows = rows.child(Self::section_label("Active"));
                for job in &self.live_jobs {
                    rows = rows.child(self.activity_job_row(job, cx));
                }
            }
            if !recent.is_empty() {
                rows = rows.child(Self::section_label("Recent"));
                for job in recent {
                    rows = rows.child(self.activity_job_row(job, cx));
                }
            }
            rows.into_any_element()
        };
        let total = self.jobs_total.unwrap_or(self.jobs.len());
        let count_label = if self.jobs_has_more {
            format!(
                "Showing {} of {total} jobs (more available; limit {})",
                self.jobs.len(),
                self.jobs_limit
            )
        } else {
            format!("Showing {} of {total} jobs", self.jobs.len())
        };
        v_flex()
            .id("media-activity-drawer")
            .debug_selector(|| "media-activity-drawer".into())
            .h(gpui::relative(ACTIVITY_DRAWER_FRACTION))
            .min_h_0()
            .min_w_0()
            .flex_shrink_0()
            .border_t_1()
            .border_color(cx.theme().colors().border_variant)
            .child(Self::tab_purpose(
                "Background research jobs — watch and cancel here; history is process-local and bounded.",
            ))
            .child(
                h_flex()
                    .h_8()
                    .px_2()
                    .items_center()
                    .min_w_0()
                    .child(
                        Label::new(count_label)
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted)
                            .truncate(),
                    ),
            )
            .child(body)
            .when_some(self.jobs_restart_behavior.as_deref(), |el, disclosure| {
                el.child(
                    div()
                        .id("media-activity-disclosure")
                        .debug_selector(|| "media-activity-disclosure".to_string())
                        .px_2()
                        .py_1()
                        .min_w_0()
                        .border_t_1()
                        .border_color(cx.theme().colors().border_variant)
                        .child(
                            Label::new(SharedString::from(disclosure.to_string()))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Hint),
                        ),
                )
            })
            .into_any_element()
    }

    /// One activity-drawer job row (the retired Queue tab's row shape,
    /// keyed by the stable job id so rows survive listing merges):
    /// op/status/created_at with Cancel for queued/running jobs.
    pub(super) fn activity_job_row(
        &self,
        job: &JobRecord,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let cancellable = job.status == "queued" || job.status == "running";
        let short_id = short_job_id(&job.id);
        let row_selector = format!("media-activity-row-{}", job.id);
        let cancel_selector = format!("media-activity-cancel-{}", job.id);
        div()
            .id(SharedString::from(row_selector.clone()))
            .debug_selector(move || row_selector)
            .h_8()
            .px_2()
            .gap_2()
            .flex()
            .items_center()
            .min_w_0()
            .rounded_sm()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(short_id)
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Hint),
            )
            .child(
                Label::new(job.op.clone())
                    .size(LabelSize::Small)
                    .color(ui::Color::Default),
            )
            .child(
                Label::new(job.status.clone())
                    .size(LabelSize::XSmall)
                    .color(if job.status == "failed" {
                        ui::Color::Error
                    } else if job.status == "completed" {
                        ui::Color::Success
                    } else {
                        ui::Color::Muted
                    }),
            )
            .child(
                Label::new(job.created_at.clone())
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Hint),
            )
            .when(cancellable, |el| {
                let job_id = job.id.clone();
                el.child(
                    div()
                        .id(SharedString::from(cancel_selector.clone()))
                        .debug_selector(move || cancel_selector)
                        .ml_auto()
                        .px_2()
                        .h_5()
                        .flex()
                        .items_center()
                        .rounded_sm()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .cursor_pointer()
                        .child(
                            Label::new("Cancel")
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Muted),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cancel_job(job_id.clone(), cx);
                        })),
                )
            })
    }
}
