//! Evidence paths and problems for a run, fetched independently of its live projection.
use ahjo_missions_contract::{RunEvidence, RunOutput};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Files the run wrote which are not already among its outputs.
pub fn written_outputs(evidence: &RunEvidence, cwd: &str, outputs: &[RunOutput]) -> Vec<RunOutput> {
    let base = format!("{}/", cwd.trim_end_matches('/'));
    evidence
        .written
        .iter()
        // An output already names the file, even when the run copied it
        // into its own store under the same name.
        .filter(|w| {
            !outputs.iter().any(|o| {
                o.value == w.path
                    || (matches!(o.kind.as_str(), "file" | "folder")
                        && file_name(&o.value) == file_name(&w.path))
            })
        })
        .map(|w| RunOutput {
            name: w.path.strip_prefix(&base).unwrap_or(&w.path).to_owned(),
            kind: "file".into(),
            value: w.path.clone(),
            card_id: String::new(),
            port_id: String::new(),
            attempt: 0,
            picks: Vec::new(),
        })
        .collect()
}

#[derive(Default)]
pub struct Store {
    pub evidence: Option<RunEvidence>,
    pub read_at: Option<usize>,
    pub error: Option<String>,
    pub images: BTreeMap<String, Image>,
}

pub enum Image {
    Loading,
    Ready(Arc<gpui::RenderImage>),
    Failed(String),
}

pub fn image_format(path: &str) -> Option<gpui::ImageFormat> {
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => gpui::ImageFormat::Png,
        "jpg" | "jpeg" => gpui::ImageFormat::Jpeg,
        "webp" => gpui::ImageFormat::Webp,
        "gif" => gpui::ImageFormat::Gif,
        "svg" => gpui::ImageFormat::Svg,
        "bmp" => gpui::ImageFormat::Bmp,
        "tif" | "tiff" => gpui::ImageFormat::Tiff,
        "ico" => gpui::ImageFormat::Ico,
        _ => return None,
    })
}

fn file_name(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}
