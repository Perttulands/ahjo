//! The Issues channel of the browser protocol (`docs/protocol.md`, Issues): a
//! browser watches the snapshot while its Issues tab is open. Items come
//! without their text, which the snapshot text command serves one item at a time.

use anyhow::{Result, bail};

use super::model::Snapshot;
use super::poller::{Issues, Subscription};

/// Browser to server: watch (1), stop watching (0), or watch and read
/// everything again now (2) — a Refresh press.
pub const WATCH: u8 = 0x98;
/// Server to browser: the snapshot as JSON.
pub const SNAPSHOT: u8 = 0x90;

/// One browser's end of the channel.
pub struct Channel {
    work: Issues,
    watching: Option<Subscription>,
}

impl Channel {
    pub fn new(work: Issues) -> Channel {
        Channel {
            work,
            watching: None,
        }
    }

    /// Handles a watch frame.
    pub fn receive(&mut self, frame: &[u8]) -> Result<()> {
        match frame {
            [WATCH, 0] => self.watching = None,
            [WATCH, on] => {
                if self.watching.is_none() {
                    self.watching = Some(self.work.subscribe());
                }
                if *on == 2 {
                    self.work.refresh();
                }
            }
            _ => bail!("watch-work frame needs one byte"),
        }
        Ok(())
    }

    /// The next frame for the browser: the snapshot when watching starts and
    /// whenever it changes. Never resolves while not watching. Cancel-safe.
    pub async fn next(&mut self) -> Vec<u8> {
        match &mut self.watching {
            Some(subscription) => snapshot(&*subscription.next().await),
            None => std::future::pending().await,
        }
    }
}

/// `0x90` and the snapshot as JSON, without the items' text.
pub fn snapshot(snapshot: &Snapshot) -> Vec<u8> {
    let mut frame = vec![SNAPSHOT];
    serde_json::to_writer(&mut frame, &snapshot.lean()).expect("a snapshot always serialises");
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issues::model::{ProjectView, RepoView};

    #[test]
    fn takes_only_one_byte_watch_frames() {
        let mut channel = Channel::new(Issues::default());
        assert!(ahjo_core::frame::owns(ahjo_issue_contract::KINDS, WATCH));
        assert!(!ahjo_core::frame::owns(
            ahjo_issue_contract::KINDS,
            SNAPSHOT
        ));
        assert!(channel.receive(&[WATCH]).is_err());
        assert!(channel.receive(&[WATCH, 1, 0]).is_err());
    }

    #[tokio::test]
    async fn sends_the_snapshot_while_watching() {
        let mut channel = Channel::new(Issues::default());
        channel.receive(&[WATCH, 1]).unwrap();
        assert_eq!(
            channel.next().await,
            b"\x90{\"projects\":[],\"refreshed\":0}"
        );
        channel.receive(&[WATCH, 0]).unwrap();
        let quiet = tokio::time::timeout(std::time::Duration::from_millis(50), channel.next());
        assert!(quiet.await.is_err());
    }

    #[test]
    fn spells_the_snapshot_as_the_browser_reads_it() {
        let work = crate::issues::parse::parse(include_str!("../../tests/fixtures/work/ahjo.json"))
            .unwrap();
        let frame = snapshot(&Snapshot {
            refreshed: 0,
            projects: vec![ProjectView::Github(RepoView {
                name: "example-owner/ahjo".into(),
                checkout: None,
                synced: Some(1),
                error: None,
                issues: Some(work),
            })],
        });
        let json = std::str::from_utf8(&frame[1..]).unwrap();
        for expected in [
            r#"{"projects":[{"tracker":"github","name":"example-owner/ahjo","synced":1,"url":"https://github.com/example-owner/ahjo","open_issues":15,"closed_issues":28,"open_pulls":0,"closed_pulls":27,"issues":[{"number":70,"#,
            r#""state":"merged""#,
            r#""checks":{"passed":1,"failed":0,"pending":0}"#,
            r#""pulls":[58]"#,
        ] {
            assert!(json.contains(expected), "{expected} in {}", &json[..300]);
        }
        assert!(!json.contains("## Goal"), "the text is left out");
        assert!(json.contains(r#""body":"""#), "and its field kept");
    }
}
