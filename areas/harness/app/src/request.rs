//! Requests shared by the harness and skills views.
use crate::HarnessPage;
use ahjo_app_kit::link::http::ANSWER;
use gpui::Context;

pub(crate) fn response<T: serde::de::DeserializeOwned>(
    answer: Result<(u16, Vec<u8>), ahjo_core::Error>,
) -> Result<T, String> {
    match answer {
        Ok((status, body)) if (200..300).contains(&status) => {
            serde_json::from_slice(&body).map_err(|e| e.to_string())
        }
        Ok((status, body)) => Err(ahjo_app_kit::host::Answer { status, body }.error_text()),
        Err(e) => Err(e.to_string()),
    }
}
impl HarnessPage {
    pub(crate) fn harness_request(
        &mut self,
        node: &str,
        method: &'static str,
        path: &str,
        body: Option<serde_json::Value>,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<(u16, Vec<u8>), ahjo_core::Error>, &mut Context<Self>)
        + 'static,
    ) {
        let mut req =
            ahjo_app_kit::host::Request::new(method, ahjo_core::NodeId(node.into()), path)
                .within(ANSWER);
        if let Some(body) = body {
            req = req.json(&body);
        }
        let task = self.host.request(req, cx);
        cx.spawn(async move |page, cx| {
            let answer = task.await.map(|a| (a.status, a.body));
            let _ = page.update(cx, |page, cx| done(page, answer, cx));
        })
        .detach();
    }
}
