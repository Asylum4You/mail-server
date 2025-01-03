/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs Ltd <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::time::SystemTime;

use common::{auth::AccessToken, scripts::ScriptModification, IntoString, Server};
use directory::Permission;
use hyper::Method;
use serde_json::json;
use sieve::{runtime::Variable, Envelope};
use smtp::scripts::{event_loop::RunScript, ScriptParameters, ScriptResult};
use std::future::Future;
use utils::url_params::UrlParams;

use crate::api::{http::ToHttpResponse, HttpRequest, HttpResponse, JsonResponse};

#[derive(Debug, serde::Serialize)]
#[serde(tag = "action")]
#[serde(rename_all = "lowercase")]
pub enum Response {
    Accept {
        modifications: Vec<ScriptModification>,
    },
    Replace {
        message: String,
        modifications: Vec<ScriptModification>,
    },
    Reject {
        reason: String,
    },
    Discard,
}

pub trait SieveHandler: Sync + Send {
    fn handle_run_sieve(
        &self,
        req: &HttpRequest,
        path: Vec<&str>,
        body: Option<Vec<u8>>,
        access_token: &AccessToken,
    ) -> impl Future<Output = trc::Result<HttpResponse>> + Send;
}

impl SieveHandler for Server {
    async fn handle_run_sieve(
        &self,
        req: &HttpRequest,
        path: Vec<&str>,
        _body: Option<Vec<u8>>,
        access_token: &AccessToken,
    ) -> trc::Result<HttpResponse> {
        // Validate the access token
        access_token.assert_has_permission(Permission::SpamFilterTrain)?;

        let (script, script_id) = match (
            path.get(1).and_then(|name| {
                self.core
                    .sieve
                    .trusted_scripts
                    .get(*name)
                    .map(|s| (s.clone(), name.to_string()))
            }),
            req.method(),
        ) {
            (Some(script), &Method::POST) => script,
            _ => {
                return Err(trc::ResourceEvent::NotFound.into_err());
            }
        };

        let mut params = ScriptParameters::new()
            .set_variable(
                "now",
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs()),
            )
            .set_variable("test", true);

        let mut envelope_to = Vec::new();
        for (key, value) in UrlParams::new(req.uri().query()).into_inner() {
            if key.starts_with("env_to") {
                envelope_to.push(Variable::from(value.to_lowercase()));
                continue;
            }
            let env = match key.as_ref() {
                "env_from" => Envelope::From,
                "env_orcpt" => Envelope::Orcpt,
                "env_ret" => Envelope::Ret,
                "env_notify" => Envelope::Notify,
                "env_id" => Envelope::Envid,
                "env_bym" => Envelope::ByMode,
                "env_byt" => Envelope::ByTrace,
                "env_byta" => Envelope::ByTimeAbsolute,
                "env_bytr" => Envelope::ByTimeRelative,
                _ => {
                    params = params.set_variable(key.into_owned(), value.into_owned());
                    continue;
                }
            };

            params = params.set_envelope(env, value);
        }

        if !envelope_to.is_empty() {
            params = params.set_envelope(Envelope::To, Variable::from(envelope_to));
        }

        // Run script
        let result = match self
            .run_script(script_id, script, params.with_access_token(access_token))
            .await
        {
            ScriptResult::Accept { modifications } => Response::Accept { modifications },
            ScriptResult::Replace {
                message,
                modifications,
            } => Response::Replace {
                message: message.into_string(),
                modifications,
            },
            ScriptResult::Reject(reason) => Response::Reject { reason },
            ScriptResult::Discard => Response::Discard,
        };

        Ok(JsonResponse::new(json!({
            "data": result,
        }))
        .into_http_response())
    }
}
