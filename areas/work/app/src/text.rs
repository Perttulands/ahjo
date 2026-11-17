//! Items' text, which the snapshot leaves out: read from the item's node
//! when the item shows, and searched on the nodes when the search changes.
use crate::Work;
use ahjo_app_kit::host::Request;
use ahjo_core::NodeId;
use gpui::Context;
use std::collections::HashSet;
use std::time::Duration;

/// One item's text as the page has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Text {
    /// Being read; what was read for an older update meanwhile.
    Reading(Option<String>),
    Read(String),
    Failed(String),
}

/// How long a read of one item's text or a search may take.
const WITHIN: Duration = Duration::from_secs(30);

impl Work {
    /// The text of `row`, read from its node if the board left it out and
    /// it was not yet read for the row's latest update.
    pub(crate) fn want_text(&mut self, row: usize, cx: &mut Context<Self>) {
        let Some(r) = self.board.rows.get(row) else {
            return;
        };
        if !r.body.is_empty() {
            return;
        }
        let older = match self.texts.get(&r.reference) {
            Some((updated, _)) if *updated == r.updated => return,
            Some((_, Text::Read(text) | Text::Reading(Some(text)))) => Some(text.clone()),
            _ => None,
        };
        let Some(node) = self.node_for_project(r.project) else {
            return;
        };
        let (reference, updated) = (r.reference.clone(), r.updated);
        self.texts
            .insert(reference.clone(), (updated, Text::Reading(older)));
        let task = self.host.request(
            Request::get(
                NodeId(node),
                format!("/api/work/text?ref={}", encode(&reference)),
            )
            .within(WITHIN),
            cx,
        );
        cx.spawn(async move |this, cx| {
            let text = match task.await {
                Ok(answer) if answer.ok() => {
                    match serde_json::from_slice::<serde_json::Value>(&answer.body) {
                        Ok(v) => match v["text"].as_str() {
                            Some(text) => Text::Read(text.to_owned()),
                            None => Text::Failed("the node sent no text".into()),
                        },
                        Err(e) => Text::Failed(e.to_string()),
                    }
                }
                // A node from before the text left the snapshot sent it
                // with the item: this one has none.
                Ok(answer) if answer.status == 404 && !answer.body.starts_with(b"{") => {
                    Text::Read(String::new())
                }
                Ok(answer) => Text::Failed(answer.error_text()),
                Err(error) => Text::Failed(error.message),
            };
            let _ = this.update(cx, |w, cx| {
                // A read for an update since replaced is dropped.
                if w.texts.get(&reference).map(|(u, _)| *u) == Some(updated) {
                    w.texts.insert(reference, (updated, text));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The items the nodes say hold `query`'s words in their text, as far
    /// as they have answered.
    pub(crate) fn hits_for(&self, query: &str) -> &HashSet<String> {
        static NONE: std::sync::LazyLock<HashSet<String>> = std::sync::LazyLock::new(HashSet::new);
        if self.text_hits.0 == query.trim() {
            &self.text_hits.1
        } else {
            &NONE
        }
    }

    /// The text of `row` as far as the page has it.
    pub(crate) fn text_of(&self, row: usize) -> Text {
        let r = &self.board.rows[row];
        if !r.body.is_empty() {
            return Text::Read(r.body.clone());
        }
        match self.texts.get(&r.reference) {
            Some((_, text)) => text.clone(),
            None => Text::Reading(None),
        }
    }

    /// Asks every node which items' text holds `query`'s words, for the
    /// page's search or the panel's find; the list takes each answer in as
    /// it lands, if the words are still those. The same words asked again,
    /// for a new snapshot, keep what was found.
    pub(crate) fn search_text(&mut self, query: &str, cx: &mut Context<Self>) {
        let query = query.trim().to_string();
        if self.text_hits.0 != query {
            self.text_hits = (query.clone(), HashSet::new());
        }
        if ahjo_work_contract::text::terms(&query).is_empty() {
            return;
        }
        for node in self.snapshots.keys() {
            let task = self.host.request(
                Request::get(
                    NodeId(node.clone()),
                    format!("/api/work/search?q={}", encode(&query)),
                )
                .within(WITHIN),
                cx,
            );
            let query = query.clone();
            cx.spawn(async move |this, cx| {
                let answer = task.await;
                let _ = this.update(cx, |w, cx| {
                    let found = match answer {
                        Ok(answer) if answer.ok() => {
                            serde_json::from_slice::<serde_json::Value>(&answer.body)
                                .map_err(|e| e.to_string())
                                .map(|v| {
                                    v["references"]
                                        .as_array()
                                        .into_iter()
                                        .flatten()
                                        .filter_map(|r| r.as_str().map(str::to_owned))
                                        .collect::<Vec<_>>()
                                })
                        }
                        // A node from before the text left the snapshot
                        // has no search: its rows carry their text.
                        Ok(answer) if answer.status == 404 && !answer.body.starts_with(b"{") => {
                            Ok(vec![])
                        }
                        Ok(answer) => Err(answer.error_text()),
                        Err(error) => Err(error.message),
                    };
                    if w.text_hits.0 != query {
                        return;
                    }
                    match found {
                        Ok(found) if found.is_empty() => {}
                        Ok(found) => {
                            w.text_hits.1.extend(found);
                            w.relist(cx);
                            cx.notify();
                        }
                        // The titles still match; the page says the text
                        // could not be searched.
                        Err(error) => {
                            w.host
                                .toast(&format!("Searching the text failed: {error}"), cx);
                        }
                    }
                });
            })
            .detach();
        }
    }
}

/// `s` as a query parameter's value.
pub(crate) fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
