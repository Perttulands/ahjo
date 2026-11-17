//! A clip on the stage: a decoding thread of its own and the frames it has
//! decoded, kept near the one shown so stepping and scrubbing answer at
//! once. The UI owns the frames (it alone may free them from the GPU);
//! the thread only decodes what it is asked for.

use crate::tokens;
use crate::video::{Clip, Decoding};
use gpui::RenderImage;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

pub enum Want {
    /// Decode up to this frame and stop.
    Seek(usize),
    /// Decode from this frame on, a few ahead of the one shown.
    Play(usize),
}

pub enum Out {
    Frame(usize, Arc<RenderImage>),
    Failed(String),
}

pub struct Player {
    pub clip: Arc<Clip>,
    frames: BTreeMap<usize, Arc<RenderImage>>,
    bytes: usize,
    /// The frame wanted on screen.
    pub at: usize,
    /// The frame last drawn, shown while the wanted one decodes.
    pub drawn: Option<usize>,
    pub playing: bool,
    pub failed: Option<String>,
    shown: Arc<AtomicUsize>,
    want: mpsc::Sender<Want>,
}

impl Player {
    /// Starts the clip's decoding thread; what it decodes arrives on the
    /// receiver, for the UI to [`Player::keep`].
    pub fn start(clip: Arc<Clip>) -> (Self, async_channel::Receiver<Out>) {
        let (want, wants) = mpsc::channel();
        let (out, outs) = async_channel::unbounded();
        let shown = Arc::new(AtomicUsize::new(0));
        let (c, s) = (clip.clone(), shown.clone());
        let started = std::thread::Builder::new()
            .name("desk-video".into())
            .spawn(move || decode(c, wants, out, s));
        let failed = started
            .err()
            .map(|e| format!("the video thread did not start: {e}"));
        let player = Self {
            clip,
            frames: BTreeMap::new(),
            bytes: 0,
            at: 0,
            drawn: None,
            playing: false,
            failed,
            shown,
            want,
        };
        let _ = player.want.send(Want::Seek(0));
        (player, outs)
    }

    pub fn frames(&self) -> usize {
        self.clip.frames()
    }

    pub fn seconds(&self, frame: usize) -> f64 {
        frame as f64 / self.clip.fps
    }

    /// The frame to draw: the wanted one when decoded, else the last drawn.
    pub fn image(&mut self) -> Option<Arc<RenderImage>> {
        if let Some(f) = self.frames.get(&self.at) {
            self.drawn = Some(self.at);
            return Some(f.clone());
        }
        self.drawn.and_then(|d| self.frames.get(&d).cloned())
    }

    pub fn has(&self, frame: usize) -> bool {
        self.frames.contains_key(&frame)
    }

    /// Keeps a decoded frame; answers the frames let go to stay in budget,
    /// which the caller frees from the GPU.
    pub fn keep(&mut self, index: usize, image: Arc<RenderImage>) -> Vec<Arc<RenderImage>> {
        let size = (self.clip.width * self.clip.height * 4) as usize;
        if let Some(old) = self.frames.insert(index, image) {
            return vec![old];
        }
        self.bytes += size;
        let mut gone = Vec::new();
        while self.bytes > tokens::VIDEO_CACHE && self.frames.len() > 1 {
            // The frame farthest from the one shown goes first.
            let (first, last) = (
                *self.frames.keys().next().unwrap_or(&0),
                *self.frames.keys().next_back().unwrap_or(&0),
            );
            let far = if self.at.abs_diff(first) > self.at.abs_diff(last) {
                first
            } else {
                last
            };
            if let Some(f) = self.frames.remove(&far) {
                gone.push(f);
                self.bytes -= size;
            }
        }
        gone
    }

    /// Every frame kept, to free when the clip leaves the stage.
    pub fn release(&mut self) -> Vec<Arc<RenderImage>> {
        self.bytes = 0;
        std::mem::take(&mut self.frames).into_values().collect()
    }

    /// Shows `frame`, paused.
    pub fn seek(&mut self, frame: usize) {
        self.playing = false;
        self.at = frame.min(self.frames().saturating_sub(1));
        self.shown.store(self.at, Ordering::Relaxed);
        if !self.has(self.at) {
            let _ = self.want.send(Want::Seek(self.at));
        }
    }

    pub fn step(&mut self, by: i64) {
        let to = (self.at as i64 + by).clamp(0, self.frames() as i64 - 1);
        self.seek(to as usize);
    }

    pub fn play(&mut self) {
        if self.at + 1 >= self.frames() {
            self.at = 0;
        }
        self.playing = true;
        self.shown.store(self.at, Ordering::Relaxed);
        let _ = self.want.send(Want::Play(self.at));
    }

    pub fn pause(&mut self) {
        self.playing = false;
        let _ = self.want.send(Want::Seek(self.at));
    }

    /// A tick of the clock while playing: the next frame if it is here.
    /// At the end the clip starts again.
    pub fn tick(&mut self) {
        if !self.playing {
            return;
        }
        let next = self.at + 1;
        if next >= self.frames() {
            self.at = 0;
            self.shown.store(0, Ordering::Relaxed);
            let _ = self.want.send(Want::Play(0));
        } else if self.has(next) {
            self.at = next;
            self.shown.store(next, Ordering::Relaxed);
        }
    }
}

/// The decoding thread: one wanted frame at a time, or a run ahead of the
/// one shown while playing. It ends when the player is dropped.
fn decode(
    clip: Arc<Clip>,
    wants: mpsc::Receiver<Want>,
    out: async_channel::Sender<Out>,
    shown: Arc<AtomicUsize>,
) {
    let mut decoding: Option<Decoding<'_>> = None;
    let mut target: Option<usize> = None;
    let mut playing = false;
    loop {
        // Take every request waiting; wait for one when there is no work.
        let next = if target.is_none() {
            wants.recv().ok()
        } else {
            match wants.try_recv() {
                Ok(w) => Some(w),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        };
        match next {
            Some(Want::Seek(i)) => {
                target = Some(i);
                playing = false;
                continue;
            }
            Some(Want::Play(i)) => {
                target = Some(i);
                playing = true;
                continue;
            }
            None if target.is_none() => return,
            None => {}
        }
        let Some(t) = target else { continue };
        if playing && t > shown.load(Ordering::Relaxed) + tokens::VIDEO_AHEAD {
            std::thread::sleep(Duration::from_millis(4));
            continue;
        }
        let fresh = match &decoding {
            None => true,
            Some(d) => d.position() > t || d.group_of(t) > d.group() + 1,
        };
        if fresh {
            match Decoding::at(&clip, t) {
                Ok(d) => decoding = Some(d),
                Err(e) => {
                    let _ = out.send_blocking(Out::Failed(e));
                    target = None;
                    continue;
                }
            }
        }
        let Some(d) = decoding.as_mut() else { continue };
        match d.next_frame() {
            Ok(Some(f)) => {
                if f.index + tokens::VIDEO_BEHIND >= t {
                    let image = picture(f.width, f.height, f.bgra);
                    if out.send_blocking(Out::Frame(f.index, image)).is_err() {
                        return;
                    }
                }
                if f.index >= t {
                    target = playing.then_some(f.index + 1);
                }
            }
            Ok(None) => {
                target = None;
                decoding = None;
            }
            Err(e) => {
                let _ = out.send_blocking(Out::Failed(e));
                target = None;
                decoding = None;
            }
        }
    }
}

fn picture(w: u32, h: u32, bgra: Vec<u8>) -> Arc<RenderImage> {
    let buffer = image::RgbaImage::from_raw(w, h, bgra)
        .unwrap_or_else(|| image::RgbaImage::new(w.max(1), h.max(1)));
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}
