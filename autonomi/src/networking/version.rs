use std::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageVersion {
    /// Release year.
    pub year: u16,
    /// Release month.
    pub month: u16,
    /// Increments per month.
    pub cycle: u16,
    /// Increments per cycle.
    pub cycle_counter: u16,
}

impl PackageVersion {
    pub fn new(year: u16, month: u16, cycle: u16, cycle_counter: u16) -> Self {
        PackageVersion {
            year,
            month,
            cycle,
            cycle_counter,
        }
    }

    pub fn is_minimum(&self, other: &PackageVersion) -> bool {
        (self.year, self.month, self.cycle, self.cycle_counter)
            >= (other.year, other.month, other.cycle, other.cycle_counter)
    }

    pub fn is_maximum(&self, other: &PackageVersion) -> bool {
        (self.year, self.month, self.cycle, self.cycle_counter)
            <= (other.year, other.month, other.cycle, other.cycle_counter)
    }

    pub fn is_exact(&self, other: &PackageVersion) -> bool {
        self.year == other.year
            && self.month == other.month
            && self.cycle == other.cycle
            && self.cycle_counter == other.cycle_counter
    }
}

impl TryFrom<String> for PackageVersion {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let parts: Vec<&str> = value.split('.').collect();
        if parts.len() < 3 || parts.len() > 4 {
            return Err(
                "Invalid version format. Expected year.month.cycle[.cycle_counter]".to_string(),
            );
        }

        let year = parts[0]
            .parse::<u16>()
            .map_err(|_| "Failed to parse year as u16".to_string())?;
        let month = parts[1]
            .parse::<u16>()
            .map_err(|_| "Failed to parse month version as u16".to_string())?;
        let cycle = parts[2]
            .parse::<u16>()
            .map_err(|_| "Failed to parse cycle version as u16".to_string())?;
        let cycle_counter = if parts.len() == 4 {
            parts[3]
                .parse::<u16>()
                .map_err(|_| "Failed to parse cycle counter version as u16".to_string())?
        } else {
            0
        };

        Ok(PackageVersion {
            year,
            month,
            cycle,
            cycle_counter,
        })
    }
}

impl Display for PackageVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.year, self.month, self.cycle, self.cycle_counter
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_minimum() {
        let version1 = PackageVersion::new(2023, 10, 2, 0);
        let version2 = PackageVersion::new(2023, 11, 1, 0);
        let version3 = PackageVersion::new(2023, 10, 3, 0);
        let version4 = PackageVersion::new(2022, 10, 2, 0);
        let version5 = PackageVersion::new(2023, 10, 2, 1);

        assert!(version1.is_minimum(&version4));
        assert!(!version1.is_minimum(&version2));
        assert!(version1.is_minimum(&version1));
        assert!(!version1.is_minimum(&version5));
        assert!(version3.is_minimum(&version1));
    }

    #[test]
    fn test_is_maximum() {
        let version1 = PackageVersion::new(2023, 10, 2, 0);
        let version2 = PackageVersion::new(2023, 11, 1, 0);
        let version3 = PackageVersion::new(2023, 10, 1, 0);
        let version4 = PackageVersion::new(2024, 1, 1, 0);
        let version5 = PackageVersion::new(2023, 10, 2, 1);

        assert!(version1.is_maximum(&version1));
        assert!(!version1.is_maximum(&version3));
        assert!(version1.is_maximum(&version2));
        assert!(version1.is_maximum(&version4));
        assert!(version1.is_maximum(&version5));
    }

    #[test]
    fn test_is_exact() {
        let version1 = PackageVersion::new(2023, 10, 2, 3);
        let version2 = PackageVersion::new(2023, 10, 2, 3);
        let version3 = PackageVersion::new(2023, 10, 3, 0);
        assert!(version1.is_exact(&version2));
        assert!(!version1.is_exact(&version3));
    }

    #[test]
    fn test_try_from_valid_string() {
        let version = PackageVersion::try_from("2023.10.2.1".to_string()).unwrap();
        assert_eq!(version.year, 2023);
        assert_eq!(version.month, 10);
        assert_eq!(version.cycle, 2);
        assert_eq!(version.cycle_counter, 1);
    }

    #[test]
    fn test_try_from_valid_string_without_cycle_counter() {
        let version = PackageVersion::try_from("2023.10.2".to_string()).unwrap();
        assert_eq!(version.year, 2023);
        assert_eq!(version.month, 10);
        assert_eq!(version.cycle, 2);
        assert_eq!(version.cycle_counter, 0);
    }

    #[test]
    fn test_try_from_invalid_string() {
        let result = PackageVersion::try_from("2023.10".to_string());
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Invalid version format. Expected year.month.cycle[.cycle_counter]"
        );
    }

    #[test]
    fn test_display() {
        let version = PackageVersion::new(2023, 10, 2, 3);
        assert_eq!(version.to_string(), "2023.10.2.3");
    }

    #[test]
    fn test_version_parsing_from_node_response() {
        // Test parsing various version strings that might come from nodes
        let test_cases = vec![
            ("2025.1.0.1", PackageVersion::new(2025, 1, 0, 1)),
            ("2024.12.5.0", PackageVersion::new(2024, 12, 5, 0)),
            ("2023.7.2", PackageVersion::new(2023, 7, 2, 0)),
        ];

        for (version_str, expected) in test_cases {
            let parsed = PackageVersion::try_from(version_str.to_string()).unwrap();
            assert_eq!(
                parsed, expected,
                "Failed to parse version string: {version_str}"
            );
        }
    }
}
