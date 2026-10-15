//! `GET /topics`: total, stable ordering, filtering by name and key pagination
//! (doc 06a, section 8.1.2). The order always ends in a unique field (the `id`), and the cursor
//! holds the last key read, so inserting or removing items does not shift the page.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use super::rules::ListQuery;
use super::{TopicView, Topics};
use crate::core::error::{ErrorCode, TabaError};
use crate::core::pagination;

/// An item's sort key: name, `id` and creation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Key {
    n: String,
    i: String,
    c: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Cursor {
    /// The last key read.
    k: Key,
    /// The `sort` and filter that generated the cursor: with others, it is refused.
    s: String,
    f: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Id,
    CreatedAt,
}

#[derive(Debug, Clone, Copy)]
struct Sort {
    field: Field,
    descending: bool,
}

impl Sort {
    fn parse(raw: Option<&str>) -> Result<Self, TabaError> {
        let raw = raw.unwrap_or("name");
        let (descending, name) = match raw.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, raw),
        };
        let field = match name {
            "name" => Field::Name,
            "id" => Field::Id,
            "createdAt" => Field::CreatedAt,
            _ => {
                return Err(TabaError::new(
                    ErrorCode::ValidationFailed,
                    "The request has invalid fields.",
                )
                .with_detail(
                    "query.sort",
                    ErrorCode::ValidationFailed,
                    "Use name, id or createdAt, with `-` in front for descending.",
                ));
            }
        };
        Ok(Self { field, descending })
    }

    fn canonical(&self) -> String {
        let name = match self.field {
            Field::Name => "name",
            Field::Id => "id",
            Field::CreatedAt => "createdAt",
        };
        format!("{}{name}", if self.descending { "-" } else { "" })
    }

    /// `sort=-createdAt` means `(createdAt desc, id asc)`: the tie-break is always the ascending
    /// `id`.
    fn compare(&self, a: &Key, b: &Key) -> Ordering {
        let primary = match self.field {
            Field::Name => a.n.cmp(&b.n),
            Field::Id => a.i.cmp(&b.i),
            Field::CreatedAt => a.c.cmp(&b.c),
        };
        let primary = if self.descending {
            primary.reverse()
        } else {
            primary
        };
        primary
            .then_with(|| a.i.cmp(&b.i))
            .then_with(|| a.n.cmp(&b.n))
    }
}

pub struct ListResult {
    pub items: Vec<TopicView>,
    pub next_cursor: Option<String>,
}

fn stale_cursor() -> TabaError {
    TabaError::new(
        ErrorCode::ValidationInvalidCursor,
        "The cursor does not apply to this `sort` and filter.",
    )
    .with_detail(
        "cursor",
        ErrorCode::ValidationInvalidCursor,
        "Cursor generated with another `sort` or filter.",
    )
}

impl Topics {
    /// All the topics, in name order, with the dead-letter queues. It is what the topic metrics
    /// walk through (doc 11): they filter and paginate on their own, after measuring.
    pub fn all_views(&self) -> Vec<TopicView> {
        let mut views = Vec::new();
        for definition in self.read().values() {
            views.push(self.view(definition));
            if definition.queue().is_some() {
                views.push(self.dead_letter_view(definition));
            }
        }
        views.sort_by(|a, b| a.definition.name.cmp(&b.definition.name));
        views
    }

    /// Answered locally: every topic's definition is on every node. Includes the
    /// dead-letter queues.
    pub fn list(&self, query: ListQuery) -> Result<ListResult, TabaError> {
        let limit = pagination::parse_limit(
            query.limit.as_deref(),
            self.rules.default_limit,
            self.rules.max_limit,
        )?;
        let sort = Sort::parse(query.sort.as_deref())?;
        let cursor = query
            .cursor
            .as_deref()
            .map(pagination::decode_cursor::<Cursor>)
            .transpose()?;
        if let Some(c) = &cursor
            && (c.s != sort.canonical() || c.f != query.name)
        {
            return Err(stale_cursor());
        }

        let mut items: Vec<(Key, TopicView)> = Vec::new();
        for definition in self.read().values() {
            let mut views = vec![self.view(definition)];
            if definition.queue().is_some() {
                views.push(self.dead_letter_view(definition));
            }
            for view in views {
                let key = Key {
                    n: view.definition.name.clone(),
                    i: view.definition.id.clone(),
                    c: view.definition.created_ms,
                };
                items.push((key, view));
            }
        }
        if !query.scope.is_empty() {
            items.retain(|(key, _)| crate::core::token::reaches(&query.scope, &key.n));
        }
        if let Some(filter) = &query.name {
            items.retain(|(key, _)| key.n.contains(filter.as_str()));
        }
        items.sort_by(|a, b| sort.compare(&a.0, &b.0));
        if let Some(c) = &cursor {
            items.retain(|(key, _)| sort.compare(key, &c.k) == Ordering::Greater);
        }

        let has_more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = match (has_more, items.last()) {
            (true, Some((key, _))) => Some(pagination::encode_cursor(&Cursor {
                k: key.clone(),
                s: sort.canonical(),
                f: query.name.clone(),
            })),
            _ => None,
        };
        Ok(ListResult {
            items: items.into_iter().map(|(_, view)| view).collect(),
            next_cursor,
        })
    }
}
