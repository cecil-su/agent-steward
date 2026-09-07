//! Authenticated invalidation notifications, including writes by CLI/Hook processes.
//! A read-only SQLite connection per subscriber tracks data_version without holding a transaction.
use std::{convert::Infallible, time::Duration};

use axum::{
    extract::State,
    http::StatusCode,
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use futures_util::stream;
use rusqlite::{Connection, OpenFlags};
use tokio::sync::OwnedSemaphorePermit;

use crate::{ServerState, failure};

struct Subscription {
    connection: Connection,
    version: i64,
    initial: bool,
    _permit: OwnedSemaphorePermit,
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

pub(crate) async fn subscribe(State(state): State<ServerState>) -> Response {
    let Ok(permit) = state.event_slots.clone().try_acquire_owned() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "SERVER_BUSY",
            "too many event subscribers",
        );
    };
    let path = state.service.database_path().to_owned();
    let opened = tokio::task::spawn_blocking(move || -> rusqlite::Result<_> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(Duration::from_secs(1))?;
        let version = connection.pragma_query_value(None, "data_version", |row| row.get(0))?;
        Ok(Subscription {
            connection,
            version,
            initial: true,
            _permit: permit,
            shutdown: state.event_shutdown,
        })
    })
    .await;
    let Ok(Ok(subscription)) = opened else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "DATABASE_UNAVAILABLE",
            "cannot observe database changes",
        );
    };
    let events = stream::unfold(Some(subscription), |next| async move {
        let mut subscription = next?;
        if subscription.initial {
            subscription.initial = false;
            // A reconnect always requests a fresh snapshot; no replay log or business payload.
            return Some((
                Ok::<_, Infallible>(Event::default().event("changed").data("refresh")),
                Some(subscription),
            ));
        }
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if subscription
                .shutdown
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                return None;
            }
            let read = tokio::task::spawn_blocking(move || {
                let version =
                    subscription
                        .connection
                        .pragma_query_value(None, "data_version", |row| row.get::<_, i64>(0));
                (subscription, version)
            })
            .await;
            match read {
                Ok((mut current, Ok(version))) => {
                    if version != current.version {
                        current.version = version;
                        return Some((
                            Ok(Event::default().event("changed").data("refresh")),
                            Some(current),
                        ));
                    }
                    subscription = current;
                }
                _ => {
                    return Some((
                        Ok(Event::default().event("unavailable").data("reconnect")),
                        None,
                    ));
                }
            }
        }
    });
    Sse::new(events)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}
