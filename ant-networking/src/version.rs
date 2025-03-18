use std::fmt::Display;

pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl Version {
    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Version {
            major,
            minor,
            patch,
        }
    }

    pub fn is_minimum(&self, other: &Version) -> bool {
        self.major > other.major
            || (self.major == other.major && self.minor > other.minor)
            || (self.major == other.major && self.minor == other.minor && self.patch >= other.patch)
    }

    pub fn is_maximum(&self, other: &Version) -> bool {
        self.major < other.major
            || (self.major == other.major && self.minor < other.minor)
            || (self.major == other.major && self.minor == other.minor && self.patch <= other.patch)
    }

    pub fn is_exact(&self, other: &Version) -> bool {
        self.major == other.major && self.minor == other.minor && self.patch == other.patch
    }
}

impl TryFrom<String> for Version {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let parts: Vec<&str> = value.split('.').collect();
        if parts.len() != 3 {
            return Err("Invalid version format. Expected major.minor.patch".to_string());
        }

        let major = parts[0]
            .parse::<u16>()
            .map_err(|_| "Failed to parse major version as u16".to_string())?;
        let minor = parts[1]
            .parse::<u16>()
            .map_err(|_| "Failed to parse minor version as u16".to_string())?;
        let patch = parts[2]
            .parse::<u16>()
            .map_err(|_| "Failed to parse patch version as u16".to_string())?;

        Ok(Version {
            major,
            minor,
            patch,
        })
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}
