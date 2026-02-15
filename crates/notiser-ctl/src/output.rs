use std::str::FromStr;

#[derive(Debug, Clone)]
pub enum OutputFormat {
    Text,
    Json,
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => Err(format!("unknown format: {s}")),
        }
    }
}
