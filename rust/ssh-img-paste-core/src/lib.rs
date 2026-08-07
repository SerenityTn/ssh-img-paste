//! Contract-first core for Windows and Linux SSH Image Paste editions.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileId(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidProfileId;

impl ProfileId {
    pub fn parse(value: &str) -> Result<Self, InvalidProfileId> {
        let mut chars = value.chars();
        let first = chars.next().ok_or(InvalidProfileId)?;
        if !first.is_ascii_alphanumeric()
            || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(InvalidProfileId);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
