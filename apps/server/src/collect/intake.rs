//! Request bodies under a per-route cap and an installation-wide in-flight byte budget.
//!
//! The cap bounds one request (0005 Request body caps). The budget bounds all requests read
//! but not yet answered, so a flood of slow or large bodies is refused with 429 instead of
//! growing memory. Bodies are counted on received bytes before parsing.

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, header};
use http_body_util::BodyExt;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub const SEND_MAX_BYTES: usize = 64 * 1024;
pub const BATCH_MAX_BYTES: usize = 2 * 1024 * 1024;

pub enum BodyError {
    TooLarge {
        size: u64,
    },
    /// The in-flight budget is spent.
    Busy,
    /// The client stopped sending.
    Aborted,
}

/// A request body together with the budget share it holds until the answer is sent.
pub struct Intake {
    pub bytes: Bytes,
    _permit: OwnedSemaphorePermit,
}

#[derive(Clone)]
pub struct Budget(Arc<Semaphore>);

impl Budget {
    pub fn new(bytes: usize) -> Self {
        Self(Arc::new(Semaphore::new(bytes)))
    }

    /// Reads `body` up to `cap` bytes. A declared `Content-Length` over the cap is refused
    /// before reading; otherwise reading stops at the first byte past the cap.
    pub async fn read(
        &self,
        headers: &HeaderMap,
        body: Body,
        cap: usize,
    ) -> Result<Intake, BodyError> {
        let declared = headers
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        if let Some(size) = declared
            && size > cap as u64
        {
            return Err(BodyError::TooLarge { size });
        }
        let reserve = declared.map_or(cap, |size| size as usize).max(1);
        let permit = self
            .0
            .clone()
            .try_acquire_many_owned(reserve as u32)
            .map_err(|_| BodyError::Busy)?;

        let mut body = body;
        let mut buffer = Vec::with_capacity(reserve.min(cap));
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_| BodyError::Aborted)?;
            if let Ok(data) = frame.into_data() {
                if buffer.len() + data.len() > cap {
                    return Err(BodyError::TooLarge {
                        size: (buffer.len() + data.len()) as u64,
                    });
                }
                buffer.extend_from_slice(&data);
            }
        }
        Ok(Intake {
            bytes: Bytes::from(buffer),
            _permit: permit,
        })
    }
}
