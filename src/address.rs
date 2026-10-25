//! Loose address comparison between calendar locations and trip addresses,
//! which rarely have exactly the same spelling.

/// Lowercase, strip punctuation except commas, drop the country name.
fn normalize(s: &str) -> String {
    let cleaned: String = s
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ',' {
                c
            } else {
                ' '
            }
        })
        .collect();
    cleaned
        .split(',')
        .map(|part| {
            part.split_whitespace()
                .filter(|w| !matches!(*w, "danmark" | "denmark" | "dk"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

fn postal_code(normalized: &str) -> Option<&str> {
    normalized
        .split([' ', ','])
        .find(|w| w.len() == 4 && w.chars().all(|c| c.is_ascii_digit()))
}

/// True when `a` and `b` name the same place: identical after
/// normalisation, or same street and number (the part before the first
/// comma) and no conflicting postal code.
pub fn matches(a: &str, b: &str) -> bool {
    let (na, nb) = (normalize(a), normalize(b));
    if na.is_empty() || nb.is_empty() {
        return false;
    }
    if na == nb {
        return true;
    }
    let (Some((street_a, _)), Some((street_b, _))) = (na.split_once(','), nb.split_once(','))
    else {
        return false;
    };
    let postal_ok = match (postal_code(&na), postal_code(&nb)) {
        (Some(pa), Some(pb)) => pa == pb,
        _ => true,
    };
    street_a == street_b && postal_ok
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn address_variants() {
        assert!(matches(
            "Vestergade 12, 5000 Odense C, Danmark",
            "vestergade 12, 5000 Odense"
        ));
        assert!(matches("Kontor", "kontor"));
        assert!(matches(
            "Østergade 1, 8000 Aarhus C",
            "Østergade 1,  8000  Aarhus"
        ));
        assert!(!matches(
            "Vestergade 12, 5000 Odense C",
            "Vestergade 12, 8000 Aarhus C"
        ));
        assert!(!matches(
            "Vestergade 12, 5000 Odense C",
            "Vestergade 14, 5000 Odense C"
        ));
        assert!(!matches("", ""));
    }
}
