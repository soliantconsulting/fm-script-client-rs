use crate::{Connection, Error, FileMakerError, ScriptClient};
use async_trait::async_trait;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use url::Url;

/// OData API script client.
///
/// The OData API script client is the currently preferred method to issue script calls against
/// FileMaker. If you are unable to utilize the OData API, you can fall back to the
/// [`crate::data_api::DataApiScriptClient`].
pub struct ODataApiScriptClient {
    connection: Arc<Connection>,
    client: Client,
}

impl ODataApiScriptClient {
    /// Creates a new OData API script client.
    ///
    /// # Examples
    ///
    /// ```
    /// use fm_script_client::Connection;
    /// use fm_script_client::odata_api::ODataApiScriptClient;
    ///
    /// let client = ODataApiScriptClient::new(
    ///     "https://foo:bar@example.com/example_database".try_into().unwrap(),
    /// );
    /// ```
    pub fn new(connection: Connection) -> Self {
        Self {
            connection: Arc::new(connection),
            client: Client::new(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestBody<T> {
    #[serde(skip_serializing_if = "Option::is_none")]
    script_parameter_value: Option<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScriptResult {
    code: i64,
    result_parameter: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResponseBody {
    script_result: ScriptResult,
}

#[derive(Debug, Deserialize)]
struct ErrorResponseBody {
    error: FileMakerError,
}

#[async_trait]
impl ScriptClient for ODataApiScriptClient {
    async fn execute<T: DeserializeOwned, P: Serialize + Send + Sync>(
        &self,
        script_name: impl Into<String> + Send,
        parameter: Option<P>,
    ) -> Result<T, Error> {
        let mut url = Url::parse(&format!(
            "{}://{}/fmi/odata/v4/{}/Script.{}",
            if self.connection.disable_tls {
                "http"
            } else {
                "https"
            },
            self.connection.hostname,
            self.connection.database,
            script_name.into(),
        ))?;

        if let Some(port) = self.connection.port {
            let _ = url.set_port(Some(port));
        }

        let body = RequestBody {
            script_parameter_value: parameter,
        };

        let response = self
            .client
            .post(url)
            .basic_auth(&self.connection.username, Some(&self.connection.password))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();

        if status.is_success() {
            let result: ResponseBody = response.json().await?;

            if result.script_result.code != 0 {
                return Err(Error::ScriptFailure {
                    code: result.script_result.code,
                    data: result.script_result.result_parameter.to_string(),
                });
            }

            let result: T = serde_json::from_value(result.script_result.result_parameter)?;
            return Ok(result);
        }

        match response.json::<ErrorResponseBody>().await {
            Ok(result) => Err(Error::FileMaker(result.error)),
            Err(_) => Err(Error::UnknownResponse(status)),
        }
    }
}
