/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs Ltd <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use common::{
    core::BuildServer,
    ipc::{OnHold, QueueEvent, QueueEventLock},
    Inner,
};
use store::write::now;
use tokio::sync::mpsc;

use super::{spool::SmtpSpool, DeliveryAttempt, Message, Status};

pub(crate) const SHORT_WAIT: Duration = Duration::from_millis(1);
pub(crate) const LONG_WAIT: Duration = Duration::from_secs(86400 * 365);

pub struct Queue {
    pub core: Arc<Inner>,
    pub on_hold: Vec<OnHold<QueueEventLock>>,
    pub next_wake_up: Duration,
}

impl SpawnQueue for mpsc::Receiver<QueueEvent> {
    fn spawn(mut self, core: Arc<Inner>) {
        tokio::spawn(async move {
            let mut queue = Queue::new(core);

            loop {
                let on_hold = match tokio::time::timeout(queue.next_wake_up, self.recv()).await {
                    Ok(Some(QueueEvent::OnHold(on_hold))) => on_hold.into(),
                    Ok(Some(QueueEvent::Stop)) | Ok(None) => {
                        break;
                    }
                    _ => None,
                };

                queue.process_events().await;

                // Add message on hold
                if let Some(on_hold) = on_hold {
                    queue.on_hold(on_hold);
                }
            }
        });
    }
}

impl Queue {
    pub fn new(core: Arc<Inner>) -> Self {
        Queue {
            core,
            on_hold: Vec::with_capacity(128),
            next_wake_up: SHORT_WAIT,
        }
    }

    pub async fn process_events(&mut self) {
        // Deliver any concurrency limited messages
        let server = self.core.build_server();
        while let Some(queue_event) = self.next_on_hold() {
            DeliveryAttempt::new(queue_event)
                .try_deliver(server.clone())
                .await;
        }

        // Deliver scheduled messages
        let now = now();
        self.next_wake_up = LONG_WAIT;
        for queue_event in server.next_event().await {
            if queue_event.due <= now {
                DeliveryAttempt::new(queue_event)
                    .try_deliver(server.clone())
                    .await;
            } else {
                self.next_wake_up = Duration::from_secs(queue_event.due - now);
            }
        }
    }

    pub fn on_hold(&mut self, message: OnHold<QueueEventLock>) {
        self.on_hold.push(OnHold {
            next_due: message.next_due,
            limiters: message.limiters,
            message: message.message,
        });
    }

    pub fn next_on_hold(&mut self) -> Option<QueueEventLock> {
        let now = now();
        self.on_hold
            .iter()
            .position(|o| {
                o.limiters
                    .iter()
                    .any(|l| l.concurrent.load(Ordering::Relaxed) < l.max_concurrent)
                    || o.next_due.map_or(false, |due| due <= now)
            })
            .map(|pos| self.on_hold.remove(pos).message)
    }
}

impl Message {
    pub fn next_event(&self) -> Option<u64> {
        let mut next_event = now();
        let mut has_events = false;

        for domain in &self.domains {
            if matches!(
                domain.status,
                Status::Scheduled | Status::TemporaryFailure(_)
            ) {
                if !has_events || domain.retry.due < next_event {
                    next_event = domain.retry.due;
                    has_events = true;
                }
                if domain.notify.due < next_event {
                    next_event = domain.notify.due;
                }
                if domain.expires < next_event {
                    next_event = domain.expires;
                }
            }
        }

        if has_events {
            next_event.into()
        } else {
            None
        }
    }

    pub fn next_delivery_event(&self) -> u64 {
        let mut next_delivery = now();

        for (pos, domain) in self
            .domains
            .iter()
            .filter(|d| matches!(d.status, Status::Scheduled | Status::TemporaryFailure(_)))
            .enumerate()
        {
            if pos == 0 || domain.retry.due < next_delivery {
                next_delivery = domain.retry.due;
            }
        }

        next_delivery
    }

    pub fn next_dsn(&self) -> u64 {
        let mut next_dsn = now();

        for (pos, domain) in self
            .domains
            .iter()
            .filter(|d| matches!(d.status, Status::Scheduled | Status::TemporaryFailure(_)))
            .enumerate()
        {
            if pos == 0 || domain.notify.due < next_dsn {
                next_dsn = domain.notify.due;
            }
        }

        next_dsn
    }

    pub fn expires(&self) -> u64 {
        let mut expires = now();

        for (pos, domain) in self
            .domains
            .iter()
            .filter(|d| matches!(d.status, Status::Scheduled | Status::TemporaryFailure(_)))
            .enumerate()
        {
            if pos == 0 || domain.expires < expires {
                expires = domain.expires;
            }
        }

        expires
    }

    pub fn next_event_after(&self, instant: u64) -> Option<u64> {
        let mut next_event = None;

        for domain in &self.domains {
            if matches!(
                domain.status,
                Status::Scheduled | Status::TemporaryFailure(_)
            ) {
                if domain.retry.due > instant
                    && next_event
                        .as_ref()
                        .map_or(true, |ne| domain.retry.due.lt(ne))
                {
                    next_event = domain.retry.due.into();
                }
                if domain.notify.due > instant
                    && next_event
                        .as_ref()
                        .map_or(true, |ne| domain.notify.due.lt(ne))
                {
                    next_event = domain.notify.due.into();
                }
                if domain.expires > instant
                    && next_event.as_ref().map_or(true, |ne| domain.expires.lt(ne))
                {
                    next_event = domain.expires.into();
                }
            }
        }

        next_event
    }
}

pub trait SpawnQueue {
    fn spawn(self, core: Arc<Inner>);
}
