//! Loose address comparison between calendar locations and trip addresses,
//! which rarely have exactly the same spelling.

use crate::trips::Position;

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

fn is_postal_code(w: &str) -> bool {
    w.len() == 4 && w.chars().all(|c| c.is_ascii_digit())
}

fn postal_code(normalized: &str) -> Option<&str> {
    normalized.split([' ', ',']).find(|w| is_postal_code(w))
}

/// The street name and house number of a normalised street part. The
/// number is the last word when it starts with a digit ("vestergade 12",
/// "vestergade 12a"), possibly followed by a single letter
/// ("vestergade 12 b"); a digit inside the name ("christian 8 s vej") is
/// not a number.
fn street_and_number(street: &str) -> (&str, Option<&str>) {
    let words: Vec<&str> = street.split(' ').collect();
    let starts_with_digit = |w: &str| w.starts_with(|c: char| c.is_ascii_digit());
    let at = match words.as_slice() {
        [.., n] if starts_with_digit(n) => words.len() - 1,
        [.., n, letter] if starts_with_digit(n) && letter.chars().count() == 1 => words.len() - 2,
        // A floor and door written without commas after the number
        // ("arendalsvej 192 2 tv"): the number is the word before them.
        _ => match (1..words.len()).find(|&i| {
            starts_with_digit(words[i]) && words[i + 1..].iter().all(|w| is_floor_or_door(w))
        }) {
            Some(i) if i + 1 < words.len() => i,
            _ => return (street, None),
        },
    };
    if at == 0 {
        return (street, None);
    }
    let name_len = words[..at].join(" ").len();
    let number = words[at];
    (&street[..name_len], Some(number))
}

/// A floor or door word in a Danish address: "2", "st", "kl", "tv", "th",
/// "mf", "sal" or a single letter.
fn is_floor_or_door(w: &str) -> bool {
    matches!(w, "st" | "kl" | "tv" | "th" | "mf" | "sal")
        || (w.len() <= 2 && w.chars().all(|c| c.is_ascii_digit()))
        || w.chars().count() == 1
}

/// The street part and the rest of a normalised address: before and after
/// the first comma, or, when it has no commas ("arendalsvej 192 2 tv 8600
/// silkeborg"), before and from the postal code.
fn split_street(n: &str) -> Option<(&str, &str)> {
    if let Some(parts) = n.split_once(',') {
        return Some(parts);
    }
    let at = n.split(' ').position(is_postal_code)?;
    if at == 0 {
        return None;
    }
    let street_len = n.split(' ').take(at).collect::<Vec<_>>().join(" ").len();
    Some((&n[..street_len], &n[street_len + 1..]))
}

/// The words after the street part that are not a postal code (town names,
/// floor and door).
fn town_words(rest: &str) -> Vec<&str> {
    rest.split([' ', ','])
        .filter(|w| !w.is_empty() && !is_postal_code(w))
        .collect()
}

/// True when `a` and `b` name the same place: identical after
/// normalisation, or the same street (the part before the first comma) and
/// no conflicting postal code. Two house numbers must be equal; when one
/// side has none (trip exports often leave it out), the street names are
/// compared. Without a postal code on either side, the town parts must
/// share a word.
pub fn matches(a: &str, b: &str) -> bool {
    let (na, nb) = (normalize(a), normalize(b));
    if na.is_empty() || nb.is_empty() {
        return false;
    }
    if na == nb {
        return true;
    }
    let (Some((street_a, rest_a)), Some((street_b, rest_b))) =
        (split_street(&na), split_street(&nb))
    else {
        return false;
    };
    let postal_ok = match (postal_code(&na), postal_code(&nb)) {
        (Some(pa), Some(pb)) => pa == pb,
        (None, None) => {
            let (ta, tb) = (town_words(rest_a), town_words(rest_b));
            ta.is_empty() || tb.is_empty() || ta.iter().any(|w| tb.contains(w))
        }
        _ => true,
    };
    let street_ok = match (street_and_number(street_a), street_and_number(street_b)) {
        ((name_a, Some(num_a)), (name_b, Some(num_b))) => name_a == name_b && num_a == num_b,
        ((name_a, _), (name_b, _)) => name_a == name_b,
    };
    street_ok && postal_ok
}

/// Configured places with coordinates (`[places]`): a trip end within
/// `radius_m` of a place matches that place's address.
#[derive(Debug, Clone, Default)]
pub struct Places {
    /// Each place's address with its coordinates.
    pub known: Vec<(String, Position)>,
    pub radius_m: f64,
}

impl Places {
    /// Whether a trip end (`trip_address`, at `trip_pos` when known) is at
    /// `place`: the addresses match, or `place` is a configured place and
    /// the trip end is within the radius of its coordinates.
    pub fn at(&self, trip_address: &str, trip_pos: Option<Position>, place: &str) -> bool {
        matches(trip_address, place)
            || trip_pos.is_some_and(|pos| {
                self.known
                    .iter()
                    .any(|(addr, at)| distance_m(pos, *at) <= self.radius_m && matches(addr, place))
            })
    }
}

/// Great-circle distance in metres.
pub fn distance_m(a: Position, b: Position) -> f64 {
    const EARTH_RADIUS_M: f64 = 6_371_000.0;
    let (lat_a, lat_b) = (a.lat.to_radians(), b.lat.to_radians());
    let d_lat = lat_b - lat_a;
    let d_lon = (b.lon - a.lon).to_radians();
    let h = (d_lat / 2.0).sin().powi(2) + lat_a.cos() * lat_b.cos() * (d_lon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::{Places, matches};
    use crate::trips::Position;

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

    #[test]
    fn address_written_without_punctuation() {
        // As typed into the phone app's Settings.
        let home = "Arendalsvej 192 2 tv 8600 Silkeborg";
        assert!(matches("Arendalsvej, 8600 Silkeborg", home));
        assert!(matches("Arendalsvej 192, 2. tv., 8600 Silkeborg", home));
        assert!(matches("Arendalsvej, 8600 Gødvad", home));
        assert!(!matches("Arendalsvej 12, 8600 Silkeborg", home));
        assert!(!matches("Arendalsvej, 7400 Herning", home));
        assert!(matches(
            "Lysbrohøjen, Lysbro",
            "Lysbrohøjen 3 8600 Silkeborg"
        ));
        // A digit in the street name is not taken for a house number.
        assert!(matches(
            "Christian 8.s Vej 10, 8600 Silkeborg",
            "Christian 8 s Vej 10 8600 Silkeborg"
        ));
    }

    #[test]
    fn street_without_house_number() {
        let work = "Lysbrohøjen 3, 8600 Silkeborg";
        let home = "Arendalsvej 192, 2. tv., 8600 Silkeborg";
        assert!(matches("Lysbrohøjen, Lysbro", work));
        assert!(matches("Arendalsvej, 8600 Gødvad", home));
        assert!(matches(home, "Arendalsvej, 8600 Silkeborg"));
        assert!(matches(
            "Christian 8.s Vej, 8600 Silkeborg",
            "Christian 8.s Vej 10, 8600 Silkeborg"
        ));
        // A different house number, or the same street elsewhere, is another place.
        assert!(!matches("Arendalsvej 12, 8600 Silkeborg", home));
        assert!(!matches("Arendalsvej, 7100 Vejle", home));
        assert!(!matches(
            "Christian 8.s Vej 12, 8600 Silkeborg",
            "Christian 8.s Vej 10, 8600 Silkeborg"
        ));
        assert!(!matches("Pedersmindevej 16, 8600 Silkeborg", work));
        // Without postal codes the towns must agree.
        assert!(matches("Alsvej, Hadsund", "Alsvej 3, Hadsund"));
        assert!(!matches("Alsvej, Hadsund", "Alsvej 3, Aalborg"));
    }

    #[test]
    fn trip_end_near_place_coordinates() {
        let work = "Lysbrohøjen 3, 8600 Silkeborg";
        let at = Position {
            lat: 56.0,
            lon: 9.5,
        };
        let places = Places {
            known: vec![(work.into(), at)],
            radius_m: 150.0,
        };
        // 0.0009° latitude ≈ 100 m, 0.0027° ≈ 300 m.
        let near = Position {
            lat: 56.0009,
            lon: 9.5,
        };
        let far = Position {
            lat: 56.0027,
            lon: 9.5,
        };
        assert!(places.at("Vestre Ringvej, Silkeborg", Some(near), work));
        assert!(!places.at("Vestre Ringvej, Silkeborg", Some(far), work));
        assert!(!places.at("Vestre Ringvej, Silkeborg", None, work));
        // Only the place with those coordinates.
        assert!(!places.at(
            "Vestre Ringvej, Silkeborg",
            Some(near),
            "Torvet 1, 8600 Silkeborg"
        ));
    }
}
