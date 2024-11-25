use crate::{Connection, Error, FileMakerError, ScriptClient};
use async_trait::async_trait;
use reqwest::{Client, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use url::Url;

/// Context for Data API script execution.
///
/// Scripts within the Data API always have to be executed together with another request. While it
/// is possible to execute a script standalone through a `GET` request, it is highly advised to not
/// do that since it is both length restricted and exposing any sent data in server logs.
///
/// The script context defines a `find` on a given layout which will be used as primary request, on
/// which the actual script execution will be added on top. It is important that this `find`
/// succeeds, otherwise a FileMaker error will be thrown.
///
/// Ideally you just create simple layout with a single field and a single record.
pub struct ScriptLayoutContext {
    layout: String,
    search_field: String,
    search_value: String,
}

impl ScriptLayoutContext {
    /// Creates a new script layout context.
    ///
    /// # Examples
    ///
    /// ```
    /// use fm_script_client::data_api::ScriptLayoutContext;
    ///
    /// let context = ScriptLayoutContext::new(
    ///     "script_layout",
    ///     "id",
    ///     "1",
    /// );
    /// ```
    pub fn new(layout: &str, search_field: &str, search_value: &str) -> Self {
        Self {
            layout: layout.to_string(),
            search_field: search_field.to_string(),
            search_value: search_value.to_string(),
        }
    }
}

/// Data API script client.
///
/// The Data API script client should only be used if the OData API is not available or cannot be
/// used for other reasons. Otherwise, you should use the
/// [`crate::odata_api::ODataApiScriptClient`].
///
/// When using the Data API client, you must specify a [`ScriptLayoutContext`] in order to execute
/// script calls. See its documentation for further details.
pub struct DataApiScriptClient {
    connection: Arc<Connection>,
    context: Arc<ScriptLayoutContext>,
    client: Client,
    token: Mutex<Option<Token>>,
}

impl DataApiScriptClient {
    /// Creates a new Data API script client.
    ///
    /// # Examples
    ///
    /// ```
    /// use fm_script_client::Connection;
    /// use fm_script_client::data_api::{DataApiScriptClient, ScriptLayoutContext};
    ///
    /// let client = DataApiScriptClient::new(
    ///     "https://foo:bar@example.com/example_database".try_into().unwrap(),
    ///     ScriptLayoutContext::new("script_layout", "id", "1"),
    /// );
    /// ```
    pub fn new(connection: Connection, context: ScriptLayoutContext) -> Self {
        Self {
            connection: Arc::new(connection),
            context: Arc::new(context),
            client: Client::new(),
            token: Mutex::new(None),
        }
    }

    /// Releases the currently used token.
    ///
    /// If the client has no token registered at the moment, it will return immediately. Otherwise,
    /// it will issue a `DELETE` against the FileMaker Data API and forget the token.
    pub async fn release_token(&self) -> Result<(), Error> {
        let token = match self.token.lock().await.take() {
            Some(token) => token,
            None => return Ok(()),
        };

        let url = self.create_url(&format!("/sessions/{}", token.token))?;
        self.client.delete(url).send().await?;

        Ok(())
    }

    async fn get_token(&self) -> Result<String, Error> {
        let mut token = self.token.lock().await;
        let now = Instant::now();

        if let Some(ref mut token) = *token {
            token.expiry = now + Duration::from_secs(60 * 14);

            if token.expiry < now {
                return Ok(token.token.clone());
            }
        }

        let url = self.create_url("/sessions")?;
        let response = self
            .client
            .post(url)
            .basic_auth(&self.connection.username, Some(&self.connection.password))
            .send()
            .await?;

        if response.status().is_success() {
            let access_token = match response.headers().get("X-FM-Data-Access-Token") {
                Some(token) => match token.to_str() {
                    Ok(token) => token.to_string(),
                    Err(_) => return Err(Error::MissingAccessToken),
                },
                None => return Err(Error::MissingAccessToken),
            };

            *token = Some(Token {
                token: access_token.clone(),
                expiry: now + Duration::from_secs(60 * 14),
            });

            return Ok(access_token);
        }

        Err(self.error_from_response(response).await)
    }

    async fn error_from_response(&self, response: Response) -> Error {
        let status = response.status();

        match response.json::<ErrorResponseBody>().await {
            Ok(result) => {
                if let Some(error) = result.messages.into_iter().next() {
                    Error::FileMaker(error)
                } else {
                    Error::UnknownResponse(status)
                }
            }
            Err(_) => Error::UnknownResponse(status),
        }
    }

    fn create_url(&self, path: &str) -> Result<Url, Error> {
        let mut url = Url::parse(&format!(
            "{}://{}/fmi/data/v1/databases/{}{}",
            if self.connection.disable_tls {
                "http"
            } else {
                "https"
            },
            self.connection.hostname,
            self.connection.database,
            path
        ))?;

        if let Some(port) = self.connection.port {
            let _ = url.set_port(Some(port));
        }

        Ok(url)
    }
}

#[derive(Debug)]
struct Token {
    token: String,
    expiry: Instant,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestBody<T> {
    query: HashMap<String, String>,
    limit: u8,
    script: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "script.param")]
    script_param: Option<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResponseBody {
    script_result: String,
    script_error: String,
}

#[derive(Debug, Deserialize)]
struct ErrorResponseBody {
    messages: Vec<FileMakerError>,
}

#[async_trait]
impl ScriptClient for DataApiScriptClient {
    async fn execute<T: DeserializeOwned, P: Serialize + Send + Sync>(
        &self,
        script_name: &str,
        parameter: Option<P>,
    ) -> Result<T, Error> {
        let token = self.get_token().await?;
        let url = self.create_url(&format!("/layouts/{}/_find", self.context.layout))?;

        let mut query = HashMap::new();
        query.insert(
            self.context.search_field.clone(),
            self.context.search_value.clone(),
        );

        let body = RequestBody {
            query,
            limit: 1,
            script: script_name.to_string(),
            script_param: Some(serde_json::to_string(&parameter)?),
        };

        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", &token))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();

        if status.is_success() {
            let result: ResponseBody = response.json().await?;

            if result.script_error != "0" {
                return Err(Error::ScriptFailure {
                    code: result.script_error.parse().unwrap_or(-1),
                    data: result.script_result,
                });
            }

            let result: T = serde_json::from_str(&result.script_result)?;
            return Ok(result);
        }

        Err(self.error_from_response(response).await)
    }
}
