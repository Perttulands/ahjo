//! HTTP authoring routes. Writes publish after the store commits them.
use super::Missions;
use ahjo_mission_engine::{Harnesses, validate};
use ahjo_missions_contract::patch;
use ahjo_missions_contract::{
    EditResponse, LayoutPatch, LayoutResponse, MissionResponse, NotePatch, Precondition,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{
        Path, State,
        rejection::{BytesRejection, JsonRejection},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};

pub fn routes(m: Missions) -> Router {
    Router::new()
        .route("/api/missions", get(list).post(create))
        .route("/api/repositories", get(repositories))
        .route("/api/missions/{id}", get(read).patch(edit).delete(delete))
        .route("/api/missions/{id}/validation", get(validation))
        .route("/api/missions/{id}/notes", get(notes).patch(edit_notes))
        .route("/api/missions/{id}/layout", get(layout).patch(edit_layout))
        .with_state(m)
}
type ResultResponse = Result<Response, ApiError>;
type JsonBody<T> = Result<Json<T>, JsonRejection>;

pub(crate) struct ApiError(StatusCode, String, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(ahjo_missions_contract::ErrorResponse {
                error: ahjo_core::Error::new(self.1, self.2),
            }),
        )
            .into_response()
    }
}
impl From<ahjo_core::Error> for ApiError {
    fn from(e: ahjo_core::Error) -> Self {
        let status = match e.code.as_str() {
            "not_found" | "unknown_run" => StatusCode::NOT_FOUND,
            "note_author_mismatch" => StatusCode::FORBIDDEN,
            "note_too_large" => StatusCode::PAYLOAD_TOO_LARGE,
            "conflict"
            | "already_exists"
            | "input_occupied"
            | "duplicate_connection"
            | "not_pending"
            | "seat_choice_required" => StatusCode::CONFLICT,
            "precondition_required" => StatusCode::PRECONDITION_REQUIRED,
            "invalid_code_gate_profile"
            | "invalid_seat_settings"
            | "invalid_definition_source"
            | "unsupported_schema"
            | "self_wire" => StatusCode::UNPROCESSABLE_ENTITY,
            "io" => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        };
        Self(status, e.code, e.message)
    }
}
fn bad(s: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, "bad_request".into(), s.into())
}
fn ok(value: Value) -> Response {
    Json(value).into_response()
}
fn tagged(value: impl serde::Serialize, etag: &str) -> Response {
    let mut r = Json(value).into_response();
    if let Ok(h) = etag.parse() {
        r.headers_mut().insert(header::ETAG, h);
    }
    r
}
fn etag(h: &HeaderMap) -> &str {
    h.get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
async fn list(State(m): State<Missions>) -> ResultResponse {
    let v = m.store.list_missions()?;
    Ok(ok(json!({"missions":v})))
}
fn slug_from_title(title: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for c in title.trim().to_lowercase().chars() {
        if c.is_alphanumeric() {
            slug.push(c);
            separator = false;
        } else if !slug.is_empty() && !separator {
            slug.push('-');
            separator = true;
        }
        if slug.len() >= 64 {
            break;
        }
    }
    slug.trim_matches('-').to_owned()
}
async fn create(State(m): State<Missions>, input: JsonBody<Value>) -> ResultResponse {
    let Json(v) = input.map_err(|error| bad(error.body_text()))?;
    let title = s(&v, "title").trim();
    let given_slug = s(&v, "slug").trim();
    let unnamed = title.is_empty() && given_slug.is_empty();
    let title = if unnamed { "Untitled mission" } else { title };
    let slug = if given_slug.is_empty() {
        slug_from_title(title)
    } else {
        given_slug.to_owned()
    };
    if slug.is_empty() {
        return Err(bad("Mission name must contain a letter or number"));
    }
    let mut result = None;
    for suffix in 1..=100 {
        let name = if suffix == 1 {
            slug.clone()
        } else {
            format!("{slug}-{suffix}")
        };
        let display = if suffix == 1 {
            title.to_owned()
        } else {
            format!("{title} {suffix}")
        };
        // A mission may start as a copy of an example.
        let created = match s(&v, "example") {
            "" => m.store.create_mission(&name, &display, s(&v, "updated_by")),
            example => m
                .store
                .create_example(&name, &display, example, s(&v, "updated_by")),
        };
        match created {
            Ok(edit) => {
                result = Some(edit);
                break;
            }
            Err(e) if e.code == "already_exists" && unnamed && suffix < 100 => continue,
            Err(e) if e.code == "already_exists" => {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "mission_exists".into(),
                    e.message,
                ));
            }
            Err(e) => return Err(e.into()),
        }
    }
    let result = result.ok_or_else(|| bad("no untitled mission name available"))?;
    // A sketch: rough steps, one per line, and what the mission is for.
    let steps = crate::sketch::steps(s(&v, "steps"));
    let result = if steps.is_empty() && s(&v, "note").trim().is_empty() {
        result
    } else {
        let slug = result.mission.slug.clone();
        crate::sketch::fill(
            &m,
            &result.mission.id,
            &slug,
            &steps,
            s(&v, "note"),
            s(&v, "updated_by"),
        )?;
        m.store.mission_edit(&slug)?
    };
    m.changed();
    let mut r = tagged(
        MissionResponse {
            mission: result.mission,
            etag: Some(result.etag.clone()),
        },
        &result.etag,
    );
    *r.status_mut() = StatusCode::CREATED;
    Ok(r)
}
async fn read(State(m): State<Missions>, Path(id): Path<String>) -> ResultResponse {
    let e = m.store.mission_edit(&id)?;
    Ok(tagged(
        MissionResponse {
            mission: e.mission,
            etag: Some(e.etag.clone()),
        },
        &e.etag,
    ))
}
async fn edit(
    State(m): State<Missions>,
    Path(id): Path<String>,
    h: HeaderMap,
    input: Result<Bytes, BytesRejection>,
) -> ResultResponse {
    let input = input.map_err(|e| bad(e.body_text()))?;
    let patch::UniquePatch(v) = serde_json::from_slice(&input).map_err(|e| bad(format!("{e}")))?;
    let request = patch::decode_request(&v).map_err(bad)?;
    let precondition = Precondition {
        rev: request.rev.unwrap_or_default(),
        etag: etag(&h).into(),
    };
    let before = m.store.mission_edit(&id)?;
    let actor = &request.updated_by;
    let (e, created) = match request.changes {
        patch::Changes::Single(change) => {
            let created = crate::created_card::CreatedCard::from_change(&change);
            (m.store.patch(&id, precondition, *change, actor)?, created)
        }
        patch::Changes::Batch(changes) => (
            m.store.patch_batch(&id, precondition, changes, actor)?,
            None,
        ),
    };
    if e.etag != before.etag {
        m.changed();
    }
    let layout = m.store.layout_edit(&id)?;
    let mut changed = std::collections::BTreeMap::new();
    if let Some(card) = created.and_then(|kind| kind.added(&before.mission, &e.mission)) {
        use ahjo_missions_contract::commands::AddedCard;
        let (key, value) = match card {
            AddedCard::InputCard(card) => ("input_card", json!(card)),
            AddedCard::Step(card) => ("step", json!(card)),
            AddedCard::Gate(card) => ("gate", json!(card)),
            AddedCard::End(card) => ("end", json!(card)),
            AddedCard::Limit(card) => ("limit", json!(card)),
        };
        changed.insert(key.into(), value);
    }
    for (op, key) in [
        ("delete_step", "step_id"),
        ("delete_gate", "gate_id"),
        ("delete_end", "end_id"),
        ("delete_limit", "limit_id"),
        ("delete_input_card", "input_card_id"),
    ] {
        if let Some(id) = v[op]["id"].as_str() {
            changed.insert(key.into(), json!(id));
        }
    }
    Ok(tagged(
        EditResponse {
            mission: e.mission,
            mission_etag: e.etag.clone(),
            layout: layout.layout,
            layout_etag: layout.etag,
            changed,
        },
        &e.etag,
    ))
}
async fn delete(
    State(m): State<Missions>,
    Path(id): Path<String>,
    h: HeaderMap,
    input: JsonBody<Value>,
) -> ResultResponse {
    let Json(v) = input.map_err(|error| bad(error.body_text()))?;
    let e = m.store.delete_mission(
        &id,
        Precondition {
            rev: v["expected_rev"].as_i64().unwrap_or_default(),
            etag: etag(&h).into(),
        },
    )?;
    m.changed();
    // The operator confirmed a delete for good: no safety copy is left to
    // clear by hand. An agent's `mission delete` keeps one, listed by
    // `mission archives`.
    m.store.remove_mission_archive(&e.slug, &e.archive_id)?;
    Ok(ok(
        json!({"deletion":{"id":e.id,"slug":e.slug,"title":e.title,"archive_id":e.archive_id}}),
    ))
}
async fn validation(State(m): State<Missions>, Path(id): Path<String>) -> ResultResponse {
    let e = m.store.mission_edit(&id)?;
    let report = validate(&e.mission, &m.store.agents(), &Harnesses::from_host());
    Ok(tagged(
        json!({"mission_rev":e.mission.rev,"mission_etag":e.etag,"errors":report.errors,"warnings":report.warnings}),
        &e.etag,
    ))
}
fn notes_value(notes: ahjo_missions_contract::Notes, etag: &str) -> Value {
    serde_json::to_value(ahjo_missions_contract::document::NotesDocument::from_notes(
        notes,
        etag.into(),
    ))
    .expect("notes serialize")
}
async fn notes(State(m): State<Missions>, Path(id): Path<String>) -> ResultResponse {
    let e = m.store.notes_edit(&id)?;
    Ok(tagged(
        json!({"notes":notes_value(e.notes,&e.etag)}),
        &e.etag,
    ))
}
async fn edit_notes(
    State(m): State<Missions>,
    Path(id): Path<String>,
    h: HeaderMap,
    input: JsonBody<Value>,
) -> ResultResponse {
    let Json(v) = input.map_err(|error| bad(error.body_text()))?;
    let target = s(&v, "target").into();
    let author = if s(&v, "author").is_empty() {
        "human:ui".into()
    } else {
        s(&v, "author").into()
    };
    let text = s(&v, "text").into();
    let id_entry = s(&v, "entry_id").into();
    let p = match s(&v, "action") {
        "" | "append" => NotePatch::Append {
            target,
            author,
            text,
        },
        "reply" | "resolve" => NotePatch::Reply {
            resolves: s(&v, "action") == "resolve",
            to: id_entry,
            author,
            text,
        },
        "edit" => NotePatch::Edit {
            target,
            author,
            id: id_entry,
            text,
        },
        "delete" => NotePatch::Delete {
            target,
            author,
            id: id_entry,
        },
        _ => return Err(bad("unknown note action")),
    };
    let e = m.store.patch_notes(&id, etag(&h), p)?;
    m.changed();
    Ok(tagged(
        json!({"notes":notes_value(e.notes,&e.etag)}),
        &e.etag,
    ))
}
async fn layout(State(m): State<Missions>, Path(id): Path<String>) -> ResultResponse {
    let e = m.store.layout_edit(&id)?;
    Ok(tagged(
        LayoutResponse {
            layout: e.layout,
            etag: e.etag.clone(),
        },
        &e.etag,
    ))
}
async fn edit_layout(
    State(m): State<Missions>,
    Path(id): Path<String>,
    h: HeaderMap,
    input: JsonBody<Value>,
) -> ResultResponse {
    let Json(v) = input.map_err(|error| bad(error.body_text()))?;
    let p = if v["arrange"].as_bool() == Some(true) {
        let e = m.store.mission_edit(&id)?;
        let l = m.store.layout_edit(&id)?;
        LayoutPatch::Nodes(ahjo_mission_engine::arrange(&e.mission, &l.layout).nodes)
    } else if let Some(nodes) = v.get("nodes") {
        LayoutPatch::Nodes(serde_json::from_value(nodes.clone()).map_err(|e| bad(e.to_string()))?)
    } else if let Some(edges) = v.get("edges") {
        LayoutPatch::Edges(serde_json::from_value(edges.clone()).map_err(|e| bad(e.to_string()))?)
    } else {
        return Err(bad("layout needs nodes, edges or arrange"));
    };
    let e = m.store.patch_layout(&id, etag(&h), p)?;
    m.changed();
    Ok(tagged(
        LayoutResponse {
            layout: e.layout,
            etag: e.etag.clone(),
        },
        &e.etag,
    ))
}

/// The repositories a step may name, as configured on this node.
async fn repositories(State(m): State<Missions>) -> Response {
    Json(json!({"repositories": m.coordinator.repositories()})).into_response()
}

#[cfg(test)]
mod error_tests {
    use super::*;
    #[test]
    fn authoring_errors_keep_their_http_boundaries() {
        for (code, status) in [
            ("seat_choice_required", StatusCode::CONFLICT),
            ("input_occupied", StatusCode::CONFLICT),
            ("duplicate_connection", StatusCode::CONFLICT),
            ("not_found", StatusCode::NOT_FOUND),
            ("note_author_mismatch", StatusCode::FORBIDDEN),
            ("note_too_large", StatusCode::PAYLOAD_TOO_LARGE),
            ("self_wire", StatusCode::UNPROCESSABLE_ENTITY),
        ] {
            assert_eq!(
                ApiError::from(ahjo_core::Error::new(code, "test")).0,
                status,
                "{code}"
            );
        }
    }
}
