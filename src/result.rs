use serde::de::DeserializeOwned;
use serde::ser::Error;
use serde_json::Value;

pub struct Void;

pub trait ScriptResultDeserialize: Sized {
    fn from_string(script_result: Option<String>) -> Result<Self, serde_json::Error>;
    fn from_value(script_result: Option<Value>) -> Result<Self, serde_json::Error>;
}

impl<T: DeserializeOwned> ScriptResultDeserialize for T {
    fn from_string(script_result: Option<String>) -> Result<Self, serde_json::Error> {
        match script_result {
            Some(string) => serde_json::from_str(&string),
            None => Err(serde_json::Error::custom("Missing script result")),
        }
    }

    fn from_value(script_result: Option<Value>) -> Result<Self, serde_json::Error> {
        match script_result {
            Some(value) => serde_json::from_value(value),
            None => Err(serde_json::Error::custom("Missing script result")),
        }
    }
}

impl ScriptResultDeserialize for Void {
    fn from_string(_script_result: Option<String>) -> Result<Self, serde_json::Error> {
        Ok(Void)
    }

    fn from_value(_script_result: Option<Value>) -> Result<Self, serde_json::Error> {
        Ok(Void)
    }
}
