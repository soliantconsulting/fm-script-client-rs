use async_trait::async_trait;
use percent_encoding::percent_decode_str;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

pub mod data_api;
pub mod odata_api;

#[async_trait]
pub trait ScriptClient {
    /// Executes a script with an optional parameter.
    ///
    /// Parameters must be serializable and results deserializable through `serde`.
    ///
    /// # Examples
    ///
    /// ```
    /// use fm_script_client::{ScriptClient, Connection, odata_api::ODataApiScriptClient};
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize)]
    /// struct Result {
    ///     success: bool,
    /// }
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     # let mut server = mockito::Server::new_async().await;
    ///     # #[cfg(not(doc))]
    ///     # let connection: Connection = format!(
    ///     #     "http://foo:bar@{}/test",
    ///     #     server.host_with_port()
    ///     # ).as_str().try_into().unwrap();
    ///     # let mock = server
    ///     #     .mock("POST", "/fmi/odata/v4/test/Script.my_script")
    ///     #     .match_header("content-length", "36")
    ///     #     .with_body(serde_json::json!({
    ///     #         "scriptResult": {
    ///     #             "code": 0,
    ///     #             "resultParameter": {"success": true},
    ///     #         },
    ///     #     }).to_string())
    ///     #     .create_async()
    ///     #     .await;
    ///     # #[cfg(doc)]
    ///     let connection: Connection = "http://foo:bar@localhost:9999/test"
    ///         .try_into()
    ///         .unwrap();
    ///
    ///     let client = ODataApiScriptClient::new(connection);
    ///     let result: Result = client.execute("my_script", Some("parameter")).await.unwrap();
    ///     assert_eq!(result.success, true);
    /// }
    /// ```
    async fn execute<T: DeserializeOwned, P: Serialize + Send + Sync>(
        &self,
        script_name: impl Into<String> + Send,
        parameter: Option<P>,
    ) -> Result<T, Error>;

    /// Convenience method to execute a script without a parameter.
    async fn execute_without_parameter<T: DeserializeOwned>(
        &self,
        script_name: &str,
    ) -> Result<T, Error> {
        self.execute::<T, ()>(script_name, None).await
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to parse URL")]
    Url(#[from] url::ParseError),

    #[error("Failed to perform request")]
    Request(#[from] reqwest::Error),

    #[error("Failed to (de)serialize JSON")]
    SerdeJson(#[from] serde_json::Error),

    #[error("FileMaker returned an error")]
    FileMaker(FileMakerError),

    #[error("FileMaker script returned an error")]
    ScriptFailure { code: i64, data: String },

    #[error("FileMaker did not respond with an access token")]
    MissingAccessToken,

    #[error("Received an unknown response")]
    UnknownResponse(StatusCode),

    #[error("Invalid connection URL")]
    InvalidConnectionUrl,
}

#[derive(Debug, Deserialize)]
pub struct FileMakerError {
    pub code: String,
    pub message: String,
}

/// Connection details for script clients.
///
/// Defines the credentials, hostname and database to connect to.
#[derive(Debug, Clone)]
pub struct Connection {
    hostname: String,
    database: String,
    username: String,
    password: String,
    port: Option<u16>,
    disable_tls: bool,
}

impl Connection {
    /// Creates a new connection.
    ///
    /// Will use the HTTPS by default, unless changes.
    ///
    /// # Examples
    ///
    /// ```
    /// use fm_script_client::Connection;
    ///
    /// let connection = Connection::new("example.com", "test_sb", "foo", "bar");
    /// ```
    pub fn new(
        hostname: impl Into<String>,
        database: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Connection {
        Self {
            hostname: hostname.into(),
            database: database.into(),
            username: username.into(),
            password: password.into(),
            port: None,
            disable_tls: false,
        }
    }

    /// Configures an alternative port to use.
    pub fn with_port(mut self, port: Option<u16>) -> Self {
        self.port = port;
        self
    }

    /// Disables TLS which forces the client to fall back to HTTP.
    pub fn without_tls(mut self, disable_tls: bool) -> Self {
        self.disable_tls = disable_tls;
        self
    }
}

impl TryFrom<Url> for Connection {
    type Error = Error;

    /// Converts a [`Url`] into a [`Connection`].
    ///
    /// URLs must contain a hostname, username and password, as well as a database as the path
    /// portion.
    ///
    /// # Examples
    ///
    /// ```
    /// use fm_script_client::Connection;
    /// use url::Url;
    ///
    /// let connection: Connection = Url::parse("https://username:password@example.com/database")
    ///     .unwrap()
    ///     .try_into()
    ///     .unwrap();
    /// ```
    fn try_from(url: Url) -> Result<Self, Self::Error> {
        let decode = |value: &str| -> Result<String, Error> {
            Ok(percent_decode_str(value)
                .decode_utf8()
                .map_err(|_| Error::InvalidConnectionUrl)?
                .to_string())
        };

        Ok(Connection {
            hostname: decode(url.host_str().ok_or_else(|| Error::InvalidConnectionUrl)?)?,
            database: decode(&url.path()[1..])?,
            username: decode(url.username())?,
            password: decode(url.password().ok_or_else(|| Error::InvalidConnectionUrl)?)?,
            port: url.port(),
            disable_tls: url.scheme() == "http",
        })
    }
}

impl TryFrom<&str> for Connection {
    type Error = Error;

    /// Converts a `&str` into a [`Connection`].
    ///
    /// Connection strings must follow this format:
    ///
    /// `https://username:password@example.com/database`
    ///
    /// # Examples
    ///
    /// ```
    /// use fm_script_client::Connection;
    ///
    /// let connection: Connection = "https://username:password@example.com/database"
    ///     .try_into()
    ///     .unwrap();
    /// ```
    fn try_from(url: &str) -> Result<Self, Self::Error> {
        Url::parse(url)?.try_into()
    }
}

impl TryFrom<String> for Connection {
    type Error = Error;

    /// Converts a `String` into a [`Connection`].
    ///
    /// Connection strings must follow this format:
    ///
    /// `https://username:password@example.com/database`
    ///
    /// # Examples
    ///
    /// ```
    /// use fm_script_client::Connection;
    ///
    /// let connection: Connection = "https://username:password@example.com/database".to_string()
    ///     .try_into()
    ///     .unwrap();
    /// ```
    fn try_from(url: String) -> Result<Self, Self::Error> {
        Url::parse(&url)?.try_into()
    }
}
