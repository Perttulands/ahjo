//! Typed requests shared by the harness and skills views.
use crate::HarnessPage;
use ahjo_core::{NodeId, TypedCommand};
use gpui::Context;

impl HarnessPage {
    pub(crate) fn harness_call<C: TypedCommand>(
        &mut self,
        node: &str,
        input: C,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<C::Output, ahjo_core::Error>, &mut Context<Self>) + 'static,
    ) {
        let task = self.host.call(NodeId(node.into()), input, cx);
        cx.spawn(async move |page, cx| {
            let answer = task.await;
            let _ = page.update(cx, |page, cx| done(page, answer, cx));
        })
        .detach();
    }
}
