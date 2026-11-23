//! H.264 clips in MP4, decoded here so a clip plays, scrubs and steps by
//! the frame with nothing installed. A clip is read whole into memory
//! (asks carry short clips) and split into its groups of pictures: a group
//! starts at a sync sample and decodes on its own, its frames coming out in
//! the order they are shown. Any frame is reached by decoding its group
//! from the start; the player keeps what it decoded, so stepping back
//! within a group costs one decode of it.

use ahjo_app_kit::Error;
use openh264::OpenH264API;
use openh264::decoder::{DecodedYUV, Decoder, DecoderConfig, Flush};
use openh264::formats::YUVSource;
use std::collections::VecDeque;
use std::io::Cursor;

/// The clip does not read here; the caller offers the system's player.
fn unreadable(words: String) -> Error {
    Error::new("clip_unreadable", words)
}

pub struct Clip {
    samples: Vec<Sample>,
    /// The track's parameter sets as Annex B, put before every sync sample.
    headers: Vec<u8>,
    length_size: usize,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    groups: Vec<Group>,
    frames: usize,
}

struct Sample {
    bytes: Vec<u8>,
}

/// A group of pictures: samples `first..end` in decode order, whose frames
/// are shown as `first_shown..first_shown + (end - first)`.
#[derive(Clone, Copy, Debug)]
struct Group {
    first: usize,
    end: usize,
    first_shown: usize,
}

/// One decoded frame: its place in showing order and its pixels as BGRA,
/// the order the GPU takes them in.
pub struct Frame {
    pub index: usize,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl Clip {
    /// Reads an MP4's first H.264 track. Another codec is refused with
    /// words saying so, and the caller offers the system's player.
    pub fn open(bytes: &[u8]) -> Result<Self, Error> {
        let mut mp4 = mp4::Mp4Reader::read_header(Cursor::new(bytes), bytes.len() as u64)
            .map_err(|e| unreadable(format!("not an MP4 that reads: {e}")))?;
        let track = mp4
            .tracks()
            .values()
            .find(|t| matches!(t.media_type(), Ok(mp4::MediaType::H264)))
            .ok_or_else(|| unreadable("no H.264 video in it".into()))?;
        let id = track.track_id();
        let avcc = &track
            .trak
            .mdia
            .minf
            .stbl
            .stsd
            .avc1
            .as_ref()
            .ok_or_else(|| unreadable("no H.264 settings in the track".into()))?
            .avcc;
        let mut headers = Vec::new();
        for set in avcc
            .sequence_parameter_sets
            .iter()
            .chain(&avcc.picture_parameter_sets)
        {
            headers.extend([0, 0, 0, 1]);
            headers.extend(&set.bytes);
        }
        let length_size = avcc.length_size_minus_one as usize + 1;
        let (width, height) = (track.width() as u32, track.height() as u32);
        let mut fps = track.frame_rate();
        let count = track.sample_count();
        let mut samples = Vec::with_capacity(count as usize);
        // Presentation time and sync flag by decode order.
        let mut shown_at: Vec<(i64, usize)> = Vec::with_capacity(count as usize);
        let mut syncs = Vec::new();
        for n in 1..=count {
            let Some(s) = mp4
                .read_sample(id, n)
                .map_err(|e| unreadable(format!("sample {n} does not read: {e}")))?
            else {
                continue;
            };
            let at = samples.len();
            if s.is_sync || at == 0 {
                syncs.push(at);
            }
            shown_at.push((s.start_time as i64 + s.rendering_offset as i64, at));
            samples.push(Sample {
                bytes: s.bytes.to_vec(),
            });
        }
        if samples.is_empty() {
            return Err(unreadable("the clip has no frames".into()));
        }
        // Showing order: each sample's place among all by presentation time.
        shown_at.sort();
        let mut shown = vec![0; samples.len()];
        for (place, (_, at)) in shown_at.iter().enumerate() {
            shown[*at] = place;
        }
        let groups = syncs
            .iter()
            .enumerate()
            .map(|(g, &first)| {
                let end = syncs.get(g + 1).copied().unwrap_or(samples.len());
                Group {
                    first,
                    end,
                    first_shown: (first..end).map(|s| shown[s]).min().unwrap_or(first),
                }
            })
            .collect();
        if !fps.is_finite() || fps <= 0. {
            fps = 30.;
        }
        Ok(Self {
            frames: samples.len(),
            samples,
            headers,
            length_size,
            width,
            height,
            fps,
            groups,
        })
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The group holding the frame shown at `index`.
    fn group_of(&self, index: usize) -> usize {
        self.groups
            .iter()
            .rposition(|g| g.first_shown <= index)
            .unwrap_or(0)
    }

    /// A sample as Annex B, with the parameter sets before a sync one.
    fn annex_b(&self, at: usize, first: bool, out: &mut Vec<u8>) {
        out.clear();
        if first {
            out.extend(&self.headers);
        }
        let mut b = self.samples[at].bytes.as_slice();
        while b.len() > self.length_size {
            let n = b[..self.length_size]
                .iter()
                .fold(0usize, |n, x| (n << 8) | *x as usize);
            let unit = &b[self.length_size..];
            let n = n.min(unit.len());
            out.extend([0, 0, 0, 1]);
            out.extend(&unit[..n]);
            b = &unit[n..];
        }
    }
}

/// Decoding forward through one group at a time.
pub struct Decoding<'a> {
    clip: &'a Clip,
    group: usize,
    next: usize,
    produced: usize,
    decoder: Decoder,
    pending: VecDeque<Frame>,
    buf: Vec<u8>,
}

impl<'a> Decoding<'a> {
    /// Ready to decode from the start of the group holding `index`.
    pub fn at(clip: &'a Clip, index: usize) -> Result<Self, Error> {
        let group = clip.group_of(index);
        Ok(Self {
            clip,
            group,
            next: clip.groups[group].first,
            produced: 0,
            decoder: fresh()?,
            pending: VecDeque::new(),
            buf: Vec::new(),
        })
    }

    /// The next frame in showing order, or `None` past the clip's end.
    pub fn next_frame(&mut self) -> Result<Option<Frame>, Error> {
        loop {
            if let Some(f) = self.pending.pop_front() {
                return Ok(Some(f));
            }
            let g = self.clip.groups[self.group];
            if self.next < g.end {
                let at = self.next;
                self.next += 1;
                self.clip.annex_b(at, at == g.first, &mut self.buf);
                let decoded = self
                    .decoder
                    .decode(&self.buf)
                    .map_err(|e| unreadable(format!("frame {at} does not decode: {e}")))?;
                if let Some(yuv) = decoded {
                    let index = g.first_shown + self.produced;
                    self.produced += 1;
                    return Ok(Some(frame(index, &yuv)));
                }
                continue;
            }
            // The group's end: what the decoder still holds, then the next
            // group with a decoder of its own.
            let held = self
                .decoder
                .flush_remaining()
                .map_err(|e| unreadable(format!("the clip's last frames do not decode: {e}")))?;
            for yuv in held {
                let index = g.first_shown + self.produced;
                self.produced += 1;
                self.pending.push_back(frame(index, &yuv));
            }
            if !self.pending.is_empty() {
                continue;
            }
            if self.group + 1 >= self.clip.groups.len() {
                return Ok(None);
            }
            self.group += 1;
            self.next = self.clip.groups[self.group].first;
            self.produced = 0;
            self.decoder = fresh()?;
        }
    }

    /// The frame that would come next, so a caller knows whether a wanted
    /// frame lies ahead of it without starting again.
    pub fn position(&self) -> usize {
        self.pending
            .front()
            .map(|f| f.index)
            .unwrap_or(self.clip.groups[self.group].first_shown + self.produced)
    }

    pub fn group(&self) -> usize {
        self.group
    }

    pub fn group_of(&self, index: usize) -> usize {
        self.clip.group_of(index)
    }
}

fn fresh() -> Result<Decoder, Error> {
    Decoder::with_api_config(
        OpenH264API::from_source(),
        DecoderConfig::new().flush_after_decode(Flush::NoFlush),
    )
    .map_err(|e| unreadable(format!("the H.264 decoder did not start: {e}")))
}

/// The decoder's picture as BGRA, the order the GPU takes.
fn frame(index: usize, yuv: &DecodedYUV<'_>) -> Frame {
    let (w, h) = yuv.dimensions();
    let mut bgra = vec![0u8; w * h * 4];
    yuv.write_rgba8(&mut bgra);
    for p in bgra.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    Frame {
        index,
        width: w as u32,
        height: h as u32,
        bgra,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 48 frames whose luma is 4 x their number, with B-frames and a sync
    /// sample every 16 (`ramp.mp4`, made by ffmpeg's geq and libx264 high).
    const RAMP: &[u8] = include_bytes!("../tests/fixtures/ramp.mp4");

    fn grey(f: &Frame) -> f32 {
        let px = f.bgra.len() / 4;
        f.bgra.chunks(4).map(|p| p[1] as f32).sum::<f32>() / px as f32
    }

    /// The frame shown at `n` has luma 4n; through the RGB conversion its
    /// green is (4n - 16) x 1.164.
    fn expected(n: usize) -> f32 {
        ((4. * n as f32 - 16.) * 1.164).clamp(0., 255.)
    }

    #[test]
    fn every_frame_comes_out_once_in_showing_order() {
        let clip = Clip::open(RAMP).unwrap();
        assert_eq!(clip.frames(), 48);
        assert_eq!((clip.width, clip.height), (64, 48));
        let mut d = Decoding::at(&clip, 0).unwrap();
        let mut n = 0;
        while let Some(f) = d.next_frame().unwrap() {
            assert_eq!(f.index, n);
            assert!(
                (grey(&f) - expected(n)).abs() < 4.,
                "frame {n} shows {} not {}",
                grey(&f),
                expected(n)
            );
            n += 1;
        }
        assert_eq!(n, 48);
    }

    #[test]
    fn a_frame_mid_group_is_reached_from_its_groups_start() {
        let clip = Clip::open(RAMP).unwrap();
        let mut d = Decoding::at(&clip, 37).unwrap();
        assert_eq!(d.position(), 32, "the third group starts at 32");
        let f = loop {
            let f = d.next_frame().unwrap().unwrap();
            if f.index == 37 {
                break f;
            }
        };
        assert!((grey(&f) - expected(37)).abs() < 4.);
    }

    #[test]
    fn what_is_not_h264_in_mp4_is_refused_in_words() {
        assert!(Clip::open(b"not a clip").is_err());
    }
}
