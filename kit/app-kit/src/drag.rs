//! What one page drags onto another: plain data, so pages never depend on
//! each other. A file on a node, dragged from the Files panel or page, is
//! a [`NodeFile`]; Send to Session attaches it where it is dropped.

use crate::theme::theme;
use crate::widgets::kit;
use ahjo_core::NodeId;
use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, px};

/// A file on a node, by its path there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeFile {
    /// Empty for the serving node, as in a [`crate::host::Request`].
    pub node: NodeId,
    pub path: String,
}

impl NodeFile {
    /// The file's name: the last part of its path.
    pub fn name(&self) -> &str {
        let path = self.path.trim_end_matches(['/', '\\']);
        path.rsplit(['/', '\\']).next().unwrap_or(path)
    }
}

/// What follows the pointer while a [`NodeFile`] is dragged: its name.
impl Render for NodeFile {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        kit::panel(t)
            .px(px(10.))
            .py(px(4.))
            .child(self.name().to_string())
    }
}
