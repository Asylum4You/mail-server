/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs Ltd <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::{
    net::Ipv4Addr,
    time::{Duration, Instant},
};

use common::{config::spamfilter::DnsBlServer, expr::functions::ResolveVariable, Server};
use mail_auth::{common::resolver::IntoFqdn, Error};
use trc::SpamEvent;

use crate::modules::expression::IpResolver;

use super::expression::SpamFilterResolver;

pub(crate) async fn is_dnsbl(
    server: &Server,
    config: &DnsBlServer,
    resolver: SpamFilterResolver<'_, impl ResolveVariable>,
) -> Option<String> {
    let time = Instant::now();
    let zone = server
        .eval_if::<String, _>(&config.zone, &resolver, resolver.ctx.input.span_id)
        .await?;

    #[cfg(feature = "test_mode")]
    {
        if zone.contains(".11.20.") {
            let parts = zone.split('.').collect::<Vec<_>>();

            return if config.tags.if_then.iter().any(|i| i.expr.items.len() == 3) && parts[0] != "2"
            {
                None
            } else {
                server
                    .eval_if(
                        &config.tags,
                        &SpamFilterResolver::new(
                            resolver.ctx,
                            &IpResolver::new(
                                format!("127.0.{}.{}", parts[1], parts[0]).parse().unwrap(),
                            ),
                            resolver.location,
                        ),
                        resolver.ctx.input.span_id,
                    )
                    .await
            };
        }
    }

    let result = match server.inner.cache.dns_rbl.get(&zone) {
        Some(Some(result)) => result,
        Some(None) => return None,
        None => {
            match server
                .core
                .smtp
                .resolvers
                .dns
                .ipv4_lookup_raw((&zone).into_fqdn().as_ref())
                .await
            {
                Ok(result) => {
                    trc::event!(
                        Spam(SpamEvent::Dnsbl),
                        Hostname = zone.clone(),
                        Result = result
                            .entry
                            .iter()
                            .map(|ip| trc::Value::from(ip.to_string()))
                            .collect::<Vec<_>>(),
                        Elapsed = time.elapsed()
                    );

                    server.inner.cache.dns_rbl.insert_with_expiry(
                        zone,
                        Some(result.entry.clone()),
                        result.expires,
                    );

                    result.entry
                }
                Err(Error::DnsRecordNotFound(_)) => {
                    trc::event!(
                        Spam(SpamEvent::Dnsbl),
                        Hostname = zone.clone(),
                        Result = trc::Value::None,
                        Elapsed = time.elapsed()
                    );

                    server
                        .inner
                        .cache
                        .dns_rbl
                        .insert(zone, None, Duration::from_secs(86400));

                    return None;
                }
                Err(err) => {
                    trc::event!(
                        Spam(SpamEvent::DnsblError),
                        Hostname = zone,
                        Elapsed = time.elapsed(),
                        CausedBy = err.to_string()
                    );

                    return None;
                }
            }
        }
    };

    server
        .eval_if(
            &config.tags,
            &SpamFilterResolver::new(
                resolver.ctx,
                &IpResolver::new(
                    result
                        .iter()
                        .copied()
                        .next()
                        .unwrap_or(Ipv4Addr::BROADCAST)
                        .into(),
                ),
                resolver.location,
            ),
            resolver.ctx.input.span_id,
        )
        .await
}
