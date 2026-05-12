use std::{convert::Infallible, str::FromStr};

use crate::COUNTER;

/// An unique id that is associated with a tray icon.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrayIconId(pub String);

impl TrayIconId {
    /// Create a new tray icon id.
    pub fn new<S: AsRef<str>>(id: S) -> Self {
        Self(id.as_ref().to_string())
    }

    /// Generate id based on process id for internal use
    pub(crate) fn new_unique() -> Self {
        Self(format!("{}-{}", std::process::id(), COUNTER.next()))
    }
}

impl AsRef<str> for TrayIconId {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl<T: ToString> From<T> for TrayIconId {
    fn from(value: T) -> Self {
        Self::new(value.to_string())
    }
}

impl FromStr for TrayIconId {
    type Err = Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

impl PartialEq<&str> for TrayIconId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<&str> for &TrayIconId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for TrayIconId {
    fn eq(&self, other: &String) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for &TrayIconId {
    fn eq(&self, other: &String) -> bool {
        self.0 == *other
    }
}

impl PartialEq<&String> for TrayIconId {
    fn eq(&self, other: &&String) -> bool {
        self.0 == **other
    }
}

impl PartialEq<&TrayIconId> for TrayIconId {
    fn eq(&self, other: &&TrayIconId) -> bool {
        other.0 == self.0
    }
}

#[cfg(test)]
mod test {
    use crate::TrayIconId;

    #[test]
    fn is_eq() {
        assert_eq!(TrayIconId::new("t"), "t",);
        assert_eq!(TrayIconId::new("t"), String::from("t"));
        assert_eq!(TrayIconId::new("t"), &String::from("t"));
        assert_eq!(TrayIconId::new("t"), TrayIconId::new("t"));
        assert_eq!(TrayIconId::new("t"), &TrayIconId::new("t"));
        assert_eq!(&TrayIconId::new("t"), &TrayIconId::new("t"));
        assert_eq!(TrayIconId::new("t").as_ref(), "t");
    }
}
