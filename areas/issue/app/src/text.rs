//! Items' text, which the snapshot leaves out: read from the item's node
//! when the item shows, and searched on the nodes when the search changes.
use crate::Issue;
use ahjo_core::NodeId;
use ahjo_issue_contract::commands::{ReadSnapshot, SearchSnapshot};
use gpui::Context;
use std::collections::HashSet;

/// One item's text as the page has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Text {
    /// Being read; what was read for an older update meanwhile.
    Reading(Option<String>),
    Read(String),
    Failed(String),
}

impl Issue {
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
        let task = self.host.call(
            NodeId(node),
            ReadSnapshot {
                reference: reference.clone(),
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let text = match task.await {
                Ok(answer) => Text::Read(answer.text),
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
        if ahjo_issue_contract::text::terms(&query).is_empty() {
            return;
        }
        for node in self.snapshots.keys() {
            let task = self.host.call(
                NodeId(node.clone()),
                SearchSnapshot {
                    query: query.clone(),
                },
                cx,
            );
            let query = query.clone();
            cx.spawn(async move |this, cx| {
                let answer = task.await;
                let _ = this.update(cx, |w, cx| {
                    let found = answer
                        .map(|answer| answer.references)
                        .map_err(|error| error.message);
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
