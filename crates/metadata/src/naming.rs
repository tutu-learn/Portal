use error::Result;

#[derive(Debug, Clone)]
pub struct NamingSeries;

impl NamingSeries {
    pub fn next(_series: &str) -> Result<String> {
        // TODO: implement naming series counter
        Ok(format!(
            "{}-{}",
            _series,
            uuid::Uuid::new_v4().to_string().split('-').next().unwrap()
        ))
    }
}
